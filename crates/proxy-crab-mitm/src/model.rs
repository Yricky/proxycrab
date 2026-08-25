use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub type HeaderValues = BTreeMap<String, Vec<String>>;
pub type RequestTags = BTreeMap<String, String>;
pub const MAX_SESSION_INTERCEPTORS_PER_KIND: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppConfig {
    #[serde(default = "default_proxy_host")]
    pub proxy_host: String,
    #[serde(default = "default_proxy_port")]
    pub proxy_port: u16,
    #[serde(default = "default_api_port")]
    pub api_port: u16,
    #[serde(default)]
    pub routing_script_name: Option<String>,
    #[serde(default)]
    pub active_session_id: Option<u64>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            proxy_host: default_proxy_host(),
            proxy_port: default_proxy_port(),
            api_port: default_api_port(),
            routing_script_name: None,
            active_session_id: None,
        }
    }
}

fn default_proxy_host() -> String {
    "0.0.0.0".into()
}

const fn default_proxy_port() -> u16 {
    8089
}

const fn default_api_port() -> u16 {
    18089
}

pub fn default_columns() -> Vec<Column> {
    vec![
        Column::Method { width: 50.0 },
        Column::Uri { width: 300.0 },
        Column::Code { width: 50.0 },
        Column::Source { width: 100.0 },
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionView {
    #[serde(default = "default_columns")]
    pub columns: Vec<Column>,
    #[serde(default)]
    pub filter: SessionFilter,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FilterColumn {
    Method,
    Uri,
    Code,
    Source,
    Stage,
    Script { script_name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FilterOption {
    Column {
        column: FilterColumn,
        #[serde(default)]
        regex: bool,
    },
    Script {
        script_name: String,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionFilter {
    pub option: Option<FilterOption>,
    pub input: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionInterceptor {
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionInterceptors {
    #[serde(default)]
    pub request: Vec<SessionInterceptor>,
    #[serde(default)]
    pub response: Vec<SessionInterceptor>,
}

impl Default for SessionView {
    fn default() -> Self {
        Self {
            columns: default_columns(),
            filter: SessionFilter::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Column {
    Method { width: f32 },
    Uri { width: f32 },
    Code { width: f32 },
    Source { width: f32 },
    Stage { width: f32 },
    Script { width: f32, script_name: String },
}

impl Column {
    pub fn width(&self) -> f32 {
        match self {
            Self::Method { width }
            | Self::Uri { width }
            | Self::Code { width }
            | Self::Source { width }
            | Self::Stage { width }
            | Self::Script { width, .. } => *width,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Method { .. } => "method",
            Self::Uri { .. } => "uri",
            Self::Code { .. } => "code",
            Self::Source { .. } => "source",
            Self::Stage { .. } => "stage",
            Self::Script { script_name, .. } => script_name,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionMetadata {
    pub id: u64,
    pub name: String,
    pub created_at: u64,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Script {
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScriptKind {
    Column,
    Filter,
    Routing,
    RequestInterceptor,
    ResponseInterceptor,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InterceptorKind {
    Request,
    Response,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResolvedSessionInterceptor {
    pub name: String,
    pub enabled: bool,
    pub valid: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResolvedSessionInterceptors {
    pub session_id: u64,
    pub request: Vec<ResolvedSessionInterceptor>,
    pub response: Vec<ResolvedSessionInterceptor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InterceptorLibraryItem {
    pub name: String,
    pub usage_count: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CaptureOutcome {
    InProgress,
    Success,
    Failed,
    Tunneled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorStage {
    Connect,
    TlsHandshake,
    RequestBody,
    Interceptor,
    Upstream,
    ResponseBody,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CaptureError {
    pub stage: ErrorStage,
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestData {
    pub method: String,
    pub uri: String,
    pub version: String,
    pub headers: HeaderValues,
    #[serde(default)]
    pub tags: RequestTags,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResponseData {
    pub status: u16,
    pub version: String,
    pub headers: HeaderValues,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BodyPayload {
    Empty,
    Text {
        content: String,
        size: u64,
        path: Option<String>,
    },
    Json {
        content: serde_json::Value,
        size: u64,
        path: Option<String>,
    },
    Binary {
        size: u64,
        path: Option<String>,
    },
    Large {
        size: u64,
        path: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Modification {
    Snapshot { headers: HeaderValues },
    MethodSet { method: String },
    UriSet { uri: String },
    StatusSet { status: u16 },
    HeaderAppend { name: String, value: String },
    HeaderSet { name: String, value: String },
    HeaderRemove { name: String, values: Vec<String> },
    BodyReplaceString { content: String },
    BodyReplaceFile { path: String },
    BodyReplaceAsset { asset_id: String },
    TagSet { key: String, value: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InterceptorExecutionOrigin {
    Saved,
    Temporary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InterceptorExecution {
    pub execution_id: u64,
    pub origin: InterceptorExecutionOrigin,
    pub completed: bool,
    pub phase: InterceptorKind,
    pub position: usize,
    pub name: String,
    pub script_hash: String,
    pub content: String,
    pub modifications: Vec<Modification>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterceptorRun {
    pub origin: InterceptorExecutionOrigin,
    pub completed: bool,
    pub phase: InterceptorKind,
    pub position: usize,
    pub name: String,
    pub script_hash: String,
    pub content: String,
    pub modifications: Vec<Modification>,
    pub error: Option<String>,
}

pub fn script_content_hash(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CaptureSummary {
    pub id: u64,
    pub session_id: u64,
    pub source: String,
    pub request: RequestData,
    pub response: Option<ResponseData>,
    pub outcome: CaptureOutcome,
    pub stage: String,
    pub error: Option<CaptureError>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CaptureDetail {
    #[serde(flatten)]
    pub summary: CaptureSummary,
    pub request_body: BodyPayload,
    pub response_body: BodyPayload,
    pub request_interceptors: Vec<InterceptorExecution>,
    pub response_interceptors: Vec<InterceptorExecution>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BreakpointSummary {
    pub id: u64,
    pub session_id: u64,
    pub capture_id: u64,
    pub phase: InterceptorKind,
    pub position: usize,
    pub interceptor_name: String,
    pub method: String,
    pub uri: String,
    pub created_at: u64,
    pub expires_at: u64,
    pub remaining_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakpointListFilter {
    pub session_id: u64,
    pub phase: Option<InterceptorKind>,
    pub interceptor_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BreakpointDetail {
    pub breakpoint: BreakpointSummary,
    pub capture: CaptureDetail,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TemporaryExecutionResult {
    pub execution: InterceptorExecution,
    pub breakpoint: BreakpointSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ProxyStatus {
    Stopped,
    Starting,
    Running {
        host: String,
        port: u16,
        started_at: u64,
    },
    Stopping,
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SystemLogEntry {
    pub seq: u64,
    pub timestamp: u64,
    pub level: String,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::{AppConfig, FilterColumn, FilterOption};

    #[test]
    fn legacy_api_host_is_ignored_and_not_serialized() {
        let config: AppConfig = serde_json::from_str(
            r#"{"proxy_host":"0.0.0.0","proxy_port":8089,"api_host":"127.0.0.1","api_port":18089,"routing_script_name":null,"active_session_id":null}"#,
        )
        .unwrap();
        let value = serde_json::to_value(config).unwrap();
        assert_eq!(value["api_port"], 18089);
        assert!(value.get("api_host").is_none());
    }

    #[test]
    fn obsolete_case_sensitive_field_is_ignored() {
        let option: FilterOption = serde_json::from_str(
            r#"{"kind":"column","column":{"kind":"uri"},"case_sensitive":false}"#,
        )
        .unwrap();

        assert_eq!(
            option,
            FilterOption::Column {
                column: FilterColumn::Uri,
                regex: false,
            }
        );
    }
}
