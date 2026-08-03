use std::{
    collections::BTreeMap,
    fs::File,
    io::Read,
    net::SocketAddr,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
};

use anyhow::{Result, anyhow, bail};
use mlua::{
    Error as LuaError, HookTriggers, Lua, UserData, UserDataFields, UserDataMethods, Value, VmState,
};

use crate::model::{
    CaptureSummary, HeaderValues, Modification, RequestData, ResponseData, ScriptKind,
};

mod codec;

pub const INSTRUCTION_LIMIT: u32 = 100_000;
pub const MEMORY_LIMIT: usize = 16 * 1024 * 1024;
const MAX_BODY_REPLACEMENT_BYTES: u64 = 64 * 1024 * 1024;
const ANONYMOUS_SCRIPT_NAME: &str = "<anonymous>";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyReplacement {
    String(String),
    File(String),
}

#[derive(Debug, Clone)]
pub struct ScriptEffects {
    pub headers: HeaderValues,
    pub body: Option<BodyReplacement>,
    pub modifications: Vec<Modification>,
}

pub fn validate_script(kind: ScriptKind, source: &str) -> Result<()> {
    let (lua, _) = safe_lua()?;
    lua.load(source)
        .set_name(match kind {
            ScriptKind::Column => "column.lua",
            ScriptKind::Filter => "filter.lua",
            ScriptKind::Routing => "routing.lua",
            ScriptKind::RequestInterceptor => "request-interceptor.lua",
            ScriptKind::ResponseInterceptor => "response-interceptor.lua",
        })
        .into_function()
        .map(|_| ())
        .map_err(Into::into)
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
    let (lua, warnings) = safe_lua()?;
    lua.globals().set("entry", EntryView(entry.clone()))?;
    let result = lua.load(source).call::<Value>(argument);
    log_json_warnings(&warnings, ScriptKind::Filter, script_name, Some(entry.id));
    match result? {
        Value::Boolean(value) => Ok(value),
        _ => bail!("filter script must return a boolean"),
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
    let (lua, warnings) = safe_lua()?;
    lua.globals().set("entry", EntryView(entry.clone()))?;
    let result = lua.load(source).eval::<Value>();
    log_json_warnings(&warnings, ScriptKind::Column, script_name, Some(entry.id));
    match result? {
        Value::Nil => Ok(String::new()),
        Value::Boolean(value) => Ok(value.to_string()),
        Value::Integer(value) => Ok(value.to_string()),
        Value::Number(value) => Ok(value.to_string()),
        Value::String(value) => Ok(value.to_str()?.to_string()),
        _ => bail!("column script must return nil or a scalar value"),
    }
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
    let (lua, warnings) = safe_lua()?;
    let state = MutationState::new(request.headers.clone());
    lua.globals().set(
        "req",
        RequestView {
            request: request.clone(),
            state: state.clone(),
        },
    )?;
    let error = lua.load(source).exec().err().map(|error| error.to_string());
    log_json_warnings(
        &warnings,
        ScriptKind::RequestInterceptor,
        script_name,
        capture_log_id,
    );
    Ok((state.into_effects(), error))
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
    let (lua, warnings) = safe_lua()?;
    let state = MutationState::new(response.headers.clone());
    lua.globals().set(
        "resp",
        ResponseView {
            response: response.clone(),
            state: state.clone(),
        },
    )?;
    let error = lua.load(source).exec().err().map(|error| error.to_string());
    log_json_warnings(
        &warnings,
        ScriptKind::ResponseInterceptor,
        script_name,
        capture_log_id,
    );
    Ok((state.into_effects(), error))
}

fn safe_lua() -> Result<(Lua, codec::JsonWarningState)> {
    let lua = Lua::new();
    lua.set_memory_limit(MEMORY_LIMIT)?;
    for library in [
        "io", "os", "package", "debug", "dofile", "loadfile", "require",
    ] {
        lua.globals().set(library, Value::Nil)?;
    }
    let executed = Arc::new(AtomicU32::new(0));
    lua.set_hook(
        HookTriggers::new().every_nth_instruction(1000),
        move |_lua, _debug| {
            if executed.fetch_add(1000, Ordering::Relaxed) + 1000 > INSTRUCTION_LIMIT {
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

#[derive(Clone)]
struct EntryView(CaptureSummary);

impl UserData for EntryView {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("id", |_, this| Ok(this.0.id));
        fields.add_field_method_get("req", |_, this| Ok(ReadRequest(this.0.request.clone())));
        fields.add_field_method_get("resp", |_, this| {
            Ok(this.0.response.clone().map(ReadResponse))
        });
    }
}

#[derive(Clone)]
struct ReadRequest(RequestData);

impl UserData for ReadRequest {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("method", |_, this| Ok(this.0.method.clone()));
        fields.add_field_method_get("version", |_, this| Ok(this.0.version.clone()));
        fields.add_field_method_get("uri", |_, this| Ok(UriView::from(&this.0.uri)));
        fields.add_field_method_get("headers", |_, this| Ok(ReadHeaders(this.0.headers.clone())));
    }
}

#[derive(Clone)]
struct ReadResponse(ResponseData);

impl UserData for ReadResponse {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("status", |_, this| Ok(this.0.status));
        fields.add_field_method_get("version", |_, this| Ok(this.0.version.clone()));
        fields.add_field_method_get("headers", |_, this| Ok(ReadHeaders(this.0.headers.clone())));
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
struct MutationState {
    headers: Arc<Mutex<HeaderValues>>,
    body: Arc<Mutex<Option<BodyReplacement>>>,
    modifications: Arc<Mutex<Vec<Modification>>>,
}

impl MutationState {
    fn new(headers: HeaderValues) -> Self {
        Self {
            headers: Arc::new(Mutex::new(headers.clone())),
            body: Arc::new(Mutex::new(None)),
            modifications: Arc::new(Mutex::new(vec![Modification::Snapshot { headers }])),
        }
    }

    fn into_effects(self) -> ScriptEffects {
        ScriptEffects {
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
            modifications: self
                .modifications
                .lock()
                .expect("Lua modification state lock poisoned")
                .clone(),
        }
    }
}

#[derive(Clone)]
struct RequestView {
    request: RequestData,
    state: MutationState,
}

impl UserData for RequestView {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("method", |_, this| Ok(this.request.method.clone()));
        fields.add_field_method_get("version", |_, this| Ok(this.request.version.clone()));
        fields.add_field_method_get("uri", |_, this| Ok(UriView::from(&this.request.uri)));
        fields.add_field_method_get("headers", |_, this| Ok(MutableHeaders(this.state.clone())));
        fields.add_field_method_get("body", |_, this| Ok(MutableBody(this.state.clone())));
    }
}

#[derive(Clone)]
struct ResponseView {
    response: ResponseData,
    state: MutationState,
}

impl UserData for ResponseView {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("status", |_, this| Ok(this.response.status));
        fields.add_field_method_get("version", |_, this| Ok(this.response.version.clone()));
        fields.add_field_method_get("headers", |_, this| Ok(MutableHeaders(this.state.clone())));
        fields.add_field_method_get("body", |_, this| Ok(MutableBody(this.state.clone())));
    }
}

#[derive(Clone)]
struct MutableHeaders(MutationState);

impl UserData for MutableHeaders {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("get", |_, this, name: String| {
            let headers = this
                .0
                .headers
                .lock()
                .expect("Lua header state lock poisoned");
            Ok(header_values(&headers, &name).and_then(|values| values.first().cloned()))
        });
        methods.add_method("get_all", |_, this, name: String| {
            let headers = this
                .0
                .headers
                .lock()
                .expect("Lua header state lock poisoned");
            Ok(header_values(&headers, &name).cloned().unwrap_or_default())
        });
        methods.add_method("all", |lua, this, ()| {
            let result = lua.create_table()?;
            let headers = this
                .0
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
                .0
                .headers
                .lock()
                .expect("Lua header state lock poisoned");
            let key = existing_header_name(&headers, &name).unwrap_or_else(|| name.to_lowercase());
            headers.entry(key).or_default().push(value.clone());
            this.0
                .modifications
                .lock()
                .expect("Lua modification state lock poisoned")
                .push(Modification::HeaderAppend { name, value });
            Ok(())
        });
        methods.add_method("set", |_, this, (name, value): (String, String)| {
            validate_header_input(&name, &value)?;
            let mut headers = this
                .0
                .headers
                .lock()
                .expect("Lua header state lock poisoned");
            remove_header(&mut headers, &name);
            headers.insert(name.to_lowercase(), vec![value.clone()]);
            this.0
                .modifications
                .lock()
                .expect("Lua modification state lock poisoned")
                .push(Modification::HeaderSet { name, value });
            Ok(())
        });
        methods.add_method("remove", |_, this, name: String| {
            let mut headers = this
                .0
                .headers
                .lock()
                .expect("Lua header state lock poisoned");
            let values = remove_header(&mut headers, &name).unwrap_or_default();
            this.0
                .modifications
                .lock()
                .expect("Lua modification state lock poisoned")
                .push(Modification::HeaderRemove { name, values });
            Ok(())
        });
    }
}

#[derive(Clone)]
struct MutableBody(MutationState);

impl UserData for MutableBody {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("replace_with_string", |_, this, content: String| {
            *this.0.body.lock().expect("Lua body state lock poisoned") =
                Some(BodyReplacement::String(content.clone()));
            this.0
                .modifications
                .lock()
                .expect("Lua modification state lock poisoned")
                .push(Modification::BodyReplaceString { content });
            Ok(())
        });
        methods.add_method("replace_with_file", |_, this, path: String| {
            if !Path::new(&path).is_absolute() {
                return Err(LuaError::runtime("body replacement path must be absolute"));
            }
            *this.0.body.lock().expect("Lua body state lock poisoned") =
                Some(BodyReplacement::File(path.clone()));
            this.0
                .modifications
                .lock()
                .expect("Lua modification state lock poisoned")
                .push(Modification::BodyReplaceFile { path });
            Ok(())
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

pub fn read_body_replacement(replacement: &BodyReplacement) -> Result<Vec<u8>> {
    match replacement {
        BodyReplacement::String(content) => {
            if content.len() as u64 > MAX_BODY_REPLACEMENT_BYTES {
                bail!("body replacement string exceeds the 64 MiB limit");
            }
            Ok(content.as_bytes().to_vec())
        }
        BodyReplacement::File(path) => {
            let file = File::open(path)
                .map_err(|error| anyhow!("failed to open body file {path}: {error}"))?;
            let mut body = Vec::new();
            file.take(MAX_BODY_REPLACEMENT_BYTES + 1)
                .read_to_end(&mut body)
                .map_err(|error| anyhow!("failed to read body file {path}: {error}"))?;
            if body.len() as u64 > MAX_BODY_REPLACEMENT_BYTES {
                bail!("body replacement file exceeds the 64 MiB limit");
            }
            Ok(body)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{net::SocketAddr, sync::Arc};

    use tracing_subscriber::prelude::*;

    use crate::{
        log_buffer::{BufferLayer, LogBuffer},
        model::{
            CaptureOutcome, CaptureSummary, HeaderValues, RequestData, ResponseData, ScriptKind,
        },
    };

    use super::{
        BodyReplacement, evaluate_column, evaluate_column_named, evaluate_filter,
        evaluate_filter_named, evaluate_routing, execute_request, execute_response,
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
        let entry = entry();
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
    fn request_script_mutates_headers_and_body() {
        let effects = execute_request(
            "req.headers:set('x-test', 'new'); req.body:replace_with_string('body')",
            &entry().request,
        )
        .unwrap();
        assert_eq!(effects.headers["x-test"], vec!["new"]);
        assert_eq!(effects.body, Some(BodyReplacement::String("body".into())));
        assert_eq!(effects.modifications.len(), 3);
        assert!(matches!(
            &effects.modifications[0],
            crate::model::Modification::Snapshot { headers }
                if headers["x-test"] == vec!["old"]
        ));
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
