use std::{
    collections::BTreeMap,
    fs::File,
    io::Read,
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

pub const INSTRUCTION_LIMIT: u32 = 100_000;
pub const MEMORY_LIMIT: usize = 16 * 1024 * 1024;
const MAX_BODY_REPLACEMENT_BYTES: u64 = 64 * 1024 * 1024;

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
    let lua = safe_lua()?;
    lua.load(source)
        .set_name(match kind {
            ScriptKind::Column => "column.lua",
            ScriptKind::RequestInterceptor => "request-interceptor.lua",
            ScriptKind::ResponseInterceptor => "response-interceptor.lua",
        })
        .into_function()
        .map(|_| ())
        .map_err(Into::into)
}

pub fn evaluate_filter(source: &str, entry: &CaptureSummary) -> Result<bool> {
    let lua = safe_lua()?;
    lua.globals().set("entry", EntryView(entry.clone()))?;
    match lua.load(source).eval::<Value>()? {
        Value::Boolean(value) => Ok(value),
        _ => bail!("filter script must return a boolean"),
    }
}

pub fn evaluate_column(source: &str, entry: &CaptureSummary) -> Result<String> {
    let lua = safe_lua()?;
    lua.globals().set("entry", EntryView(entry.clone()))?;
    match lua.load(source).eval::<Value>()? {
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
    let lua = safe_lua()?;
    let state = MutationState::new(request.headers.clone());
    lua.globals().set(
        "req",
        RequestView {
            request: request.clone(),
            state: state.clone(),
        },
    )?;
    let error = lua.load(source).exec().err().map(|error| error.to_string());
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
    let lua = safe_lua()?;
    let state = MutationState::new(response.headers.clone());
    lua.globals().set(
        "resp",
        ResponseView {
            response: response.clone(),
            state: state.clone(),
        },
    )?;
    let error = lua.load(source).exec().err().map(|error| error.to_string());
    Ok((state.into_effects(), error))
}

fn safe_lua() -> Result<Lua> {
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
    Ok(lua)
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
                scheme: uri.scheme_str().map(str::to_string).unwrap_or_default(),
                host: uri.host().map(str::to_string).unwrap_or_default(),
                port: uri.port_u16(),
                path: uri.path().to_string(),
                query: uri.query().map(str::to_string).unwrap_or_default(),
            };
        }
        Self {
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
        fields.add_field_method_get("scheme", |_, this| Ok(this.scheme.clone()));
        fields.add_field_method_get("host", |_, this| Ok(this.host.clone()));
        fields.add_field_method_get("port", |_, this| Ok(this.port));
        fields.add_field_method_get("path", |_, this| Ok(this.path.clone()));
        fields.add_field_method_get("query", |_, this| Ok(this.query.clone()));
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
    use crate::model::{CaptureOutcome, CaptureSummary, HeaderValues, RequestData, ScriptKind};

    use super::{
        BodyReplacement, evaluate_column, evaluate_filter, execute_request, validate_script,
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
        assert!(evaluate_filter("return entry.req.uri.host == 'example.com'", &entry).unwrap());
        assert_eq!(
            evaluate_column("return entry.req.headers:get('x-test')", &entry).unwrap(),
            "old"
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
        assert!(evaluate_filter("while true do end", &entry()).is_err());
        assert!(evaluate_column("return string.rep('x', 20 * 1024 * 1024)", &entry()).is_err());
    }
}
