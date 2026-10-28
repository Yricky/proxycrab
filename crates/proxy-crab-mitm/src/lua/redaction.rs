//! Read-only originals and transactional, staged output. Never used by forwarding.
use super::*;
use mlua::MetaMethod;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartState {
    Unloaded,
    Loaded,
    Redacted,
}
impl PartState {
    fn name(self) -> &'static str {
        match self {
            Self::Unloaded => "unloaded",
            Self::Loaded => "loaded",
            Self::Redacted => "redacted",
        }
    }
}

/// One compiled evaluator per API request (or HAR download), with fresh globals per invocation.
#[derive(Clone)]
pub struct RedactionEvaluator(Arc<Mutex<ReusableScript>>, bool);
impl RedactionEvaluator {
    pub fn new(source: &str, name: &str) -> Result<Self> {
        Ok(Self(
            Arc::new(Mutex::new(ReusableScript::new(
                source,
                ScriptKind::Redaction,
                name,
            )?)),
            source
                .lines()
                .any(|line| line.trim() == "-- @proxycrab websocket-messages"),
        ))
    }
}

/// Message policies explicitly opt in with `-- @proxycrab websocket-messages`.
/// Unlike HTTP staged reads, each invocation has exactly one fully loaded body.
impl RedactionEvaluator {
    pub fn websocket_message(
        &self,
        api: &str,
        uri: &str,
        side: BodySide,
        metadata: &serde_json::Value,
        bytes: Vec<u8>,
    ) -> Result<Option<Vec<u8>>> {
        if !self.1 {
            bail!("policy does not support websocket messages");
        }
        if bytes.len() as u64 > LUA_BODY_LIMIT {
            bail!("message exceeds body limit");
        }
        let output = Arc::new(Mutex::new(Some(bytes.clone())));
        let body = WebSocketRedactionBody {
            original: Arc::new(bytes),
            output: output.clone(),
        };
        let script = self.0.lock().expect("redaction evaluator lock");
        script.lua.gc_restart();
        script
            .lua
            .app_data_ref::<InstructionCounter>()
            .expect("instruction counter")
            .reset();
        let _ = script.warnings.take();
        let env = script
            .environment_factory
            .call::<Table>(script.lua.globals())?;
        let json: Table = script.lua.globals().get("json")?;
        let context: Table = json
            .get::<Function>("decode")?
            .call(serde_json::to_string(metadata)?)?;
        context.set("api", api)?;
        context.set("kind", "websocket_message")?;
        // Read-only proxy: scripts cannot replace context fields or the message's body.
        let context = WebSocketRedactionContext(context);
        env.set("context", context)?;
        env.set("req", Value::Nil)?;
        env.set("resp", Value::Nil)?;
        env.set(
            match side {
                BodySide::Request => "req",
                BodySide::Response => "resp",
            },
            WebSocketRedactionMessage {
                uri: uri.to_owned(),
                body,
            },
        )?;
        if !script.function.set_environment(env)? {
            bail!("redaction script has no environment");
        }
        let result = script.function.call::<()>(());
        let _ = script.warnings.take();
        result?;
        let result = output.lock().expect("message output lock").clone();
        if result
            .as_ref()
            .is_some_and(|b| b.len() as u64 > LUA_BODY_LIMIT)
        {
            bail!("replacement exceeds body limit");
        }
        Ok(result)
    }
}
#[derive(Clone)]
struct WebSocketRedactionContext(Table);
impl UserData for WebSocketRedactionContext {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Index, |_, this, key: String| {
            this.0.get::<Value>(key)
        });
        methods.add_meta_method(
            MetaMethod::NewIndex,
            |_, _, _: (Value, Value)| -> mlua::Result<()> {
                Err(LuaError::runtime("WebSocket context is read-only"))
            },
        );
    }
}
#[derive(Clone)]
struct WebSocketRedactionMessage {
    uri: String,
    body: WebSocketRedactionBody,
}
impl UserData for WebSocketRedactionMessage {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("uri", |_, this| Ok(UriView::from(&this.uri)));
        fields.add_field_method_get("version", |_, _| Ok("WebSocket/13"));
        for name in ["headers", "method", "status"] {
            fields.add_field_method_get(name, |_, _| Ok(Value::Nil));
        }
        fields.add_field_method_get("body", |_, this| Ok(this.body.clone()));
    }
}
#[derive(Clone)]
struct WebSocketRedactionBody {
    original: Arc<Vec<u8>>,
    output: Arc<Mutex<Option<Vec<u8>>>>,
}
impl UserData for WebSocketRedactionBody {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("as_string", |lua, this, ()| {
            std::str::from_utf8(&this.original).map_err(LuaError::external)?;
            lua.create_string(this.original.as_slice())
        });
        methods.add_method("as_bytes", |lua, this, ()| {
            lua.create_string(this.original.as_slice())
        });
        methods.add_method("as_json", |lua, this, ()| {
            let _: serde_json::Value =
                serde_json::from_slice(&this.original).map_err(LuaError::external)?;
            let json: Table = lua.globals().get("json")?;
            json.get::<Function>("decode")?
                .call::<Value>(lua.create_string(this.original.as_slice())?)
        });
        for name in ["replace_with_string", "replace_with_bytes"] {
            methods.add_method(name, |_, this, value: mlua::LuaString| {
                if value.as_bytes().len() as u64 > LUA_BODY_LIMIT {
                    return Err(LuaError::runtime("body exceeds limit"));
                }
                *this.output.lock().expect("message output lock") = Some(value.as_bytes().to_vec());
                Ok(())
            });
        }
        methods.add_method("replace_with_json", |lua, this, value: Value| {
            let json: Table = lua.globals().get("json")?;
            let encoded = json
                .get::<Function>("encode")?
                .call::<mlua::LuaString>(value)?;
            if encoded.as_bytes().len() as u64 > LUA_BODY_LIMIT {
                return Err(LuaError::runtime("body exceeds limit"));
            }
            *this.output.lock().expect("message output lock") = Some(encoded.as_bytes().to_vec());
            Ok(())
        });
        methods.add_method("hide", |_, this, ()| {
            *this.output.lock().expect("message output lock") = None;
            Ok(())
        });
    }
}

#[derive(Clone)]
pub struct RedactedCapture(Arc<Mutex<Flow>>);
struct Flow {
    evaluator: RedactionEvaluator,
    api: String,
    original: Arc<CaptureSummary>,
    output: CaptureSummary,
    states: [PartState; 4],
    resolver: Arc<BodySourceResolver>,
    sources: Arc<[OnceLock<Option<BodySource>>; 2]>,
    raw: CaptureBodyAccess,
    read_failed: Arc<std::sync::atomic::AtomicBool>,
    replacements: [Option<String>; 2],
    uri_only: bool,
    failed: bool,
}

impl RedactedCapture {
    pub fn new<F>(
        evaluator: RedactionEvaluator,
        api: &str,
        original: CaptureSummary,
        resolver: F,
    ) -> Result<Self>
    where
        F: Fn(BodySide) -> Result<Option<BodySource>> + Send + Sync + 'static,
    {
        Self::create(evaluator, api, original, Arc::new(resolver), false, true)
    }
    pub fn snapshot<F>(
        evaluator: RedactionEvaluator,
        api: &str,
        original: CaptureSummary,
        request_present: bool,
        resolver: F,
    ) -> Result<Self>
    where
        F: Fn(BodySide) -> Result<Option<BodySource>> + Send + Sync + 'static,
    {
        Self::create(
            evaluator,
            api,
            original,
            Arc::new(resolver),
            false,
            request_present,
        )
    }
    pub fn uri_only(evaluator: RedactionEvaluator, api: &str, uri: &str) -> Result<Self> {
        let original = CaptureSummary {
            id: 0,
            session_id: 0,
            source: String::new(),
            request: RequestData {
                method: String::new(),
                uri: uri.into(),
                version: String::new(),
                headers: HeaderValues::new(),
                tags: RequestTags::new(),
            },
            response: None,
            outcome: CaptureOutcome::Success,
            stage: String::new(),
            error: None,
            created_at: 0,
            updated_at: 0,
        };
        Self::create(evaluator, api, original, Arc::new(|_| Ok(None)), true, true)
    }
    fn create(
        evaluator: RedactionEvaluator,
        api: &str,
        original: CaptureSummary,
        resolver: Arc<BodySourceResolver>,
        uri_only: bool,
        request_present: bool,
    ) -> Result<Self> {
        let response = original.response.is_some();
        let sources: Arc<[OnceLock<Option<BodySource>>; 2]> =
            Arc::new([OnceLock::new(), OnceLock::new()]);
        let read_failed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let slots = sources.clone();
        let failure = read_failed.clone();
        let raw = CaptureBodyAccess::new(move |side| {
            let result = slots[side_index(side)]
                .get()
                .cloned()
                .flatten()
                .map(strict_source)
                .transpose();
            if result.is_err() {
                failure.store(true, Ordering::Relaxed);
            }
            result
        });
        let mut flow = Flow {
            evaluator,
            api: api.into(),
            output: original.clone(),
            original: Arc::new(original),
            states: [
                if request_present {
                    PartState::Loaded
                } else {
                    PartState::Unloaded
                },
                PartState::Unloaded,
                if response {
                    PartState::Loaded
                } else {
                    PartState::Unloaded
                },
                PartState::Unloaded,
            ],
            resolver,
            sources,
            raw,
            read_failed,
            replacements: [None, None],
            uri_only,
            failed: false,
        };
        flow.run()?;
        Ok(Self(Arc::new(Mutex::new(flow))))
    }
    pub fn ensure_success(&self) -> Result<()> {
        if self.0.lock().expect("redaction flow lock").failed {
            bail!("redaction failed");
        }
        Ok(())
    }
    pub fn summary(&self) -> CaptureSummary {
        let flow = self.0.lock().expect("redaction flow lock");
        let mut output = flow.output.clone();
        for side in 0..2 {
            if flow.replacements[side].is_some() || flow.cached_bytes(side).is_some() {
                let headers = if side == 0 {
                    Some(&mut output.request.headers)
                } else {
                    output.response.as_mut().map(|r| &mut r.headers)
                };
                if let Some(headers) = headers {
                    remove_header(headers, "content-encoding");
                    remove_header(headers, "content-length");
                }
            }
        }
        output
    }
    pub fn body_access(&self) -> CaptureBodyAccess {
        let view = self.clone();
        CaptureBodyAccess::new(move |side| view.body_source(side))
    }
    pub fn body_source(&self, side: BodySide) -> Result<Option<BodySource>> {
        let index = side_index(side);
        let mut flow = self.0.lock().expect("redaction flow lock");
        if flow.failed {
            bail!("redaction failed");
        }
        let result = (|| {
            if flow.uri_only || flow.states[index * 2] == PartState::Unloaded {
                return Ok(None);
            }
            if flow.states[index * 2 + 1] != PartState::Redacted {
                if flow.states[index * 2 + 1] == PartState::Unloaded {
                    let source = (flow.resolver)(side)?;
                    let _ = flow.sources[index].set(source);
                    flow.states[index * 2 + 1] = PartState::Loaded;
                }
                flow.run()?;
            }
            let headers = if index == 0 {
                &flow.output.request.headers
            } else {
                &flow
                    .output
                    .response
                    .as_ref()
                    .expect("response exists")
                    .headers
            };
            let content_type = header_values(headers, "content-type")
                .and_then(|values| values.first())
                .cloned();
            if let Some(content) = &flow.replacements[index] {
                return Ok(Some(BodySource {
                    data: BodySourceData::Bytes(content.as_bytes().to_vec()),
                    path: None,
                    stored_size: content.len() as u64,
                    content_type,
                    content_encodings: Vec::new(),
                }));
            }
            if let Some(bytes) = flow.cached_bytes(index) {
                return Ok(Some(BodySource {
                    data: BodySourceData::Bytes(bytes.to_vec()),
                    path: None,
                    stored_size: bytes.len() as u64,
                    content_type,
                    content_encodings: Vec::new(),
                }));
            }
            Ok(flow.sources[index]
                .get()
                .cloned()
                .flatten()
                .map(|mut source| {
                    source.content_type = content_type;
                    source
                }))
        })();
        if result.is_err() {
            flow.failed = true;
        }
        result
    }
}
fn side_index(side: BodySide) -> usize {
    match side {
        BodySide::Request => 0,
        BodySide::Response => 1,
    }
}

#[derive(Clone)]
struct Invocation {
    original: Arc<CaptureSummary>,
    states: [PartState; 4],
    raw: CaptureBodyAccess,
    output: Arc<Mutex<(CaptureSummary, [Option<String>; 2])>>,
    uri_only: bool,
}
impl Flow {
    fn cached_bytes(&self, side: usize) -> Option<&Arc<[u8]>> {
        let slot = if side == 0 {
            &self.raw.0.request
        } else {
            &self.raw.0.response
        };
        match slot.get() {
            Some(Ok(Some(bytes))) => Some(bytes),
            _ => None,
        }
    }
    fn run(&mut self) -> Result<()> {
        let invocation = Invocation {
            original: self.original.clone(),
            states: self.states,
            raw: self.raw.clone(),
            output: Arc::new(Mutex::new((self.output.clone(), self.replacements.clone()))),
            uri_only: self.uri_only,
        };
        let script = self.evaluator.0.lock().expect("redaction evaluator lock");
        script.lua.gc_restart();
        script
            .lua
            .app_data_ref::<InstructionCounter>()
            .expect("instruction counter")
            .reset();
        let _ = script.warnings.take();
        let environment = script
            .environment_factory
            .call::<Table>(script.lua.globals())?;
        let context = script.lua.create_table()?;
        context.set("api", self.api.as_str())?;
        for (name, index) in [("req", 0), ("resp", 1)] {
            let state = script.lua.create_table()?;
            state.set("header", self.states[index * 2].name())?;
            state.set("body", self.states[index * 2 + 1].name())?;
            context.set(name, state)?;
        }
        environment.set("context", context)?;
        environment.set(
            "req",
            (self.states[0] != PartState::Unloaded).then(|| RedactionMessage {
                invocation: invocation.clone(),
                side: 0,
            }),
        )?;
        environment.set(
            "resp",
            self.original.response.as_ref().map(|_| RedactionMessage {
                invocation: invocation.clone(),
                side: 1,
            }),
        )?;
        if !script.function.set_environment(environment)? {
            bail!("redaction script has no environment");
        }
        script
            .lua
            .app_data_ref::<InstructionCounter>()
            .expect("instruction counter")
            .reset();
        let result = script.function.call::<()>(());
        // No script-controlled warning/error payload is published through system logs.
        let _ = script.warnings.take();
        result?;
        if self.read_failed.load(Ordering::Relaxed) {
            bail!("redaction body read failed");
        }
        let output = invocation.output.lock().expect("redaction output lock");
        self.output = output.0.clone();
        self.replacements = output.1.clone();
        for state in &mut self.states {
            if *state == PartState::Loaded {
                *state = PartState::Redacted;
            }
        }
        Ok(())
    }
}
#[derive(Clone)]
struct RedactionMessage {
    invocation: Invocation,
    side: usize,
}
impl UserData for RedactionMessage {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("uri", |_, this| {
            Ok((this.side == 0).then(|| UriView::from(&this.invocation.original.request.uri)))
        });
        fields.add_field_method_set("uri", |_, this, uri: String| {
            if this.side == 0 && this.invocation.states[0] == PartState::Loaded {
                uri.parse::<hyper::Uri>().map_err(LuaError::external)?;
                this.invocation
                    .output
                    .lock()
                    .expect("redaction output lock")
                    .0
                    .request
                    .uri = uri;
            }
            Ok(())
        });
        fields.add_field_method_get("method", |_, this| {
            Ok((this.side == 0 && !this.invocation.uri_only)
                .then(|| this.invocation.original.request.method.clone()))
        });
        fields.add_field_method_get("status", |_, this| {
            Ok(if this.side == 1 {
                this.invocation.original.response.as_ref().map(|r| r.status)
            } else {
                None
            })
        });
        fields.add_field_method_get("version", |_, this| {
            Ok(if this.invocation.uri_only {
                None
            } else if this.side == 0 {
                Some(this.invocation.original.request.version.clone())
            } else {
                this.invocation
                    .original
                    .response
                    .as_ref()
                    .map(|r| r.version.clone())
            })
        });
        fields.add_field_method_get("headers", |_, this| {
            Ok((!this.invocation.uri_only
                && this.invocation.states[this.side * 2] != PartState::Unloaded)
                .then(|| RedactionHeaders(this.clone())))
        });
        fields.add_field_method_get("body", |_, this| {
            Ok(
                (this.invocation.states[this.side * 2 + 1] != PartState::Unloaded)
                    .then(|| RedactionBody(this.clone())),
            )
        });
    }
}
impl RedactionMessage {
    fn headers(&self) -> &HeaderValues {
        if self.side == 0 {
            &self.invocation.original.request.headers
        } else {
            &self
                .invocation
                .original
                .response
                .as_ref()
                .expect("response exists")
                .headers
        }
    }
}
#[derive(Clone)]
struct RedactionHeaders(RedactionMessage);
impl RedactionHeaders {
    fn write(&self, op: impl FnOnce(&mut HeaderValues)) {
        if self.0.invocation.states[self.0.side * 2] != PartState::Loaded {
            return;
        }
        let mut output = self
            .0
            .invocation
            .output
            .lock()
            .expect("redaction output lock");
        op(if self.0.side == 0 {
            &mut output.0.request.headers
        } else {
            &mut output.0.response.as_mut().expect("response exists").headers
        });
    }
}
impl UserData for RedactionHeaders {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("get", |_, this, name: String| {
            Ok(header_values(this.0.headers(), &name)
                .and_then(|v| v.first())
                .cloned())
        });
        methods.add_method("get_all", |_, this, name: String| {
            Ok(header_values(this.0.headers(), &name)
                .cloned()
                .unwrap_or_default())
        });
        methods.add_method("all", |lua, this, ()| {
            let table = lua.create_table()?;
            for (name, values) in this.0.headers() {
                table.set(name.as_str(), values.clone())?;
            }
            Ok(table)
        });
        methods.add_method("set", |_, this, (name, value): (String, String)| {
            if this.0.invocation.states[this.0.side * 2] != PartState::Loaded {
                return Ok(());
            }
            validate_header_input(&name, &value)?;
            this.write(|h| {
                remove_header(h, &name);
                h.insert(name.to_lowercase(), vec![value]);
            });
            Ok(())
        });
        methods.add_method("append", |_, this, (name, value): (String, String)| {
            if this.0.invocation.states[this.0.side * 2] != PartState::Loaded {
                return Ok(());
            }
            validate_header_input(&name, &value)?;
            this.write(|h| {
                let key = existing_header_name(h, &name).unwrap_or_else(|| name.to_lowercase());
                h.entry(key).or_default().push(value);
            });
            Ok(())
        });
        methods.add_method("remove", |_, this, name: String| {
            this.write(|h| {
                remove_header(h, &name);
            });
            Ok(())
        });
    }
}
#[derive(Clone)]
struct RedactionBody(RedactionMessage);
impl UserData for RedactionBody {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("as_string", |lua, this, ()| {
            let side = if this.0.side == 0 {
                BodySide::Request
            } else {
                BodySide::Response
            };
            let Some(bytes) = this.0.invocation.raw.text_bytes(side)? else {
                return Ok(Value::Nil);
            };
            std::str::from_utf8(&bytes).map_err(LuaError::external)?;
            Ok(Value::String(lua.create_string(bytes.as_ref())?))
        });
        methods.add_method("as_json", |lua, this, ()| {
            let side = if this.0.side == 0 {
                BodySide::Request
            } else {
                BodySide::Response
            };
            let Some(bytes) = this.0.invocation.raw.text_bytes(side)? else {
                return Ok(Value::Nil);
            };
            let _: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(LuaError::external)?;
            let json: Table = lua.globals().get("json")?;
            json.get::<Function>("decode")?
                .call::<Value>(lua.create_string(bytes.as_ref())?)
        });
        methods.add_method("replace_with_string", |_, this, content: String| {
            if this.0.invocation.states[this.0.side * 2 + 1] == PartState::Loaded {
                this.0
                    .invocation
                    .output
                    .lock()
                    .expect("redaction output lock")
                    .1[this.0.side] = Some(content);
            }
            Ok(())
        });
    }
}

fn strict_source(mut source: BodySource) -> Result<BodySource> {
    if !is_textual(source.content_type.as_deref().unwrap_or_default()) {
        return Ok(source);
    }
    let mut reader: Box<dyn Read> = match source.data {
        BodySourceData::File(ref path) => Box::new(File::open(path)?.take(source.stored_size)),
        BodySourceData::Bytes(ref bytes) => Box::new(std::io::Cursor::new(bytes.clone())),
    };
    for encoding in source.content_encodings.iter().rev() {
        reader = match encoding.as_str() {
            "gzip" => Box::new(flate2::read::GzDecoder::new(reader)),
            "deflate" => Box::new(flate2::read::ZlibDecoder::new(reader)),
            "br" => Box::new(brotli::Decompressor::new(reader, 4096)),
            "zstd" => Box::new(zstd::stream::read::Decoder::new(reader)?),
            _ => bail!("unsupported body encoding"),
        };
    }
    let mut bytes = Vec::new();
    reader.take(LUA_BODY_LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LUA_BODY_LIMIT {
        bail!("body exceeds Lua limit");
    }
    source.data = BodySourceData::Bytes(bytes);
    source.content_encodings.clear();
    Ok(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn summary() -> CaptureSummary {
        CaptureSummary {
            id: 1,
            session_id: 1,
            source: "client".into(),
            request: RequestData {
                method: "GET".into(),
                uri: "https://example.com/secret".into(),
                version: "HTTP/1.1".into(),
                headers: BTreeMap::from([
                    ("authorization".into(), vec!["secret".into()]),
                    ("content-type".into(), vec!["text/plain".into()]),
                ]),
                tags: BTreeMap::new(),
            },
            response: None,
            outcome: CaptureOutcome::Success,
            stage: "done".into(),
            error: None,
            created_at: 1,
            updated_at: 1,
        }
    }
    #[test]
    fn websocket_policy_is_opt_in_directional_and_fail_closed() {
        let metadata =
            serde_json::json!({"id": 1, "type": "text", "started_at": 1, "completed_at": 2});
        let old = RedactionEvaluator::new("", "legacy").unwrap();
        assert!(
            old.websocket_message(
                "/test",
                "wss://example.com",
                BodySide::Request,
                &metadata,
                b"secret".to_vec()
            )
            .is_err()
        );
        let evaluator = RedactionEvaluator::new(
            r#"-- @proxycrab websocket-messages
            assert(context.kind == 'websocket_message')
            assert(context.id == 1)
            assert(req ~= nil and resp == nil)
            assert(req.version == 'WebSocket/13')
            assert(req.headers == nil and req.method == nil)
            assert(req.body:as_string() == 'secret')
            req.body:replace_with_string('safe')
        "#,
            "ws",
        )
        .unwrap();
        assert_eq!(
            evaluator
                .websocket_message(
                    "/test",
                    "wss://example.com",
                    BodySide::Request,
                    &metadata,
                    b"secret".to_vec()
                )
                .unwrap()
                .unwrap(),
            b"safe"
        );
        assert!(
            evaluator
                .websocket_message(
                    "/test",
                    "wss://example.com",
                    BodySide::Response,
                    &metadata,
                    b"secret".to_vec()
                )
                .is_err()
        );
    }

    #[test]
    fn websocket_policy_hiding_empty_binary_and_readonly_metadata() {
        let metadata = serde_json::json!({"id": 3});
        for (body, expected) in [
            ("resp.body:hide()", None),
            ("resp.body:replace_with_string('')", Some(Vec::new())),
            (
                "resp.body:replace_with_bytes(resp.body:as_bytes())",
                Some(vec![0, 255]),
            ),
        ] {
            let evaluator = RedactionEvaluator::new(
                &format!(
                    "-- @proxycrab websocket-messages\nassert(req == nil and resp ~= nil); {body}"
                ),
                "ws",
            )
            .unwrap();
            assert_eq!(
                evaluator
                    .websocket_message(
                        "/test",
                        "wss://example.com",
                        BodySide::Response,
                        &metadata,
                        vec![0, 255]
                    )
                    .unwrap(),
                expected
            );
        }
        for mutation in [
            "rawset(context, 'id', 4)",
            "context.id = 4",
            "resp.version = 'HTTP/1.1'",
            "resp.uri = 'wss://other'",
            "error('fail')",
        ] {
            let evaluator = RedactionEvaluator::new(
                &format!("-- @proxycrab websocket-messages\n{mutation}"),
                "ws",
            )
            .unwrap();
            assert!(
                evaluator
                    .websocket_message(
                        "/test",
                        "wss://example.com",
                        BodySide::Response,
                        &metadata,
                        vec![]
                    )
                    .is_err()
            );
        }
    }

    #[test]
    fn redaction_reuses_original_body_between_stages_and_does_not_emit_later_appends() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("body");
        std::fs::write(&path, b"first").unwrap();
        let mut original = summary();
        original.response = Some(ResponseData {
            status: 200,
            version: "HTTP/1.1".into(),
            headers: Default::default(),
        });
        let body_path = path.clone();
        let evaluator = RedactionEvaluator::new(
            "if req.body then assert(req.body:as_string() == 'first') end",
            "test",
        )
        .unwrap();
        let view = RedactedCapture::new(evaluator, "/test", original, move |side| {
            Ok(if side == BodySide::Request {
                Some(BodySource {
                    data: BodySourceData::File(body_path.clone()),
                    path: None,
                    stored_size: 5,
                    content_type: Some("text/plain".into()),
                    content_encodings: vec![],
                })
            } else {
                None
            })
        })
        .unwrap();
        view.body_source(BodySide::Request).unwrap();
        std::fs::write(path, b"first and later secret").unwrap();
        view.body_source(BodySide::Response).unwrap();
        assert!(
            matches!(view.body_source(BodySide::Request).unwrap().unwrap().data, BodySourceData::Bytes(b) if b == b"first")
        );
    }
    #[test]
    fn redaction_missing_request_stays_unloaded_in_response_snapshot() {
        let mut original = summary();
        original.response = Some(ResponseData {
            status: 200,
            version: "HTTP/1.1".into(),
            headers: Default::default(),
        });
        let evaluator = RedactionEvaluator::new("assert(req == nil and context.req.header == 'unloaded' and context.req.body == 'unloaded'); assert(resp ~= nil)", "test").unwrap();
        let view =
            RedactedCapture::snapshot(evaluator, "/test", original, false, |_| Ok(None)).unwrap();
        assert!(view.body_source(BodySide::Request).unwrap().is_none());
        view.body_source(BodySide::Response).unwrap();
    }
    #[test]
    fn redaction_invalid_compression_is_fatal_even_when_lua_catches_getter_error() {
        let evaluator = RedactionEvaluator::new(
            "if req.body then pcall(function() req.body:as_string() end) end",
            "test",
        )
        .unwrap();
        let view = RedactedCapture::new(evaluator, "/test", summary(), |_| {
            Ok(Some(BodySource {
                data: BodySourceData::Bytes(b"invalid".to_vec()),
                path: None,
                stored_size: 7,
                content_type: Some("text/plain".into()),
                content_encodings: vec!["gzip".into()],
            }))
        })
        .unwrap();
        assert!(view.body_source(BodySide::Request).is_err());
        assert!(view.body_source(BodySide::Request).is_err());
        assert!(view.ensure_success().is_err());
    }

    #[test]
    fn redaction_stages_read_originals_and_commit_only_loaded_parts() {
        let script = RedactionEvaluator::new(
            r#"
            assert(context.api == '/test')
            assert(req.headers:get('authorization') == 'secret')
            req.headers:set('authorization', 'hidden')
            assert(req.headers:get('authorization') == 'secret')
            if context.req.body == 'loaded' then
                assert(context.req.header == 'redacted')
                req.headers:set('authorization', 'too-late')
                assert(req.body:as_string() == 'original')
                req.body:replace_with_string('masked')
                assert(req.body:as_string() == 'original')
            end
        "#,
            "test",
        )
        .unwrap();
        let loads = Arc::new(AtomicU32::new(0));
        let count = loads.clone();
        let original = summary();
        let view = RedactedCapture::new(script, "/test", original.clone(), move |_| {
            count.fetch_add(1, Ordering::Relaxed);
            Ok(Some(BodySource {
                data: BodySourceData::Bytes(b"original".to_vec()),
                path: None,
                stored_size: 8,
                content_type: Some("text/plain".into()),
                content_encodings: vec![],
            }))
        })
        .unwrap();
        assert_eq!(loads.load(Ordering::Relaxed), 0);
        assert_eq!(view.summary().request.headers["authorization"], ["hidden"]);
        for _ in 0..2 {
            let source = view.body_source(BodySide::Request).unwrap().unwrap();
            assert!(matches!(source.data, BodySourceData::Bytes(b) if b == b"masked"));
        }
        assert_eq!(loads.load(Ordering::Relaxed), 1);
        assert_eq!(view.summary().request.headers["authorization"], ["hidden"]);
        assert_eq!(original.request.headers["authorization"], ["secret"]);
    }
    #[test]
    fn redaction_noop_completes_header_and_failed_body_does_not_commit() {
        let view = RedactedCapture::new(RedactionEvaluator::new("if context.req.body == 'loaded' then req.headers:remove('authorization'); req.body:replace_with_string('bad'); error('failure') end", "test").unwrap(), "/test", summary(), |_| Ok(None)).unwrap();
        assert_eq!(view.0.lock().unwrap().states[0], PartState::Redacted);
        assert!(view.body_source(BodySide::Request).is_err());
        assert_eq!(view.summary().request.headers["authorization"], ["secret"]);
        assert!(view.0.lock().unwrap().replacements[0].is_none());
    }
    #[test]
    fn redaction_uri_filter_exposes_only_uri() {
        let evaluator = RedactionEvaluator::new("assert(req.headers == nil and req.body == nil and req.method == nil and resp == nil); req.uri = 'https://example.com/masked'", "test").unwrap();
        let view =
            RedactedCapture::uri_only(evaluator, "/api/logs/ids", "https://example.com/secret")
                .unwrap();
        assert_eq!(view.summary().request.uri, "https://example.com/masked");
    }
}
