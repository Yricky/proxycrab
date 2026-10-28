//! Connection-lived Lua callbacks. Effects commit only after a successful invocation.
use super::*;
use mlua::{IntoLua, MetaMethod};
use std::collections::HashMap;

pub(super) const REGISTRATION: &str = "proxycrab.ws.callback";
pub(crate) const QUEUE_LIMIT: usize = 16_384;

#[derive(Clone)]
pub(crate) struct WsMessageInput {
    pub side: BodySide,
    pub metadata: crate::websocket::WebSocketMessage,
    pub body: WsBody,
}

#[derive(Clone)]
pub(crate) enum WsBody {
    Ready(Arc<[u8]>),
    Lazy {
        read: Arc<dyn Fn() -> anyhow::Result<Option<Vec<u8>>> + Send + Sync>,
        cached: Arc<OnceLock<Result<Option<Arc<[u8]>>, String>>>,
    },
}

impl From<Arc<[u8]>> for WsBody {
    fn from(body: Arc<[u8]>) -> Self {
        Self::Ready(body)
    }
}

impl WsBody {
    pub(crate) fn lazy(
        read: impl Fn() -> anyhow::Result<Option<Vec<u8>>> + Send + Sync + 'static,
    ) -> Self {
        Self::Lazy {
            read: Arc::new(read),
            cached: Arc::new(OnceLock::new()),
        }
    }
    fn read(&self) -> mlua::Result<Option<Arc<[u8]>>> {
        match self {
            Self::Ready(body) => Ok(Some(body.clone())),
            Self::Lazy { read, cached } => cached
                .get_or_init(|| {
                    read()
                        .map(|body| body.map(Arc::from))
                        .map_err(|error| error.to_string())
                })
                .clone()
                .map_err(LuaError::runtime),
        }
    }
}

pub(crate) type WsHistory =
    Arc<dyn Fn(u64, usize) -> anyhow::Result<Vec<WsMessageInput>> + Send + Sync>;

#[derive(Debug)]
pub(crate) struct WsSend {
    pub side: BodySide,
    pub kind: String,
    pub body: Arc<[u8]>,
}

#[derive(Default, Debug)]
pub(crate) struct WsEffects {
    pub dropped: bool,
    pub sends: Vec<WsSend>,
}

pub(crate) struct WsCallback {
    lua: Lua,
    function: Function,
}

impl WsCallback {
    pub(super) fn new(lua: Lua, function: Function) -> Self {
        Self { lua, function }
    }

    pub(crate) fn call(
        &self,
        message: WsMessageInput,
        history: WsHistory,
        queued: usize,
    ) -> Result<WsEffects, String> {
        self.lua
            .app_data_ref::<InstructionCounter>()
            .unwrap()
            .reset();
        let pending = Arc::new(Mutex::new(Some(Pending {
            effects: WsEffects::default(),
            queued,
            overflow: false,
            bodies: HashMap::new(),
        })));
        let result = self.function.call::<()>(WsContext {
            message,
            history,
            pending: pending.clone(),
        });
        // Taking the transaction also invalidates any ws value retained by a closure.
        let staged = pending.lock().unwrap().take().unwrap();
        if staged.overflow {
            return Err("WebSocket pending queue limit exceeded (16384 messages)".into());
        }
        if self
            .lua
            .app_data_ref::<InstructionCounter>()
            .unwrap()
            .0
            .load(Ordering::Relaxed)
            > INSTRUCTION_LIMIT
        {
            return Err("Lua instruction limit exceeded".into());
        }
        result.map_err(|error| error.to_string())?;
        Ok(staged.effects)
    }
}

struct Pending {
    effects: WsEffects,
    queued: usize,
    overflow: bool,
    bodies: HashMap<mlua::LuaString, Arc<[u8]>>,
}

struct WsContext {
    message: WsMessageInput,
    history: WsHistory,
    pending: Arc<Mutex<Option<Pending>>>,
}

impl UserData for WsContext {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("message", |_, this| Ok(this.message.clone()));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("drop", |_, this, ()| {
            let mut guard = this.pending.lock().unwrap();
            let pending = guard.as_mut().ok_or_else(expired)?;
            pending.effects.dropped = true;
            Ok(())
        });
        methods.add_method(
            "send",
            |_, this, (direction, body, kind): (String, mlua::LuaString, Option<String>)| {
                let mut guard = this.pending.lock().unwrap();
                let pending = guard.as_mut().ok_or_else(expired)?;
                if pending.queued.saturating_add(pending.effects.sends.len()) >= QUEUE_LIMIT {
                    pending.overflow = true;
                    return Err(LuaError::runtime(
                        "WebSocket pending queue limit exceeded (16384 messages)",
                    ));
                }
                let side = match direction.as_str() {
                    "request" => BodySide::Request,
                    "response" => BodySide::Response,
                    _ => {
                        return Err(LuaError::runtime(
                            "send direction must be request (server) or response (client)",
                        ));
                    }
                };
                let kind = kind.unwrap_or_else(|| "text".into());
                if !matches!(kind.as_str(), "text" | "binary") {
                    return Err(LuaError::runtime("send type must be text or binary"));
                }
                if body.as_bytes().len() > LUA_BODY_LIMIT as usize {
                    return Err(LuaError::runtime("WebSocket message exceeds 16 MiB"));
                }
                if kind == "text" && std::str::from_utf8(&body.as_bytes()).is_err() {
                    return Err(LuaError::runtime("text message must be valid UTF-8"));
                }
                // Pin each unique Lua string only for this transaction; repeated sends
                // share host storage rather than multiplying large payload allocations.
                let bytes = pending
                    .bodies
                    .entry(body.clone())
                    .or_insert_with(|| Arc::from(body.as_bytes().as_ref()))
                    .clone();
                pending.effects.sends.push(WsSend {
                    side,
                    kind,
                    body: bytes,
                });
                Ok(())
            },
        );
        methods.add_method(
            "history",
            |_, this, (after, limit): (Option<u64>, Option<usize>)| {
                if this.pending.lock().unwrap().is_none() {
                    return Err(expired());
                }
                let messages =
                    (this.history)(after.unwrap_or(0), limit.unwrap_or(64).clamp(1, 256))
                        .map_err(LuaError::external)?;
                Ok(HistoryPage(messages))
            },
        );
    }
}

fn expired() -> LuaError {
    LuaError::runtime("WebSocket invocation has already completed")
}

impl UserData for WsMessageInput {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Index, |lua, this, key: String| {
            match key.as_str() {
                "body" => match this.body.read()? {
                    Some(body) => lua.create_string(body.as_ref())?.into_lua(lua),
                    None => Ok(Value::Nil),
                },
                "side" | "direction" => match this.side {
                    BodySide::Request => "request",
                    BodySide::Response => "response",
                }
                .into_lua(lua),
                _ => {
                    let metadata =
                        serde_json::to_value(&this.metadata).map_err(LuaError::external)?;
                    readonly_value(lua, metadata.get(&key).cloned().unwrap_or_default())
                }
            }
        });
    }
}

struct HistoryPage(Vec<WsMessageInput>);
impl UserData for HistoryPage {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Len, |_, this, ()| Ok(this.0.len()));
        methods.add_meta_method(MetaMethod::Index, |_, this, index: usize| {
            Ok(index.checked_sub(1).and_then(|i| this.0.get(i)).cloned())
        });
    }
}

// Metadata and provenance are recursively immutable, including nested error lists.
struct ReadonlyJson(serde_json::Value);
impl UserData for ReadonlyJson {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Len, |_, this, ()| {
            Ok(this.0.as_array().map_or(0, Vec::len))
        });
        methods.add_meta_method(MetaMethod::Index, |lua, this, key: Value| {
            let value = match key {
                Value::String(key) => this.0.get(key.to_str()?.as_ref()).cloned(),
                Value::Integer(key) if key > 0 => this.0.get(key as usize - 1).cloned(),
                _ => None,
            };
            readonly_value(lua, value.unwrap_or_default())
        });
    }
}
fn readonly_value(lua: &Lua, value: serde_json::Value) -> mlua::Result<Value> {
    match value {
        serde_json::Value::Null => Ok(Value::Nil),
        serde_json::Value::Bool(value) => value.into_lua(lua),
        serde_json::Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                value.into_lua(lua)
            } else {
                value.as_f64().into_lua(lua)
            }
        }
        serde_json::Value::String(value) => value.into_lua(lua),
        value => lua.create_userdata(ReadonlyJson(value))?.into_lua(lua),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn register(source: &str) -> (Option<WsCallback>, Option<String>) {
        let request = RequestData {
            method: "GET".into(),
            uri: "https://example.com/ws".into(),
            version: "HTTP/1.1".into(),
            headers: HeaderValues::new(),
            tags: RequestTags::new(),
        };
        let state = SharedInterceptorState::new_request(
            request.method.clone(),
            request.uri.clone(),
            request.headers.clone(),
            request.tags.clone(),
        );
        let (_, error, callback) = execute_request_with_ws(
            source,
            &request,
            state,
            ModificationJournal::for_request(&request, None),
            "test",
            None,
            None,
        )
        .unwrap();
        (callback, error)
    }

    fn message(id: u32) -> WsMessageInput {
        WsMessageInput {
            side: BodySide::Request,
            metadata: serde_json::from_value(serde_json::json!({
                "id":id, "type":"text", "started_at":1, "completed_at":2,
                "size":5, "content_available":true, "sequence":id,
                "producer":{"execution_id":3,"name":"producer","invocation_id":4,"send_index":1}
            }))
            .unwrap(),
            body: WsBody::Ready(Arc::from(b"hello".as_slice())),
        }
    }
    fn history() -> WsHistory {
        Arc::new(|_, _| Ok(vec![message(1)]))
    }

    #[test]
    fn registration_is_unique_and_uncaught_errors_cancel_it() {
        let (callback, error) =
            register("req:on_ws_message(function(ws) end); req:on_ws_message(function(ws) end)");
        assert!(callback.is_none());
        assert!(error.unwrap().contains("only be registered once"));
        let (callback, error) = register(
            "req:on_ws_message(function(ws) ws:drop() end); assert(not pcall(function() req:on_ws_message(function(ws) end) end))",
        );
        assert!(error.is_none());
        assert!(
            callback
                .unwrap()
                .call(message(2), history(), 0)
                .unwrap()
                .dropped
        );
        let (callback, error) =
            register("req:on_ws_message(function(ws) end); error('init failed')");
        assert!(callback.is_none());
        assert!(error.unwrap().contains("init failed"));
    }

    #[test]
    fn captures_survive_and_current_history_and_request_aliases_are_readonly() {
        let (callback, error) = register(
            r#"
            local n = 0
            local request, headers, body = req, req.headers, req.body
            req:on_ws_message(function(ws)
                n = n + 1
                assert(request.method == 'GET')
                assert(not pcall(function() request.method = 'POST' end))
                assert(not pcall(function() request.uri = '/changed' end))
                assert(not pcall(function() request:set_tag('x', 'y') end))
                assert(not pcall(function() headers:set('x', 'y') end))
                assert(not pcall(function() headers:append('x', 'y') end))
                assert(not pcall(function() headers:remove('x') end))
                assert(not pcall(function() body:replace_with_string('changed') end))
                assert(not pcall(function() req:on_ws_message(function() end) end))
                assert(ws.message.body == 'hello' and ws.message.direction == 'request')
                assert(not pcall(function() ws.message.body = 'changed' end))
                local h = ws:history()
                assert(#h == 1 and h[1].id == 1)
                assert(not pcall(function() h[1] = ws.message end))
                assert(not pcall(function() h[1].body = 'changed' end))
                assert(not pcall(function() h[1].producer.name = 'changed' end))
                ws:send('response', tostring(n))
            end)
        "#,
        );
        assert!(error.is_none());
        let callback = callback.unwrap();
        for count in 1..=2 {
            let effects = callback.call(message(2), history(), 0).unwrap();
            assert_eq!(effects.sends[0].body.as_ref(), count.to_string().as_bytes());
            assert_eq!(effects.sends[0].side, BodySide::Response);
        }
    }

    #[test]
    fn failures_rollback_effects_but_next_invocation_runs_with_closure_state() {
        let (callback, _) = register(
            r#"
            local count = 0
            req:on_ws_message(function(ws)
                count = count + 1
                ws:drop()
                ws:send('request', tostring(count))
                if count == 1 then error('first failed') end
            end)
        "#,
        );
        let callback = callback.unwrap();
        assert!(
            callback
                .call(message(1), history(), 0)
                .unwrap_err()
                .contains("first failed")
        );
        let effects = callback.call(message(2), history(), 0).unwrap();
        assert!(effects.dropped);
        assert_eq!(effects.sends[0].body.as_ref(), b"2");
    }

    #[test]
    fn queue_limit_counts_staged_sends_and_cannot_be_caught_to_commit() {
        let (callback, _) = register(
            r#"
            req:on_ws_message(function(ws)
                ws:drop()
                assert(pcall(function() ws:send('request', 'first') end))
                assert(not pcall(function() ws:send('response', 'second') end))
            end)
        "#,
        );
        let error = callback
            .unwrap()
            .call(message(1), history(), QUEUE_LIMIT - 1)
            .unwrap_err();
        assert!(error.contains("queue limit"));
        let (callback, _) =
            register("req:on_ws_message(function(ws) ws:send('request', 'last') end)");
        assert_eq!(
            callback
                .unwrap()
                .call(message(1), history(), QUEUE_LIMIT - 1)
                .unwrap()
                .sends
                .len(),
            1
        );
    }

    #[test]
    fn old_context_cannot_emit_or_drop_in_later_invocations() {
        let (callback, _) = register(
            r#"
            local old
            req:on_ws_message(function(ws)
                if old then
                    assert(not pcall(function() old:send('response', 'stale') end))
                    assert(not pcall(function() old:drop() end))
                    assert(not pcall(function() old:history() end))
                end
                old = ws
            end)
        "#,
        );
        let callback = callback.unwrap();
        callback.call(message(1), history(), 0).unwrap();
        let effects = callback.call(message(2), history(), 0).unwrap();
        assert!(!effects.dropped && effects.sends.is_empty());
    }

    #[test]
    fn instruction_limit_resets_between_invocations() {
        let (callback, _) = register(
            r#"
            local n = 0
            req:on_ws_message(function(ws)
                n = n + 1
                if n == 1 then while true do end end
                ws:send('response', 'recovered')
            end)
        "#,
        );
        let callback = callback.unwrap();
        assert!(
            callback
                .call(message(1), history(), 0)
                .unwrap_err()
                .contains("instruction limit")
        );
        assert_eq!(
            callback.call(message(2), history(), 0).unwrap().sends[0]
                .body
                .as_ref(),
            b"recovered"
        );
    }
    #[test]
    fn history_bodies_are_lazy_cached_and_optional() {
        let reads = Arc::new(AtomicU32::new(0));
        let history_reads = reads.clone();
        let history: WsHistory = Arc::new(move |after, limit| {
            assert_eq!((after, limit), (0, 64));
            let reads = history_reads.clone();
            let mut first = message(1);
            first.body = WsBody::lazy(move || {
                reads.fetch_add(1, Ordering::Relaxed);
                Ok(Some(b"lazy".to_vec()))
            });
            let mut second = message(2);
            second.body = WsBody::lazy(|| Ok(None));
            Ok(vec![first, second])
        });
        let (callback, _) = register(
            "req:on_ws_message(function(ws) local h = ws:history(); assert(h[1].id == 1) end)",
        );
        callback
            .unwrap()
            .call(message(3), history.clone(), 0)
            .unwrap();
        assert_eq!(reads.load(Ordering::Relaxed), 0);
        let (callback, _) = register(
            "req:on_ws_message(function(ws) local h = ws:history(); assert(h[1].body == 'lazy'); assert(h[1].body == 'lazy'); assert(h[2].body == nil) end)",
        );
        callback.unwrap().call(message(3), history, 0).unwrap();
        assert_eq!(reads.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn binary_send_preserves_bytes_and_text_rejects_invalid_utf8() {
        let (callback, _) = register(
            r#"
            req:on_ws_message(function(ws)
                local bytes = string.char(0, 255, 128)
                assert(not pcall(function() ws:send('request', bytes, 'text') end))
                ws:send('request', bytes, 'binary')
            end)
        "#,
        );
        let effects = callback.unwrap().call(message(1), history(), 0).unwrap();
        assert_eq!(effects.sends[0].kind, "binary");
        assert_eq!(effects.sends[0].body.as_ref(), &[0, 255, 128]);
    }
    #[test]
    fn captured_breakpoint_is_disabled_after_request_execution() {
        let root = tempfile::tempdir().unwrap();
        let store = crate::storage::CaptureStore::open(1, root.path()).unwrap();
        let request = RequestData {
            method: "GET".into(),
            uri: "http://example.com/ws".into(),
            version: "HTTP/1.1".into(),
            headers: HeaderValues::new(),
            tags: RequestTags::new(),
        };
        let state = SharedInterceptorState::new_request(
            request.method.clone(),
            request.uri.clone(),
            request.headers.clone(),
            request.tags.clone(),
        );
        let journal = ModificationJournal::for_request(&request, None);
        let hook = BreakpointHook {
            registry: Arc::new(BreakpointRegistry::default()),
            context: BreakpointContext {
                session_id: 1,
                capture_id: 1,
                phase: crate::model::InterceptorKind::Request,
                position: 0,
                interceptor_name: "test".into(),
                parent_execution_id: 1,
                request: request.clone(),
                response: None,
                state: state.clone(),
                parent_journal: journal.clone(),
                store,
            },
        };
        let (_, error, callback) = execute_request_with_ws(
            "local pause = breakpoint; req:on_ws_message(function(ws) local ok, err = pcall(pause, 1000); assert(not ok and string.find(tostring(err), 'read%-only')) end)",
            &request, state, journal, "test", None, Some(hook),
        ).unwrap();
        assert!(error.is_none());
        callback.unwrap().call(message(1), history(), 0).unwrap();
    }
    #[test]
    fn temporary_execution_rejects_registration_without_freezing_shared_state() {
        let request = RequestData {
            method: "GET".into(),
            uri: "http://example.com/ws".into(),
            version: "HTTP/1.1".into(),
            headers: HeaderValues::new(),
            tags: RequestTags::new(),
        };
        let state = SharedInterceptorState::new_request(
            request.method.clone(),
            request.uri.clone(),
            request.headers.clone(),
            request.tags.clone(),
        );
        let journal = ModificationJournal::for_request(&request, None);
        let (_, error) = execute_request_with_state(
            "req:on_ws_message(function(ws) end)",
            &request,
            state.clone(),
            journal.clone(),
            "temporary",
            None,
            None,
        )
        .unwrap();
        assert!(
            error
                .unwrap()
                .contains("only available during a connection")
        );
        let (effects, error) = execute_request_with_state(
            "req.method = 'POST'; req.headers:set('x-test', 'ok')",
            &request,
            state,
            journal,
            "temporary",
            None,
            None,
        )
        .unwrap();
        assert!(error.is_none());
        assert_eq!(effects.method.as_deref(), Some("POST"));
        assert_eq!(effects.headers.get("x-test").unwrap(), &["ok"]);
    }
    #[test]
    fn repeated_sends_share_payload_storage() {
        let (callback, _) = register(
            r#"
            req:on_ws_message(function(ws)
                local body = string.rep('x', 1024 * 1024)
                ws:send('request', body)
                ws:send('response', body, 'binary')
                ws:send('request', string.rep('x', 1024 * 1024))
            end)
        "#,
        );
        let effects = callback.unwrap().call(message(1), history(), 0).unwrap();
        assert_eq!(effects.sends.len(), 3);
        assert!(Arc::ptr_eq(&effects.sends[0].body, &effects.sends[1].body));
        assert!(Arc::ptr_eq(&effects.sends[0].body, &effects.sends[2].body));
    }
}
