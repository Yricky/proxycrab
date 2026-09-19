use std::{
    collections::BTreeMap,
    fs::File,
    io::Read,
    net::SocketAddr,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU32, Ordering},
    },
};

use anyhow::{Result, bail};
use mlua::{
    Error as LuaError, Function, HookTriggers, Lua, Table, UserData, UserDataFields,
    UserDataMethods, Value, VmState,
};

use crate::{
    asset::{Asset, AssetStore},
    breakpoint::{BreakpointContext, BreakpointRegistry},
    model::{
        BodySourceType, CaptureOutcome, CaptureSummary, HeaderValues, Modification, RequestData,
        RequestInterceptorSnapshot, RequestTags, ResponseData, ResponseInterceptorSnapshot,
        ScriptKind,
    },
    proxy::body::{DeferredBodyRead, DeferredBodyReader},
    storage::{BodySide, BodySource, BodySourceData},
};

mod codec;

pub const INSTRUCTION_LIMIT: u32 = 100_000;
pub const MEMORY_LIMIT: usize = 16 * 1024 * 1024;
const ANONYMOUS_SCRIPT_NAME: &str = "<anonymous>";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyReplacement {
    String(String),
    Asset(Asset),
}

impl BodyReplacement {
    pub fn source_type(&self) -> BodySourceType {
        match self {
            Self::String(content) => BodySourceType::String {
                content: content.clone(),
            },
            Self::Asset(asset) => BodySourceType::Asset {
                asset_id: asset.metadata.id.clone(),
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScriptEffects {
    pub method: Option<String>,
    pub uri: Option<String>,
    pub status: Option<u16>,
    pub headers: HeaderValues,
    pub body: Option<BodyReplacement>,
    pub tags: RequestTags,
    pub modifications: Vec<Modification>,
}

#[derive(Clone)]
pub(crate) struct BreakpointHook {
    pub registry: Arc<BreakpointRegistry>,
    pub context: BreakpointContext,
}

#[derive(Clone, Copy)]
pub(crate) struct ResponseScriptContext<'a> {
    pub request: &'a RequestData,
    pub response: &'a ResponseData,
}

pub fn validate_script(kind: ScriptKind, source: &str) -> Result<()> {
    let (lua, _) = safe_lua()?;
    // `@` 前缀让 Lua 将 chunk 名视为文件名，错误消息直接显示 `xxx.lua:行号`
    // 而非内部的 `[string "xxx.lua"]` 包装格式。
    let name = match kind {
        ScriptKind::Column => "@column.lua",
        ScriptKind::Filter => "@filter.lua",
        ScriptKind::Routing => "@routing.lua",
        ScriptKind::RequestInterceptor => "@request-interceptor.lua",
        ScriptKind::ResponseInterceptor => "@response-interceptor.lua",
    };
    lua.load(source)
        .set_name(name)
        .into_function()
        .map(|_| ())
        .map_err(|error| crate::error::Error::InvalidScript(error.to_string()).into())
}

pub fn evaluate_routing(
    source: &str,
    phase: &str,
    request: &RequestData,
    authority: &str,
    source_address: SocketAddr,
    script_name: &str,
) -> Result<Option<bool>> {
    let (lua, warnings) = safe_lua()?;
    lua.globals().set("phase", phase)?;
    lua.globals().set(
        "req",
        RoutingRequestView {
            request: request.clone(),
            authority: authority.to_string(),
        },
    )?;
    lua.globals()
        .set("source", SourceView::from(source_address))?;
    let result = lua.load(source).eval::<Value>();
    log_json_warnings(&warnings, ScriptKind::Routing, script_name, None);
    match result? {
        Value::Nil => Ok(None),
        Value::Boolean(value) => Ok(Some(value)),
        _ => bail!("routing script must return a boolean or nil"),
    }
}

pub fn evaluate_filter(source: &str, argument: &str, entry: &CaptureSummary) -> Result<bool> {
    evaluate_filter_named(source, argument, entry, ANONYMOUS_SCRIPT_NAME)
}

pub fn evaluate_filter_named(
    source: &str,
    argument: &str,
    entry: &CaptureSummary,
    script_name: &str,
) -> Result<bool> {
    FilterEvaluator::new(source, script_name)?.evaluate(argument, entry)
}

pub struct FilterEvaluator {
    script: ReusableScript,
}

impl FilterEvaluator {
    pub fn new(source: &str, script_name: &str) -> Result<Self> {
        Ok(Self {
            script: ReusableScript::new(source, ScriptKind::Filter, script_name)?,
        })
    }

    pub fn evaluate(&self, argument: &str, entry: &CaptureSummary) -> Result<bool> {
        self.evaluate_with_bodies(argument, entry, CaptureBodyAccess::unavailable())
    }

    pub fn evaluate_with_bodies(
        &self,
        argument: &str,
        entry: &CaptureSummary,
        bodies: CaptureBodyAccess,
    ) -> Result<bool> {
        self.script.prepare(entry, bodies)?;
        let result = self.script.function.call::<Value>(argument);
        self.script.finish(entry.id);
        match result? {
            Value::Boolean(value) => Ok(value),
            _ => bail!("filter script must return a boolean"),
        }
    }
}

pub struct ColumnEvaluator {
    script: ReusableScript,
}

impl ColumnEvaluator {
    pub fn new(source: &str, script_name: &str) -> Result<Self> {
        Ok(Self {
            script: ReusableScript::new(source, ScriptKind::Column, script_name)?,
        })
    }

    pub fn evaluate(&self, entry: &CaptureSummary) -> Result<String> {
        self.evaluate_with_bodies(entry, CaptureBodyAccess::unavailable())
    }

    pub fn evaluate_with_bodies(
        &self,
        entry: &CaptureSummary,
        bodies: CaptureBodyAccess,
    ) -> Result<String> {
        self.script.prepare(entry, bodies)?;
        let result = self.script.function.call::<Value>(());
        self.script.finish(entry.id);
        match result? {
            Value::Nil => Ok(String::new()),
            Value::Boolean(value) => Ok(value.to_string()),
            Value::Integer(value) => Ok(value.to_string()),
            Value::Number(value) => Ok(value.to_string()),
            Value::String(value) => Ok(value.to_str()?.to_string()),
            _ => bail!("column script must return nil or a scalar value"),
        }
    }
}

struct ReusableScript {
    lua: Lua,
    function: Function,
    environment_factory: Function,
    warnings: codec::JsonWarningState,
    kind: ScriptKind,
    name: String,
}

impl ReusableScript {
    fn new(source: &str, kind: ScriptKind, name: &str) -> Result<Self> {
        let (lua, warnings) = safe_lua()?;
        let function = lua.load(source).into_function()?;
        let environment_factory = lua
            .load(
                r#"return function(base)
                     local environment = {}
                     for key, value in next, base do
                       if key ~= "_G" then
                         if type(value) == "table" and key ~= "base64" and key ~= "json" then
                           local copy = {}
                           for table_key, table_value in next, value do
                             copy[table_key] = table_value
                           end
                           local metatable = getmetatable(value)
                           if type(metatable) == "table" then
                             setmetatable(copy, metatable)
                           end
                           value = copy
                         end
                         environment[key] = value
                       end
                     end
                     environment._G = environment
                     local base_load = environment.load
                     environment.load = function(chunk, chunk_name, mode, chunk_environment)
                       if chunk_environment == nil then
                         chunk_environment = environment
                       end
                       return base_load(chunk, chunk_name, mode, chunk_environment)
                     end
                     return environment
                   end"#,
            )
            .eval()?;
        Ok(Self {
            lua,
            function,
            environment_factory,
            warnings,
            kind,
            name: name.to_string(),
        })
    }

    fn prepare(&self, entry: &CaptureSummary, bodies: CaptureBodyAccess) -> Result<()> {
        self.lua.gc_restart();
        let _ = self.warnings.take();
        let environment = self.environment_factory.call::<Table>(self.lua.globals())?;
        let bodies = if entry.outcome == CaptureOutcome::InProgress {
            CaptureBodyAccess::unavailable()
        } else {
            bodies
        };
        environment.set(
            "entry",
            EntryView {
                entry: entry.clone(),
                bodies,
            },
        )?;
        if !self.function.set_environment(environment)? {
            bail!("script function has no Lua environment");
        }
        self.lua
            .app_data_ref::<InstructionCounter>()
            .expect("safe Lua always has an instruction counter")
            .reset();
        Ok(())
    }

    fn finish(&self, capture_id: u64) {
        log_json_warnings(&self.warnings, self.kind, &self.name, Some(capture_id));
    }
}

#[derive(Clone, Default)]
struct InstructionCounter(Arc<AtomicU32>);

impl InstructionCounter {
    fn reset(&self) {
        self.0.store(0, Ordering::Relaxed);
    }

    fn add(&self, count: u32) -> u32 {
        self.0.fetch_add(count, Ordering::Relaxed) + count
    }
}

pub fn evaluate_column(source: &str, entry: &CaptureSummary) -> Result<String> {
    evaluate_column_named(source, entry, ANONYMOUS_SCRIPT_NAME)
}

pub fn evaluate_column_named(
    source: &str,
    entry: &CaptureSummary,
    script_name: &str,
) -> Result<String> {
    ColumnEvaluator::new(source, script_name)?.evaluate(entry)
}

pub fn execute_request(source: &str, request: &RequestData) -> Result<ScriptEffects> {
    let (effects, error) = execute_request_lenient(source, request)?;
    if let Some(error) = error {
        bail!(error);
    }
    Ok(effects)
}

pub fn execute_request_lenient(
    source: &str,
    request: &RequestData,
) -> Result<(ScriptEffects, Option<String>)> {
    execute_request_lenient_named(source, request, ANONYMOUS_SCRIPT_NAME, None)
}

pub fn execute_request_lenient_named(
    source: &str,
    request: &RequestData,
    script_name: &str,
    capture_log_id: Option<u64>,
) -> Result<(ScriptEffects, Option<String>)> {
    let state = SharedInterceptorState::new_request(
        request.method.clone(),
        request.uri.clone(),
        request.headers.clone(),
        request.tags.clone(),
    );
    let journal = ModificationJournal::for_request(request, None);
    execute_request_with_state(
        source,
        request,
        state,
        journal,
        script_name,
        capture_log_id,
        None,
    )
}

pub(crate) fn execute_request_with_state(
    source: &str,
    request: &RequestData,
    state: SharedInterceptorState,
    journal: ModificationJournal,
    script_name: &str,
    capture_log_id: Option<u64>,
    breakpoint: Option<BreakpointHook>,
) -> Result<(ScriptEffects, Option<String>)> {
    let (lua, warnings) = safe_lua()?;
    lua.globals().set(
        "req",
        RequestView {
            request: request.clone(),
            state: state.clone(),
            journal: journal.clone(),
        },
    )?;
    install_breakpoint(&lua, breakpoint)?;
    install_get_asset(&lua, state.clone())?;
    let error = lua.load(source).exec().err().map(|error| error.to_string());
    log_json_warnings(
        &warnings,
        ScriptKind::RequestInterceptor,
        script_name,
        capture_log_id,
    );
    Ok((state.effects(&journal), error))
}

pub fn execute_response(source: &str, response: &ResponseData) -> Result<ScriptEffects> {
    let (effects, error) = execute_response_lenient(source, response)?;
    if let Some(error) = error {
        bail!(error);
    }
    Ok(effects)
}

pub fn execute_response_lenient(
    source: &str,
    response: &ResponseData,
) -> Result<(ScriptEffects, Option<String>)> {
    execute_response_lenient_named(source, response, ANONYMOUS_SCRIPT_NAME, None)
}

pub fn execute_response_lenient_named(
    source: &str,
    response: &ResponseData,
    script_name: &str,
    capture_log_id: Option<u64>,
) -> Result<(ScriptEffects, Option<String>)> {
    execute_response_lenient_named_with_tags(
        source,
        response,
        &RequestTags::new(),
        script_name,
        capture_log_id,
    )
}

pub fn execute_response_lenient_named_with_tags(
    source: &str,
    response: &ResponseData,
    request_tags: &RequestTags,
    script_name: &str,
    capture_log_id: Option<u64>,
) -> Result<(ScriptEffects, Option<String>)> {
    let request = RequestData {
        method: String::new(),
        uri: String::new(),
        version: String::new(),
        headers: HeaderValues::new(),
        tags: request_tags.clone(),
    };
    let state = SharedInterceptorState::new_response(
        response.status,
        response.headers.clone(),
        request_tags.clone(),
    );
    let journal = ModificationJournal::for_response(response, None);
    execute_response_with_state(
        source,
        ResponseScriptContext {
            request: &request,
            response,
        },
        state,
        journal,
        script_name,
        capture_log_id,
        None,
    )
}

pub(crate) fn execute_response_with_state(
    source: &str,
    context: ResponseScriptContext<'_>,
    state: SharedInterceptorState,
    journal: ModificationJournal,
    script_name: &str,
    capture_log_id: Option<u64>,
    breakpoint: Option<BreakpointHook>,
) -> Result<(ScriptEffects, Option<String>)> {
    let (lua, warnings) = safe_lua()?;
    lua.globals().set(
        "resp",
        ResponseView {
            response: context.response.clone(),
            state: state.clone(),
            journal: journal.clone(),
        },
    )?;
    install_breakpoint(&lua, breakpoint)?;
    install_get_asset(&lua, state.clone())?;
    lua.globals().set(
        "req",
        ResponseRequestView {
            request: context.request.clone(),
            state: state.clone(),
            journal: journal.clone(),
        },
    )?;
    let error = lua.load(source).exec().err().map(|error| error.to_string());
    log_json_warnings(
        &warnings,
        ScriptKind::ResponseInterceptor,
        script_name,
        capture_log_id,
    );
    Ok((state.effects(&journal), error))
}

fn install_breakpoint(lua: &Lua, hook: Option<BreakpointHook>) -> Result<()> {
    let Some(hook) = hook else {
        return Ok(());
    };
    let breakpoint = lua.create_function(move |_, timeout_ms: u64| {
        hook.registry
            .wait(hook.context.clone(), timeout_ms)
            .map_err(LuaError::external)
    })?;
    lua.globals().set("breakpoint", breakpoint)?;
    Ok(())
}

fn install_get_asset(lua: &Lua, state: SharedInterceptorState) -> Result<()> {
    let get_asset = lua.create_function(move |_, id: String| {
        let assets = state
            .assets
            .lock()
            .expect("Lua asset store state lock poisoned")
            .clone()
            .ok_or_else(|| LuaError::runtime("asset store is unavailable"))?;
        match assets.get(&id) {
            Ok(Some(asset)) => Ok(Some(LuaAsset(asset))),
            Ok(None) | Err(crate::asset::AssetError::InvalidId(_)) => Ok(None),
            Err(error) => Err(LuaError::external(error)),
        }
    })?;
    lua.globals().set("get_asset", get_asset)?;
    Ok(())
}

fn safe_lua() -> Result<(Lua, codec::JsonWarningState)> {
    let lua = Lua::new();
    lua.set_memory_limit(MEMORY_LIMIT)?;
    for library in [
        "io", "os", "package", "debug", "dofile", "loadfile", "require",
    ] {
        lua.globals().set(library, Value::Nil)?;
    }
    let executed = InstructionCounter::default();
    lua.set_app_data(executed.clone());
    lua.set_hook(
        HookTriggers::new().every_nth_instruction(1000),
        move |_lua, _debug| {
            if executed.add(1000) > INSTRUCTION_LIMIT {
                Err(LuaError::runtime("Lua instruction limit exceeded"))
            } else {
                Ok(VmState::Continue)
            }
        },
    )?;
    let warnings = codec::install(&lua)?;
    Ok((lua, warnings))
}

fn log_json_warnings(
    warnings: &codec::JsonWarningState,
    kind: ScriptKind,
    script_name: &str,
    capture_log_id: Option<u64>,
) {
    let counts = warnings.take();
    if counts.is_empty() {
        return;
    }
    let kind = match kind {
        ScriptKind::Column => "column script",
        ScriptKind::Filter => "filter script",
        ScriptKind::Routing => "routing script",
        ScriptKind::RequestInterceptor => "request interceptor",
        ScriptKind::ResponseInterceptor => "response interceptor",
    };
    let capture = capture_log_id
        .map(|id| format!("capture_log_id={id}, "))
        .unwrap_or_default();
    tracing::warn!(
        "Lua JSON decode warning in {kind} {script_name:?}: \
         {capture}lossy_numbers={}, duplicate_keys={}",
        counts.lossy_numbers,
        counts.duplicate_keys,
    );
}

type BodySourceResolver = dyn Fn(BodySide) -> Result<Option<BodySource>> + Send + Sync + 'static;
type CachedBody = Result<Option<Arc<[u8]>>, String>;

#[derive(Clone)]
pub struct CaptureBodyAccess(Arc<CaptureBodyAccessInner>);

struct CaptureBodyAccessInner {
    resolver: Option<Arc<BodySourceResolver>>,
    request: OnceLock<CachedBody>,
    response: OnceLock<CachedBody>,
}

impl CaptureBodyAccess {
    pub fn new<F>(resolver: F) -> Self
    where
        F: Fn(BodySide) -> Result<Option<BodySource>> + Send + Sync + 'static,
    {
        Self(Arc::new(CaptureBodyAccessInner {
            resolver: Some(Arc::new(resolver)),
            request: OnceLock::new(),
            response: OnceLock::new(),
        }))
    }

    pub fn unavailable() -> Self {
        Self(Arc::new(CaptureBodyAccessInner {
            resolver: None,
            request: OnceLock::new(),
            response: OnceLock::new(),
        }))
    }

    fn text_bytes(&self, side: BodySide) -> mlua::Result<Option<Arc<[u8]>>> {
        let cache = match side {
            BodySide::Request => &self.0.request,
            BodySide::Response => &self.0.response,
        };
        cache
            .get_or_init(|| {
                let Some(resolver) = &self.0.resolver else {
                    return Ok(None);
                };
                let Some(source) = resolver(side).map_err(|error| error.to_string())? else {
                    return Ok(None);
                };
                read_text_body_source(source)
                    .map(|bytes| bytes.map(Arc::from))
                    .map_err(|error| error.to_string())
            })
            .clone()
            .map_err(LuaError::runtime)
    }
}

#[derive(Clone)]
struct EntryView {
    entry: CaptureSummary,
    bodies: CaptureBodyAccess,
}

impl UserData for EntryView {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("id", |_, this| Ok(this.entry.id));
        fields.add_field_method_get("req", |_, this| {
            Ok(ReadRequest {
                request: this.entry.request.clone(),
                body: ReadBody {
                    access: this.bodies.clone(),
                    side: BodySide::Request,
                },
            })
        });
        fields.add_field_method_get("resp", |_, this| {
            Ok(this.entry.response.clone().map(|response| ReadResponse {
                response,
                body: ReadBody {
                    access: this.bodies.clone(),
                    side: BodySide::Response,
                },
            }))
        });
    }
}

#[derive(Clone)]
struct ReadRequest {
    request: RequestData,
    body: ReadBody,
}

impl UserData for ReadRequest {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("method", |_, this| Ok(this.request.method.clone()));
        fields.add_field_method_get("version", |_, this| Ok(this.request.version.clone()));
        fields.add_field_method_get("uri", |_, this| Ok(UriView::from(&this.request.uri)));
        fields.add_field_method_get("headers", |_, this| {
            Ok(ReadHeaders(this.request.headers.clone()))
        });
        fields.add_field_method_get("body", |_, this| Ok(this.body.clone()));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("get_tag", |_, this, key: String| {
            Ok(this.request.tags.get(&key).cloned())
        });
    }
}

#[derive(Clone)]
struct ReadResponse {
    response: ResponseData,
    body: ReadBody,
}

impl UserData for ReadResponse {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("status", |_, this| Ok(this.response.status));
        fields.add_field_method_get("version", |_, this| Ok(this.response.version.clone()));
        fields.add_field_method_get("headers", |_, this| {
            Ok(ReadHeaders(this.response.headers.clone()))
        });
        fields.add_field_method_get("body", |_, this| Ok(this.body.clone()));
    }
}

#[derive(Clone)]
struct ReadBody {
    access: CaptureBodyAccess,
    side: BodySide,
}

impl UserData for ReadBody {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("as_string", |lua, this, ()| {
            let Some(bytes) = this.access.text_bytes(this.side)? else {
                return Ok(Value::Nil);
            };
            match std::str::from_utf8(&bytes) {
                Ok(_) => Ok(Value::String(lua.create_string(bytes.as_ref())?)),
                Err(_) => Ok(Value::Nil),
            }
        });
        methods.add_method("as_json", |lua, this, ()| {
            let Some(bytes) = this.access.text_bytes(this.side)? else {
                return Ok(Value::Nil);
            };
            let Ok(text) = lua.create_string(bytes.as_ref()) else {
                return Ok(Value::Nil);
            };
            let json: Table = lua.globals().get("json")?;
            let decode: Function = json.get("decode")?;
            Ok(decode.call::<Value>(text).unwrap_or(Value::Nil))
        });
    }
}

#[derive(Clone)]
struct UriView {
    raw: String,
    scheme: String,
    host: String,
    port: Option<u16>,
    path: String,
    query: String,
}

impl UriView {
    fn from(value: &str) -> Self {
        if let Ok(uri) = value.parse::<hyper::Uri>() {
            return Self {
                raw: value.to_string(),
                scheme: uri.scheme_str().map(str::to_string).unwrap_or_default(),
                host: uri.host().map(str::to_string).unwrap_or_default(),
                port: uri.port_u16(),
                path: uri.path().to_string(),
                query: uri.query().map(str::to_string).unwrap_or_default(),
            };
        }
        Self {
            raw: value.to_string(),
            scheme: String::new(),
            host: String::new(),
            port: None,
            path: value.to_string(),
            query: String::new(),
        }
    }
}

impl UserData for UriView {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("raw", |_, this| Ok(this.raw.clone()));
        fields.add_field_method_get("scheme", |_, this| Ok(this.scheme.clone()));
        fields.add_field_method_get("host", |_, this| Ok(this.host.clone()));
        fields.add_field_method_get("port", |_, this| Ok(this.port));
        fields.add_field_method_get("path", |_, this| Ok(this.path.clone()));
        fields.add_field_method_get("query", |_, this| Ok(this.query.clone()));
    }
}

#[derive(Clone)]
struct RoutingRequestView {
    request: RequestData,
    authority: String,
}

impl UserData for RoutingRequestView {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("method", |_, this| Ok(this.request.method.clone()));
        fields.add_field_method_get("version", |_, this| Ok(this.request.version.clone()));
        fields.add_field_method_get("authority", |_, this| Ok(this.authority.clone()));
        fields.add_field_method_get("uri", |_, this| Ok(UriView::from(&this.request.uri)));
    }
}

#[derive(Clone)]
struct SourceView {
    ip: String,
    port: u16,
    address: String,
}

impl From<SocketAddr> for SourceView {
    fn from(value: SocketAddr) -> Self {
        Self {
            ip: value.ip().to_string(),
            port: value.port(),
            address: value.to_string(),
        }
    }
}

impl UserData for SourceView {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("ip", |_, this| Ok(this.ip.clone()));
        fields.add_field_method_get("port", |_, this| Ok(this.port));
        fields.add_field_method_get("address", |_, this| Ok(this.address.clone()));
    }
}

#[derive(Clone)]
struct ReadHeaders(HeaderValues);

impl UserData for ReadHeaders {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("get", |_, this, name: String| {
            Ok(header_values(&this.0, &name).and_then(|values| values.first().cloned()))
        });
        methods.add_method("get_all", |_, this, name: String| {
            Ok(header_values(&this.0, &name).cloned().unwrap_or_default())
        });
        methods.add_method("all", |lua, this, ()| {
            let result = lua.create_table()?;
            for (name, values) in &this.0 {
                result.set(name.as_str(), values.clone())?;
            }
            Ok(result)
        });
    }
}

#[derive(Clone)]
pub(crate) struct SharedInterceptorState {
    method: Arc<Mutex<Option<String>>>,
    uri: Arc<Mutex<Option<String>>>,
    status: Arc<Mutex<Option<u16>>>,
    headers: Arc<Mutex<HeaderValues>>,
    body: Arc<Mutex<Option<BodyReplacement>>>,
    tags: Arc<Mutex<RequestTags>>,
    raw_body: Arc<Mutex<Option<DeferredBodyReader>>>,
    assets: Arc<Mutex<Option<AssetStore>>>,
    response_phase: bool,
}

impl SharedInterceptorState {
    pub(crate) fn new(headers: HeaderValues, tags: RequestTags) -> Self {
        Self {
            method: Arc::new(Mutex::new(None)),
            uri: Arc::new(Mutex::new(None)),
            status: Arc::new(Mutex::new(None)),
            headers: Arc::new(Mutex::new(headers)),
            body: Arc::new(Mutex::new(None)),
            tags: Arc::new(Mutex::new(tags)),
            raw_body: Arc::new(Mutex::new(None)),
            assets: Arc::new(Mutex::new(None)),
            response_phase: false,
        }
    }

    pub(crate) fn new_request(
        method: String,
        uri: String,
        headers: HeaderValues,
        tags: RequestTags,
    ) -> Self {
        let state = Self::new(headers, tags);
        *state.method.lock().expect("Lua method state lock poisoned") = Some(method);
        *state.uri.lock().expect("Lua URI state lock poisoned") = Some(uri);
        state
    }

    pub(crate) fn new_response(status: u16, headers: HeaderValues, tags: RequestTags) -> Self {
        let mut state = Self::new(headers, tags);
        state.response_phase = true;
        *state.status.lock().expect("Lua status state lock poisoned") = Some(status);
        state
    }

    pub(crate) fn effects(&self, journal: &ModificationJournal) -> ScriptEffects {
        ScriptEffects {
            method: self
                .method
                .lock()
                .expect("Lua method state lock poisoned")
                .clone(),
            uri: self
                .uri
                .lock()
                .expect("Lua URI state lock poisoned")
                .clone(),
            status: *self.status.lock().expect("Lua status state lock poisoned"),
            headers: self
                .headers
                .lock()
                .expect("Lua header state lock poisoned")
                .clone(),
            body: self
                .body
                .lock()
                .expect("Lua body state lock poisoned")
                .clone(),
            tags: self
                .tags
                .lock()
                .expect("Lua tag state lock poisoned")
                .clone(),
            modifications: journal.snapshot(),
        }
    }

    pub(crate) fn tags(&self) -> RequestTags {
        self.tags
            .lock()
            .expect("Lua tag state lock poisoned")
            .clone()
    }

    pub(crate) fn status(&self) -> Option<u16> {
        *self.status.lock().expect("Lua status state lock poisoned")
    }

    pub(crate) fn method(&self) -> Option<String> {
        self.method
            .lock()
            .expect("Lua method state lock poisoned")
            .clone()
    }

    pub(crate) fn uri(&self) -> Option<String> {
        self.uri
            .lock()
            .expect("Lua URI state lock poisoned")
            .clone()
    }

    pub(crate) fn headers(&self) -> HeaderValues {
        self.headers
            .lock()
            .expect("Lua header state lock poisoned")
            .clone()
    }

    pub(crate) fn body(&self) -> Option<BodyReplacement> {
        self.body
            .lock()
            .expect("Lua body state lock poisoned")
            .clone()
    }

    pub(crate) fn set_body(&self, body: Option<BodyReplacement>) {
        *self.body.lock().expect("Lua body state lock poisoned") = body;
    }

    pub(crate) fn set_raw_body(&self, reader: DeferredBodyReader) {
        *self
            .raw_body
            .lock()
            .expect("Lua raw body state lock poisoned") = Some(reader);
    }

    pub(crate) fn set_asset_store(&self, assets: AssetStore) {
        *self
            .assets
            .lock()
            .expect("Lua asset store state lock poisoned") = Some(assets);
    }
}

struct ModificationJournalState {
    initial_snapshot: Modification,
    modifications: Vec<Modification>,
    snapshot_recorded: bool,
}

#[derive(Clone)]
pub(crate) struct ModificationJournal(Arc<Mutex<ModificationJournalState>>);

impl ModificationJournal {
    pub(crate) fn for_request(request: &RequestData, body: Option<&BodyReplacement>) -> Self {
        Self::new(Modification::Snapshot {
            request: Some(RequestInterceptorSnapshot {
                method: request.method.clone(),
                uri: request.uri.clone(),
                version: request.version.clone(),
                headers: request.headers.clone(),
                body: body.map(BodyReplacement::source_type).unwrap_or_default(),
            }),
            response: None,
        })
    }

    pub(crate) fn for_response(response: &ResponseData, body: Option<&BodyReplacement>) -> Self {
        Self::new(Modification::Snapshot {
            request: None,
            response: Some(ResponseInterceptorSnapshot {
                status: response.status,
                version: response.version.clone(),
                headers: response.headers.clone(),
                body: body.map(BodyReplacement::source_type).unwrap_or_default(),
            }),
        })
    }

    pub(crate) fn for_current_request(
        request: &RequestData,
        state: &SharedInterceptorState,
    ) -> Self {
        Self::for_request(
            &RequestData {
                method: state
                    .method()
                    .expect("request Lua state always has a method"),
                uri: state.uri().expect("request Lua state always has a URI"),
                version: request.version.clone(),
                headers: state.headers(),
                tags: RequestTags::new(),
            },
            state.body().as_ref(),
        )
    }

    pub(crate) fn for_current_response(
        response: &ResponseData,
        state: &SharedInterceptorState,
    ) -> Self {
        Self::for_response(
            &ResponseData {
                status: state
                    .status()
                    .expect("response Lua state always has a status"),
                version: response.version.clone(),
                headers: state.headers(),
            },
            state.body().as_ref(),
        )
    }

    fn new(initial_snapshot: Modification) -> Self {
        Self(Arc::new(Mutex::new(ModificationJournalState {
            initial_snapshot,
            modifications: Vec::new(),
            snapshot_recorded: false,
        })))
    }

    fn push(&self, modification: Modification) {
        let records_snapshot = !matches!(modification, Modification::TagSet { .. });
        let mut state = self
            .0
            .lock()
            .expect("Lua modification journal lock poisoned");
        if records_snapshot && !state.snapshot_recorded {
            let snapshot = state.initial_snapshot.clone();
            state.modifications.insert(0, snapshot);
            state.snapshot_recorded = true;
        }
        state.modifications.push(modification);
    }

    pub(crate) fn snapshot(&self) -> Vec<Modification> {
        self.0
            .lock()
            .expect("Lua modification journal lock poisoned")
            .modifications
            .clone()
    }
}

#[derive(Clone)]
struct RequestView {
    request: RequestData,
    state: SharedInterceptorState,
    journal: ModificationJournal,
}

impl UserData for RequestView {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("method", |_, this| {
            Ok(this
                .state
                .method()
                .expect("request Lua state always has a method"))
        });
        fields.add_field_method_set("method", |_, this, method: String| {
            hyper::Method::from_bytes(method.as_bytes())
                .map_err(|error| LuaError::runtime(error.to_string()))?;
            *this
                .state
                .method
                .lock()
                .expect("Lua method state lock poisoned") = Some(method.clone());
            this.journal.push(Modification::MethodSet { method });
            Ok(())
        });
        fields.add_field_method_get("version", |_, this| Ok(this.request.version.clone()));
        fields.add_field_method_get("uri", |_, this| {
            Ok(UriView::from(
                &this
                    .state
                    .uri()
                    .expect("request Lua state always has a URI"),
            ))
        });
        fields.add_field_method_set("uri", |_, this, uri: String| {
            uri.parse::<hyper::Uri>()
                .map_err(|error| LuaError::runtime(error.to_string()))?;
            *this.state.uri.lock().expect("Lua URI state lock poisoned") = Some(uri.clone());
            this.journal.push(Modification::UriSet { uri });
            Ok(())
        });
        fields.add_field_method_get("headers", |_, this| {
            Ok(MutableHeaders {
                state: this.state.clone(),
                journal: this.journal.clone(),
            })
        });
        fields.add_field_method_get("body", |_, this| {
            Ok(MutableBody {
                state: this.state.clone(),
                journal: this.journal.clone(),
            })
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        add_mutable_tag_methods(methods);
    }
}

#[derive(Clone)]
struct ResponseView {
    response: ResponseData,
    state: SharedInterceptorState,
    journal: ModificationJournal,
}

impl UserData for ResponseView {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("status", |_, this| {
            Ok(this
                .state
                .status()
                .expect("response Lua state always has a status"))
        });
        fields.add_field_method_set("status", |_, this, status: i64| {
            if !(100..=999).contains(&status) {
                return Err(LuaError::runtime(
                    "response status must be an integer between 100 and 999",
                ));
            }
            let status = status as u16;
            *this
                .state
                .status
                .lock()
                .expect("Lua status state lock poisoned") = Some(status);
            this.journal.push(Modification::StatusSet { status });
            Ok(())
        });
        fields.add_field_method_get("version", |_, this| Ok(this.response.version.clone()));
        fields.add_field_method_get("headers", |_, this| {
            Ok(MutableHeaders {
                state: this.state.clone(),
                journal: this.journal.clone(),
            })
        });
        fields.add_field_method_get("body", |_, this| {
            Ok(MutableBody {
                state: this.state.clone(),
                journal: this.journal.clone(),
            })
        });
    }
}

#[derive(Clone)]
struct ResponseRequestView {
    request: RequestData,
    state: SharedInterceptorState,
    journal: ModificationJournal,
}

impl UserData for ResponseRequestView {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("method", |_, this| Ok(this.request.method.clone()));
        fields.add_field_method_get("version", |_, this| Ok(this.request.version.clone()));
        fields.add_field_method_get("uri", |_, this| Ok(UriView::from(&this.request.uri)));
        fields.add_field_method_get("headers", |_, this| {
            Ok(ReadHeaders(this.request.headers.clone()))
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        add_mutable_tag_methods(methods);
    }
}

trait MutableTagView {
    fn tag_state(&self) -> (&SharedInterceptorState, &ModificationJournal);
}

impl MutableTagView for RequestView {
    fn tag_state(&self) -> (&SharedInterceptorState, &ModificationJournal) {
        (&self.state, &self.journal)
    }
}

impl MutableTagView for ResponseRequestView {
    fn tag_state(&self) -> (&SharedInterceptorState, &ModificationJournal) {
        (&self.state, &self.journal)
    }
}

fn add_mutable_tag_methods<T, M>(methods: &mut M)
where
    T: MutableTagView + Clone + Send + 'static,
    M: UserDataMethods<T>,
{
    methods.add_method("get_tag", |_, this, key: String| {
        Ok(this
            .tag_state()
            .0
            .tags
            .lock()
            .expect("Lua tag state lock poisoned")
            .get(&key)
            .cloned())
    });
    methods.add_method("set_tag", |_, this, (key, value): (String, String)| {
        let (state, journal) = this.tag_state();
        state
            .tags
            .lock()
            .expect("Lua tag state lock poisoned")
            .insert(key.clone(), value.clone());
        journal.push(Modification::TagSet { key, value });
        Ok(())
    });
}

#[derive(Clone)]
struct MutableHeaders {
    state: SharedInterceptorState,
    journal: ModificationJournal,
}

impl UserData for MutableHeaders {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("get", |_, this, name: String| {
            let headers = this
                .state
                .headers
                .lock()
                .expect("Lua header state lock poisoned");
            Ok(header_values(&headers, &name).and_then(|values| values.first().cloned()))
        });
        methods.add_method("get_all", |_, this, name: String| {
            let headers = this
                .state
                .headers
                .lock()
                .expect("Lua header state lock poisoned");
            Ok(header_values(&headers, &name).cloned().unwrap_or_default())
        });
        methods.add_method("all", |lua, this, ()| {
            let result = lua.create_table()?;
            let headers = this
                .state
                .headers
                .lock()
                .expect("Lua header state lock poisoned");
            for (name, values) in headers.iter() {
                result.set(name.as_str(), values.clone())?;
            }
            Ok(result)
        });
        methods.add_method("append", |_, this, (name, value): (String, String)| {
            validate_header_input(&name, &value)?;
            let mut headers = this
                .state
                .headers
                .lock()
                .expect("Lua header state lock poisoned");
            let key = existing_header_name(&headers, &name).unwrap_or_else(|| name.to_lowercase());
            headers.entry(key).or_default().push(value.clone());
            this.journal
                .push(Modification::HeaderAppend { name, value });
            Ok(())
        });
        methods.add_method("set", |_, this, (name, value): (String, String)| {
            validate_header_input(&name, &value)?;
            let mut headers = this
                .state
                .headers
                .lock()
                .expect("Lua header state lock poisoned");
            remove_header(&mut headers, &name);
            headers.insert(name.to_lowercase(), vec![value.clone()]);
            this.journal.push(Modification::HeaderSet { name, value });
            Ok(())
        });
        methods.add_method("remove", |_, this, name: String| {
            let mut headers = this
                .state
                .headers
                .lock()
                .expect("Lua header state lock poisoned");
            let values = remove_header(&mut headers, &name).unwrap_or_default();
            this.journal
                .push(Modification::HeaderRemove { name, values });
            Ok(())
        });
    }
}

#[derive(Clone)]
struct MutableBody {
    state: SharedInterceptorState,
    journal: ModificationJournal,
}

impl UserData for MutableBody {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("replace_with_string", |_, this, content: String| {
            *this
                .state
                .body
                .lock()
                .expect("Lua body state lock poisoned") =
                Some(BodyReplacement::String(content.clone()));
            this.journal
                .push(Modification::BodyReplaceString { content });
            remove_content_encoding(&this.state, &this.journal);
            Ok(())
        });
        methods.add_method("replace_with_asset", |_, this, asset: mlua::AnyUserData| {
            let asset = asset
                .borrow::<LuaAsset>()
                .map_err(|_| LuaError::runtime("replace_with_asset requires an asset object"))?
                .0
                .clone();
            *this
                .state
                .body
                .lock()
                .expect("Lua body state lock poisoned") =
                Some(BodyReplacement::Asset(asset.clone()));
            this.journal.push(Modification::BodyReplaceAsset {
                asset_id: asset.metadata.id,
            });
            remove_content_encoding(&this.state, &this.journal);
            Ok(())
        });
        methods.add_method("as_string", |lua, this, ()| {
            let Some(bytes) = effective_text_bytes(this)? else {
                return Ok(Value::Nil);
            };
            match std::str::from_utf8(&bytes) {
                Ok(_) => Ok(Value::String(lua.create_string(bytes)?)),
                Err(_) => Ok(Value::Nil),
            }
        });
        methods.add_method("as_json", |lua, this, ()| {
            let Some(bytes) = effective_text_bytes(this)? else {
                return Ok(Value::Nil);
            };
            let Ok(text) = lua.create_string(bytes) else {
                return Ok(Value::Nil);
            };
            let json: Table = lua.globals().get("json")?;
            let decode: Function = json.get("decode")?;
            Ok(decode.call::<Value>(text).unwrap_or(Value::Nil))
        });
    }
}

#[derive(Clone)]
struct LuaAsset(Asset);

impl UserData for LuaAsset {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("id", |_, this| Ok(this.0.metadata.id.clone()));
        fields.add_field_method_get("size", |_, this| Ok(this.0.metadata.size));
        fields.add_field_method_get("content_type", |_, this| {
            Ok(this.0.metadata.content_type.clone())
        });
        fields.add_field_method_get("sha256", |_, this| Ok(this.0.metadata.sha256.clone()));
        fields.add_field_method_get("created_at", |_, this| Ok(this.0.metadata.created_at));
    }
}

const LUA_BODY_LIMIT: u64 = 16 * 1024 * 1024;

fn effective_text_bytes(body: &MutableBody) -> mlua::Result<Option<Vec<u8>>> {
    let headers = body.state.headers();
    let content_type = header_values(&headers, "content-type")
        .and_then(|values| values.first())
        .map(String::as_str)
        .unwrap_or_default();
    if !is_textual(content_type) {
        return Ok(None);
    }
    let replacement = body.state.body();
    let source = match replacement {
        Some(BodyReplacement::String(content)) => {
            if content.len() as u64 > LUA_BODY_LIMIT {
                return Err(LuaError::runtime("body exceeds the 16 MiB Lua limit"));
            }
            return Ok(Some(content.into_bytes()));
        }
        Some(BodyReplacement::Asset(asset)) => DeferredBodyRead::File(asset.path().to_path_buf()),
        None => {
            let reader = body
                .state
                .raw_body
                .lock()
                .expect("Lua raw body state lock poisoned")
                .clone()
                .ok_or_else(|| LuaError::runtime("raw body is unavailable"))?;
            let timeout = body
                .state
                .response_phase
                .then(|| {
                    body.state
                        .tags()
                        .get("_crab_resp_bodyframe_timeout")
                        .and_then(|value| value.parse::<u64>().ok())
                        .filter(|value| *value > 0)
                        .map(std::time::Duration::from_millis)
                })
                .flatten();
            reader.read(timeout).map_err(LuaError::runtime)?
        }
    };
    let encodings = header_values(&headers, "content-encoding")
        .into_iter()
        .flatten()
        .flat_map(|value| value.split(','))
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty() && value != "identity")
        .collect::<Vec<_>>();
    match source {
        DeferredBodyRead::Empty => {
            read_body_reader(Box::new(std::io::Cursor::new(Vec::new())), &encodings)
        }
        DeferredBodyRead::File(path) => read_body_file(&path, &encodings),
    }
}

fn read_text_body_source(source: BodySource) -> mlua::Result<Option<Vec<u8>>> {
    if !is_textual(source.content_type.as_deref().unwrap_or_default()) {
        return Ok(None);
    }
    match source.data {
        BodySourceData::Bytes(bytes) => read_body_reader(
            Box::new(std::io::Cursor::new(bytes)),
            &source.content_encodings,
        ),
        BodySourceData::File(path) => read_body_file(&path, &source.content_encodings),
    }
}

fn read_body_file(path: &std::path::Path, encodings: &[String]) -> mlua::Result<Option<Vec<u8>>> {
    read_body_reader(
        Box::new(File::open(path).map_err(LuaError::external)?),
        encodings,
    )
}

fn read_body_reader(
    mut reader: Box<dyn Read>,
    encodings: &[String],
) -> mlua::Result<Option<Vec<u8>>> {
    for encoding in encodings.iter().rev() {
        reader = match encoding.as_str() {
            "gzip" => Box::new(flate2::read::GzDecoder::new(reader)),
            "deflate" => Box::new(flate2::read::ZlibDecoder::new(reader)),
            "br" => Box::new(brotli::Decompressor::new(reader, 4096)),
            "zstd" => match zstd::stream::read::Decoder::new(reader) {
                Ok(decoder) => Box::new(decoder),
                Err(_) => return Ok(None),
            },
            _ => return Ok(None),
        };
    }
    let mut bytes = Vec::new();
    if reader
        .take(LUA_BODY_LIMIT + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return Ok(None);
    }
    if bytes.len() as u64 > LUA_BODY_LIMIT {
        return Err(LuaError::runtime("body exceeds the 16 MiB Lua limit"));
    }
    Ok(Some(bytes))
}

fn is_textual(content_type: &str) -> bool {
    content_type.starts_with("text/")
        || ["json", "xml", "javascript", "x-www-form-urlencoded"]
            .iter()
            .any(|kind| content_type.contains(kind))
}

fn remove_content_encoding(state: &SharedInterceptorState, journal: &ModificationJournal) {
    let mut headers = state
        .headers
        .lock()
        .expect("Lua header state lock poisoned");
    if let Some(values) = remove_header(&mut headers, "content-encoding") {
        journal.push(Modification::HeaderRemove {
            name: "content-encoding".into(),
            values,
        });
    }
}

fn validate_header_input(name: &str, value: &str) -> mlua::Result<()> {
    hyper::header::HeaderName::from_bytes(name.as_bytes())
        .map_err(|error| LuaError::runtime(error.to_string()))?;
    hyper::header::HeaderValue::from_str(value)
        .map_err(|error| LuaError::runtime(error.to_string()))?;
    Ok(())
}

fn header_values<'a>(headers: &'a HeaderValues, name: &str) -> Option<&'a Vec<String>> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, values)| values)
}

fn existing_header_name(headers: &HeaderValues, name: &str) -> Option<String> {
    headers
        .keys()
        .find(|key| key.eq_ignore_ascii_case(name))
        .cloned()
}

fn remove_header(headers: &mut BTreeMap<String, Vec<String>>, name: &str) -> Option<Vec<String>> {
    let key = existing_header_name(headers, name)?;
    headers.remove(&key)
}

#[cfg(test)]
mod tests {
    use std::{
        io::Write,
        net::SocketAddr,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use flate2::{Compression, write::GzEncoder};
    use tracing_subscriber::prelude::*;

    use crate::{
        log_buffer::{BufferLayer, LogBuffer},
        model::{
            CaptureOutcome, CaptureSummary, HeaderValues, RequestData, RequestTags, ResponseData,
            ScriptKind,
        },
        storage::{BodySide, BodySource, BodySourceData},
    };

    use super::{
        BodyReplacement, CaptureBodyAccess, ColumnEvaluator, FilterEvaluator, LUA_BODY_LIMIT,
        ModificationJournal, ResponseScriptContext, SharedInterceptorState, evaluate_column,
        evaluate_column_named, evaluate_filter, evaluate_filter_named, evaluate_routing,
        execute_request, execute_request_with_state, execute_response, execute_response_with_state,
        validate_script,
    };

    fn entry() -> CaptureSummary {
        CaptureSummary {
            id: 1,
            session_id: 2,
            source: "127.0.0.1".into(),
            request: RequestData {
                method: "GET".into(),
                uri: "https://example.com/path?q=1".into(),
                version: "HTTP/1.1".into(),
                headers: HeaderValues::from([("x-test".into(), vec!["old".into()])]),
                tags: Default::default(),
            },
            response: None,
            outcome: CaptureOutcome::InProgress,
            stage: "request".into(),
            error: None,
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn evaluates_filter_and_column() {
        let mut entry = entry();
        entry.request.tags.insert("team".into(), "checkout".into());
        assert!(
            evaluate_filter(
                "local input = ...; return entry.req.uri.host == input",
                "example.com",
                &entry
            )
            .unwrap()
        );
        assert!(
            !evaluate_filter(
                "local input = ...; return entry.req.uri.host == input",
                " example.com ",
                &entry
            )
            .unwrap()
        );
        assert_eq!(
            evaluate_column("return entry.req.headers:get('x-test')", &entry).unwrap(),
            "old"
        );
        assert!(
            evaluate_filter("return entry.req:get_tag('team') == 'checkout'", "", &entry).unwrap()
        );
    }

    #[test]
    fn historical_body_access_is_read_only_lazy_and_cached() {
        let mut entry = entry();
        entry.outcome = CaptureOutcome::Success;
        entry.response = Some(ResponseData {
            status: 200,
            version: "HTTP/1.1".into(),
            headers: HeaderValues::from([("content-type".into(), vec!["text/plain".into()])]),
        });
        let reads = Arc::new(AtomicUsize::new(0));
        let reads_for_resolver = reads.clone();
        let bodies = CaptureBodyAccess::new(move |side| {
            reads_for_resolver.fetch_add(1, Ordering::Relaxed);
            Ok(Some(match side {
                BodySide::Request => BodySource {
                    data: BodySourceData::Bytes(br#"{"order_id":"needle"}"#.to_vec()),
                    path: None,
                    stored_size: 21,
                    content_type: Some("application/json".into()),
                    content_encodings: Vec::new(),
                },
                BodySide::Response => BodySource {
                    data: BodySourceData::Bytes(b"response".to_vec()),
                    path: None,
                    stored_size: 8,
                    content_type: Some("text/plain".into()),
                    content_encodings: Vec::new(),
                },
            }))
        });

        let filter = FilterEvaluator::new(
            "local body = entry.req.body; local value = body:as_json(); \
             return value.order_id == ... and body:as_string() ~= nil",
            "body-filter",
        )
        .unwrap();
        assert!(
            filter
                .evaluate_with_bodies("needle", &entry, bodies.clone())
                .unwrap()
        );
        let column =
            ColumnEvaluator::new("return entry.resp.body:as_string()", "body-column").unwrap();
        assert_eq!(
            column.evaluate_with_bodies(&entry, bodies.clone()).unwrap(),
            "response"
        );
        assert_eq!(reads.load(Ordering::Relaxed), 2);

        assert!(
            ColumnEvaluator::new(
                "entry.req.body:replace_with_string('changed'); return ''",
                "read-only-body",
            )
            .unwrap()
            .evaluate_with_bodies(&entry, bodies)
            .is_err()
        );
        assert_eq!(
            ColumnEvaluator::new(
                "return entry.req.body:as_string() or 'unavailable'",
                "unavailable-body",
            )
            .unwrap()
            .evaluate_with_bodies(&entry, CaptureBodyAccess::unavailable())
            .unwrap(),
            "unavailable"
        );
    }

    #[test]
    fn historical_body_access_follows_content_encoding_and_size_rules() {
        let mut entry = entry();
        entry.outcome = CaptureOutcome::Success;
        let evaluate = |source: &str, body: BodySource| {
            ColumnEvaluator::new(source, "historical-body-rules")
                .unwrap()
                .evaluate_with_bodies(
                    &entry,
                    CaptureBodyAccess::new(move |_| Ok(Some(body.clone()))),
                )
        };

        assert_eq!(
            evaluate(
                "return entry.req.body:as_string() == nil",
                BodySource {
                    data: BodySourceData::Bytes(b"binary".to_vec()),
                    path: None,
                    stored_size: 6,
                    content_type: Some("application/octet-stream".into()),
                    content_encodings: Vec::new(),
                },
            )
            .unwrap(),
            "true"
        );
        assert_eq!(
            evaluate(
                "return entry.req.body:as_json() == nil",
                BodySource {
                    data: BodySourceData::Bytes(b"not json".to_vec()),
                    path: None,
                    stored_size: 8,
                    content_type: Some("text/plain".into()),
                    content_encodings: Vec::new(),
                },
            )
            .unwrap(),
            "true"
        );

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"decoded").unwrap();
        let compressed = encoder.finish().unwrap();
        assert_eq!(
            evaluate(
                "return entry.req.body:as_string()",
                BodySource {
                    stored_size: compressed.len() as u64,
                    data: BodySourceData::Bytes(compressed),
                    path: None,
                    content_type: Some("text/plain".into()),
                    content_encodings: vec!["gzip".into()],
                },
            )
            .unwrap(),
            "decoded"
        );
        assert!(
            evaluate(
                "return entry.req.body:as_string()",
                BodySource {
                    data: BodySourceData::Bytes(vec![0; LUA_BODY_LIMIT as usize + 1]),
                    path: None,
                    stored_size: LUA_BODY_LIMIT + 1,
                    content_type: Some("text/plain".into()),
                    content_encodings: Vec::new(),
                },
            )
            .is_err()
        );
    }

    #[test]
    fn reusable_evaluators_keep_rows_isolated() {
        let column = ColumnEvaluator::new(
            "counter = (counter or 0) + 1; \
             string.proxy_crab_counter = (string.proxy_crab_counter or 0) + 1; \
             return counter .. ':' .. string.proxy_crab_counter",
            "isolated-column",
        )
        .unwrap();
        assert_eq!(column.evaluate(&entry()).unwrap(), "1:1");
        assert_eq!(column.evaluate(&entry()).unwrap(), "1:1");

        let loaded = ColumnEvaluator::new(
            "load('loaded = (loaded or 0) + 1')(); return loaded",
            "isolated-load",
        )
        .unwrap();
        assert_eq!(loaded.evaluate(&entry()).unwrap(), "1");
        assert_eq!(loaded.evaluate(&entry()).unwrap(), "1");

        let filter = FilterEvaluator::new(
            "seen = (seen or 0) + 1; return seen == 1 and ... == 'match'",
            "isolated-filter",
        )
        .unwrap();
        assert!(filter.evaluate("match", &entry()).unwrap());
        assert!(filter.evaluate("match", &entry()).unwrap());
    }

    #[test]
    fn reusable_evaluator_resets_instruction_budget_per_row() {
        let evaluator =
            ColumnEvaluator::new("for _ = 1, 10000 do end; return 'ok'", "budget-column").unwrap();

        for _ in 0..8 {
            assert_eq!(evaluator.evaluate(&entry()).unwrap(), "ok");
        }
    }

    #[test]
    fn reusable_evaluator_resets_warning_counts_per_row() {
        let buffer = Arc::new(LogBuffer::new(8));
        let subscriber = tracing_subscriber::registry().with(BufferLayer::new(buffer.clone()));
        tracing::subscriber::with_default(subscriber, || {
            let evaluator = FilterEvaluator::new(
                r#"local value = json.decode('{"duplicate":1,"duplicate":2}')
                   return value.duplicate == 2"#,
                "reused-warning",
            )
            .unwrap();
            assert!(evaluator.evaluate("", &entry()).unwrap());
            assert!(evaluator.evaluate("", &entry()).unwrap());
        });

        let logs = buffer.query(None, 8);
        assert_eq!(logs.len(), 2);
        assert!(
            logs.iter()
                .all(|log| log.message.contains("duplicate_keys=1"))
        );
        assert!(
            logs.iter()
                .all(|log| !log.message.contains("duplicate_keys=2"))
        );
    }

    #[test]
    fn filter_requires_boolean_and_propagates_runtime_failures() {
        let entry = entry();
        assert!(evaluate_filter("return ...", "not-a-boolean", &entry).is_err());
        assert!(evaluate_filter("error('boom')", "input", &entry).is_err());
        assert!(evaluate_filter("while true do end", "input", &entry).is_err());
    }

    #[test]
    fn routing_exposes_request_and_source_metadata_and_validates_returns() {
        let request = entry().request;
        let source: SocketAddr = "127.0.0.1:4321".parse().unwrap();
        let script = r#"
            if phase == "http"
              and req.method == "GET"
              and req.version == "HTTP/1.1"
              and req.authority == "example.com:443"
              and req.uri.raw == "https://example.com/path?q=1"
              and req.uri.scheme == "https"
              and req.uri.host == "example.com"
              and req.uri.port == nil
              and req.uri.path == "/path"
              and req.uri.query == "q=1"
              and source.ip == "127.0.0.1"
              and source.port == 4321
              and source.address == "127.0.0.1:4321"
            then
              return true
            end
            return nil
        "#;

        assert_eq!(
            evaluate_routing(script, "http", &request, "example.com:443", source, "route",)
                .unwrap(),
            Some(true)
        );
        assert_eq!(
            evaluate_routing("return nil", "http", &request, "", source, "route").unwrap(),
            None
        );
        assert_eq!(
            evaluate_routing("return false", "http", &request, "", source, "route").unwrap(),
            Some(false)
        );
        assert!(
            evaluate_routing("return 'capture'", "http", &request, "", source, "route").is_err()
        );
    }

    #[test]
    fn request_script_mutates_method_uri_headers_and_body() {
        let effects = execute_request(
            "req.method = 'BREW'; \
             req.uri = 'http://alternate.example/new-path?q=2'; \
             assert(req.method == 'BREW'); \
             assert(req.uri.host == 'alternate.example' and req.uri.path == '/new-path'); \
             req.headers:set('x-test', 'new'); req.body:replace_with_string('body'); \
             assert(req:get_tag('missing') == nil); req:set_tag('empty', ''); \
             req:set_tag('team', 'one'); req:set_tag('team', 'two')",
            &entry().request,
        )
        .unwrap();
        assert_eq!(effects.method.as_deref(), Some("BREW"));
        assert_eq!(
            effects.uri.as_deref(),
            Some("http://alternate.example/new-path?q=2")
        );
        assert_eq!(effects.headers["x-test"], vec!["new"]);
        assert_eq!(effects.body, Some(BodyReplacement::String("body".into())));
        assert_eq!(effects.tags["empty"], "");
        assert_eq!(effects.tags["team"], "two");
        assert_eq!(effects.modifications.len(), 8);
        assert!(matches!(
            &effects.modifications[0],
            crate::model::Modification::Snapshot {
                request: Some(snapshot),
                response: None,
            } if snapshot.method == "GET"
                && snapshot.uri == "https://example.com/path?q=1"
                && snapshot.version == "HTTP/1.1"
                && snapshot.headers["x-test"] == vec!["old"]
                && snapshot.body == crate::model::BodySourceType::Original
        ));
    }

    #[tokio::test]
    async fn body_getters_follow_text_json_encoding_and_size_rules() {
        let root = tempfile::tempdir().unwrap();
        let store = crate::asset::AssetStore::open(root.path()).unwrap();
        let mut invalid_upload = store
            .begin_upload("invalid.gz", "text/plain".into())
            .await
            .unwrap();
        invalid_upload.write(b"not gzip").await.unwrap();
        invalid_upload.finish().await.unwrap();
        let mut large_upload = store
            .begin_upload("large.txt", "text/plain".into())
            .await
            .unwrap();
        let chunk = vec![0; 1024 * 1024];
        for _ in 0..16 {
            large_upload.write(&chunk).await.unwrap();
        }
        large_upload.write(&[0]).await.unwrap();
        large_upload.finish().await.unwrap();

        let request = entry().request;
        let state = SharedInterceptorState::new_request(
            request.method.clone(),
            request.uri.clone(),
            HeaderValues::from([("content-type".into(), vec!["text/plain".into()])]),
            RequestTags::new(),
        );
        state.set_asset_store(store.clone());
        let journal = ModificationJournal::for_current_request(&request, &state);
        let run = |source: &str, state: SharedInterceptorState, journal: ModificationJournal| {
            execute_request_with_state(source, &request, state, journal, "body-getters", None, None)
                .unwrap()
                .1
        };

        state.set_body(Some(BodyReplacement::String("{\"ok\":true}".into())));
        assert!(run("assert(req.body:as_string() == '{\"ok\":true}'); assert(req.body:as_json().ok == true)", state.clone(), journal.clone()).is_none());
        state.set_body(Some(BodyReplacement::String("not json".into())));
        assert!(
            run(
                "assert(req.body:as_json() == nil)",
                state.clone(),
                journal.clone()
            )
            .is_none()
        );

        *state.headers.lock().unwrap() = HeaderValues::from([(
            "content-type".into(),
            vec!["application/octet-stream".into()],
        )]);
        assert!(
            run(
                "assert(req.body:as_string() == nil and req.body:as_json() == nil)",
                state.clone(),
                journal.clone()
            )
            .is_none()
        );

        *state.headers.lock().unwrap() = HeaderValues::from([
            ("content-type".into(), vec!["text/plain".into()]),
            ("content-encoding".into(), vec!["gzip".into()]),
        ]);
        state.set_body(Some(BodyReplacement::Asset(
            store.get("invalid.gz").unwrap().unwrap(),
        )));
        assert!(
            run(
                "assert(req.body:as_string() == nil)",
                state.clone(),
                journal.clone()
            )
            .is_none()
        );

        state.headers.lock().unwrap().remove("content-encoding");
        state.set_body(Some(BodyReplacement::Asset(
            store.get("large.txt").unwrap().unwrap(),
        )));
        assert!(
            run("req.body:as_string()", state, journal)
                .unwrap()
                .contains("16 MiB")
        );
    }

    #[test]
    fn request_method_and_uri_reject_values_the_http_stack_cannot_represent() {
        let request = entry().request;
        assert!(execute_request("req.method = 'BAD METHOD'", &request).is_err());
        assert!(execute_request("req.uri = 'http://['", &request).is_err());
    }

    #[test]
    fn response_script_mutates_status_and_reads_request_context() {
        let mut request = entry().request;
        request.tags.insert("team".into(), "checkout".into());
        let response = ResponseData {
            status: 200,
            version: "HTTP/1.1".into(),
            headers: HeaderValues::new(),
        };
        let state = SharedInterceptorState::new_response(
            response.status,
            response.headers.clone(),
            request.tags.clone(),
        );
        let journal = ModificationJournal::for_response(&response, None);
        let (effects, error) = execute_response_with_state(
            "assert(req.method == 'GET'); \
             assert(req.version == 'HTTP/1.1'); \
             assert(req.uri.path == '/path' and req.uri.query == 'q=1'); \
             assert(req.headers:get('x-test') == 'old'); \
             assert(req.body == nil and req:get_tag('team') == 'checkout'); \
             assert(resp.status == 200); resp.status = 777; \
             req:set_tag('empty', ''); error('boom')",
            ResponseScriptContext {
                request: &request,
                response: &response,
            },
            state,
            journal,
            "response-tags",
            Some(1),
            None,
        )
        .unwrap();

        assert_eq!(effects.status, Some(777));
        assert_eq!(effects.tags["empty"], "");
        assert!(error.unwrap().contains("boom"));
        assert!(effects.modifications.iter().any(|modification| matches!(
            modification,
            crate::model::Modification::StatusSet { status: 777 }
        )));
        assert!(matches!(
            effects.modifications.last(),
            Some(crate::model::Modification::TagSet { key, value })
                if key == "empty" && value.is_empty()
        ));
    }

    #[test]
    fn response_status_accepts_http_type_range_and_rejects_unrepresentable_values() {
        let response = ResponseData {
            status: 200,
            version: "HTTP/1.1".into(),
            headers: HeaderValues::new(),
        };
        assert_eq!(
            execute_response("resp.status = 100; resp.status = 999", &response)
                .unwrap()
                .status,
            Some(999)
        );
        assert!(
            execute_response("resp.status = 99", &response)
                .unwrap_err()
                .to_string()
                .contains("between 100 and 999")
        );
        assert!(
            execute_response("resp.status = 1000", &response)
                .unwrap_err()
                .to_string()
                .contains("between 100 and 999")
        );
    }

    #[test]
    fn validates_syntax_and_stops_runaway_script() {
        assert!(validate_script(ScriptKind::Column, "return (").is_err());
        assert!(validate_script(ScriptKind::Filter, "return (").is_err());
        assert!(evaluate_column("return string.rep('x', 20 * 1024 * 1024)", &entry()).is_err());
    }

    #[test]
    fn base64_module_supports_standard_url_safe_and_binary_round_trips() {
        let entry = entry();
        assert_eq!(
            evaluate_column("return base64.encode('hello')", &entry).unwrap(),
            "aGVsbG8="
        );
        assert_eq!(
            evaluate_column(
                "return base64.url_encode(string.char(251, 255), false)",
                &entry,
            )
            .unwrap(),
            "-_8"
        );
        assert!(
            evaluate_filter(
                r#"local value = base64.url_decode(base64.url_encode(string.char(0, 255), true))
                   return #value == 2
                      and string.byte(value, 1) == 0
                      and string.byte(value, 2) == 255"#,
                "",
                &entry,
            )
            .unwrap(),
        );
        assert!(
            evaluate_column("return base64.decode('aGV sbG8=')", &entry).is_err(),
            "standard decoding must reject whitespace",
        );
        assert!(
            evaluate_column("return base64.decode('aGVsbG8')", &entry).is_err(),
            "standard decoding must require canonical padding",
        );
        assert!(
            evaluate_filter(
                "local value = base64.url_decode('-_8='); \
                 return string.byte(value, 1) == 251 and string.byte(value, 2) == 255",
                "",
                &entry,
            )
            .unwrap(),
        );
        assert!(
            evaluate_column("base64.encode = nil; return ''", &entry).is_err(),
            "module fields must be read-only",
        );
        assert!(
            evaluate_column("return base64.url_encode('value')", &entry,).is_err(),
            "URL-safe encoding must require the padding boolean",
        );
        assert!(
            evaluate_column(
                "return base64.encode(string.rep('x', 12 * 1024 * 1024 + 1))",
                &entry,
            )
            .is_err(),
            "predicted output larger than 16 MiB must be rejected",
        );
    }

    #[test]
    fn json_module_round_trips_types_and_preserves_empty_containers() {
        let entry = entry();
        assert_eq!(
            evaluate_column(
                "return json.encode({z = 1, a = json.null, list = json.array({})})",
                &entry,
            )
            .unwrap(),
            r#"{"a":null,"list":[],"z":1}"#,
        );
        assert_eq!(
            evaluate_column(
                r#"local value = json.decode('{"array":[],"object":{}}')
                   return json.encode(value)"#,
                &entry,
            )
            .unwrap(),
            r#"{"array":[],"object":{}}"#,
        );
        assert!(
            evaluate_column(
                "local value = {}; value.self = value; return json.encode(value)",
                &entry,
            )
            .is_err(),
            "cycles must be rejected",
        );
        assert_eq!(
            evaluate_column(
                "local value = json.array({}); table.insert(value, 1); return json.encode(value)",
                &entry,
            )
            .unwrap(),
            "[1]",
        );
        assert_eq!(
            evaluate_column(
                "local shared = {x = 1}; return json.encode({a = shared, b = shared})",
                &entry,
            )
            .unwrap(),
            r#"{"a":{"x":1},"b":{"x":1}}"#,
        );
        assert!(
            evaluate_column(
                "return json.encode(json.array(setmetatable({}, {})))",
                &entry,
            )
            .is_err(),
            "forced containers must reject existing metatables",
        );
        assert!(
            evaluate_column("return json.encode({[1] = true, name = 'mixed'})", &entry).is_err(),
            "mixed keys must be rejected",
        );
        assert!(
            evaluate_column("return json.encode({[2] = true})", &entry).is_err(),
            "sparse arrays must be rejected",
        );
        assert!(
            evaluate_column("return json.encode(0 / 0)", &entry).is_err(),
            "non-finite numbers must be rejected",
        );
        assert!(
            evaluate_column(
                r#"local root = json.array({})
                   local current = root
                   for _ = 1, 128 do
                     local child = json.array({})
                     current[1] = child
                     current = child
                   end
                   return json.encode(root)"#,
                &entry,
            )
            .is_err(),
            "nesting deeper than 128 containers must be rejected",
        );
        assert!(
            evaluate_column(
                r#"local shared = string.rep("x", 1024 * 1024)
                   local values = json.array({})
                   for index = 1, 17 do
                     values[index] = shared
                   end
                   return json.encode(values)"#,
                &entry,
            )
            .is_err(),
            "repeated references must not expand JSON beyond 16 MiB",
        );
        assert!(
            evaluate_column("json.null = nil; return ''", &entry).is_err(),
            "JSON module fields must be read-only",
        );
    }

    #[test]
    fn codec_modules_are_available_to_all_script_types() {
        let entry = entry();
        assert!(
            evaluate_filter(
                "return json.decode('{\"ok\":true}').ok and base64.decode('eA==') == 'x'",
                "",
                &entry,
            )
            .unwrap(),
        );
        assert_eq!(
            evaluate_column("return base64.encode(json.encode(json.array({1})))", &entry).unwrap(),
            "WzFd",
        );
        assert_eq!(
            evaluate_routing(
                "return base64.decode('dHJ1ZQ==') == 'true'",
                "http",
                &entry.request,
                "example.com",
                "127.0.0.1:1".parse().unwrap(),
                "codec-route",
            )
            .unwrap(),
            Some(true),
        );

        let request_effects = execute_request(
            "req.headers:set('x-codec', base64.encode(json.encode({ok = true})))",
            &entry.request,
        )
        .unwrap();
        assert_eq!(request_effects.headers["x-codec"], ["eyJvayI6dHJ1ZX0="]);

        let response = ResponseData {
            status: 200,
            version: "HTTP/1.1".into(),
            headers: HeaderValues::new(),
        };
        let response_effects = execute_response(
            "resp.headers:set('x-codec', base64.url_encode(json.encode(json.array({1})), false))",
            &response,
        )
        .unwrap();
        assert_eq!(response_effects.headers["x-codec"], ["WzFd"]);
    }

    #[test]
    fn json_decode_aggregates_contextual_warnings_in_system_logs() {
        let buffer = Arc::new(LogBuffer::new(8));
        let subscriber = tracing_subscriber::registry().with(BufferLayer::new(buffer.clone()));
        let result = tracing::subscriber::with_default(subscriber, || {
            evaluate_filter_named(
                r#"local value = json.decode(
                     '{"large":9223372036854775808,"duplicate":1,"duplicate":2}'
                   )
                   return math.type(value.large) == "float" and value.duplicate == 2"#,
                "",
                &entry(),
                "warning-check",
            )
        });
        assert!(result.unwrap());

        let logs = buffer.query(None, 8);
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].level, "WARN");
        assert!(logs[0].message.contains("filter script \"warning-check\""));
        assert!(logs[0].message.contains("capture_log_id=1"));
        assert!(logs[0].message.contains("lossy_numbers=1"));
        assert!(logs[0].message.contains("duplicate_keys=1"));
        assert!(!logs[0].message.contains("9223372036854775808"));

        buffer.clear();
        let subscriber = tracing_subscriber::registry().with(BufferLayer::new(buffer.clone()));
        let result = tracing::subscriber::with_default(subscriber, || {
            evaluate_filter_named(
                r#"json.decode('{"duplicate":1,"duplicate":2} trailing')
                   return true"#,
                "",
                &entry(),
                "failed-decode",
            )
        });
        assert!(result.is_err());
        assert!(
            buffer.query(None, 8).is_empty(),
            "failed JSON decodes must not emit conversion warnings",
        );

        // A named column execution uses the same aggregation path.
        assert_eq!(
            evaluate_column_named("return json.encode(nil)", &entry(), "null-column").unwrap(),
            "null",
        );
    }
}
