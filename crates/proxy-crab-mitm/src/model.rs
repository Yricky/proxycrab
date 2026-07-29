use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub type HeaderValues = BTreeMap<String, Vec<String>>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppConfig {
    #[serde(default = "default_proxy_host")]
    pub proxy_host: String,
    #[serde(default = "default_proxy_port")]
    pub proxy_port: u16,
    #[serde(default = "default_api_host")]
    pub api_host: String,
    #[serde(default = "default_api_port")]
    pub api_port: u16,
    #[serde(default)]
    pub active_session_id: Option<u64>,
    #[serde(default)]
    pub active_request_interceptors: Vec<String>,
    #[serde(default)]
    pub active_response_interceptors: Vec<String>,
    #[serde(default = "default_columns")]
    pub columns: Vec<Column>,
    #[serde(default)]
    pub filter_history: Vec<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            proxy_host: default_proxy_host(),
            proxy_port: default_proxy_port(),
            api_host: default_api_host(),
            api_port: default_api_port(),
            active_session_id: None,
            active_request_interceptors: Vec::new(),
            active_response_interceptors: Vec::new(),
            columns: default_columns(),
            filter_history: Vec::new(),
        }
    }
}

fn default_proxy_host() -> String {
    "0.0.0.0".into()
}

const fn default_proxy_port() -> u16 {
    8089
}

fn default_api_host() -> String {
    "127.0.0.1".into()
}

const fn default_api_port() -> u16 {
    18089
}

fn default_columns() -> Vec<Column> {
    vec![
        Column::Method { width: 50.0 },
        Column::Uri { width: 300.0 },
        Column::Code { width: 50.0 },
        Column::Source { width: 100.0 },
    ]
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
pub struct InterceptorInfo {
    pub name: String,
    pub enabled: bool,
    pub order: Option<usize>,
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
    Text { content: String },
    Json { content: serde_json::Value },
    Binary { size: u64 },
    Large { size: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum Modification {
    #[serde(rename = "snapshot")]
    Snapshot { headers: HeaderValues },
    #[serde(rename = "header_append")]
    HeaderAppend {
        #[serde(rename = "key")]
        name: String,
        value: String,
    },
    #[serde(rename = "header_set")]
    HeaderSet {
        #[serde(rename = "key")]
        name: String,
        value: String,
    },
    #[serde(rename = "header_remove")]
    HeaderRemove {
        #[serde(rename = "key")]
        name: String,
        #[serde(rename = "removed_values")]
        values: Vec<String>,
    },
    #[serde(rename = "body_replace_str")]
    BodyReplaceString { content: String },
    #[serde(rename = "body_replace_file")]
    BodyReplaceFile { path: String },
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
    pub request_modifications: Vec<Modification>,
    pub response_modifications: Vec<Modification>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ProxyStatus {
    Stopped,
    Starting,
    Running { host: String, port: u16 },
    Stopping,
    Failed { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SystemLogEntry {
    pub seq: u64,
    pub timestamp: u64,
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspacePaths {
    pub current_path: String,
    pub configured_path: String,
}
