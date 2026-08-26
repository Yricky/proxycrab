use std::collections::BTreeMap;

use proxy_crab_mitm::{
    bypass::BypassEntry,
    model::{
        AppConfig, BodyPayload, BreakpointSummary, CaptureError, Column, InterceptorExecution,
        InterceptorKind, InterceptorLibraryItem, ProxyStatus, Script, SessionFilter,
        SessionMetadata, SystemLogEntry,
    },
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagerError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_size: Option<u64>,
}

impl std::fmt::Display for ManagerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ManagerError {}

impl ManagerError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            actual_size: None,
            max_size: None,
        }
    }

    pub fn body_too_large(actual_size: u64, max_size: u64) -> Self {
        Self {
            code: "body_too_large".into(),
            message: format!("body size {actual_size} exceeds the requested {max_size} byte limit"),
            actual_size: Some(actual_size),
            max_size: Some(max_size),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new("bad_request", message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new("not_found", message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new("conflict", message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("internal_error", message)
    }
}

pub type ManagerResult<T> = Result<T, ManagerError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetWorkspaceRequest {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSessionRequest {
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSessionRequest {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAgentsPresetRequest {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAgentsPresetRequest {
    pub name: Option<String>,
    pub content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentsPreset {
    pub id: String,
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentsPresetState {
    pub active_id: String,
    pub presets: Vec<AgentsPreset>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActiveSession {
    pub session_id: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionQuery {
    pub session_id: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BodyQuery {
    pub session_id: Option<u64>,
    pub side: String,
    pub max_size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AssetQuery {
    pub format: Option<String>,
}

pub const fn default_body_max_size() -> u64 {
    16 * 1024 * 1024
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogIdsRequest {
    pub session_id: Option<u64>,
    pub filter: Option<SessionFilter>,
    pub ids: Option<Vec<u64>>,
    pub min_id: Option<u64>,
    pub max_id: Option<u64>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogIdsPayload {
    pub matched_ids: Vec<u64>,
    pub in_progress_ids: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogViewItem {
    pub id: u64,
    pub updated_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionViewInput {
    pub columns: Vec<Column>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogViewsRequest {
    pub session_id: Option<u64>,
    pub logs: Vec<LogViewItem>,
    pub view: Option<SessionViewInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogViewRow {
    pub id: u64,
    pub created_at: u64,
    pub updated_at: u64,
    pub outcome: String,
    pub cells: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogViewException {
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column_index: Option<usize>,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogViewsPayload {
    pub columns: Vec<Column>,
    pub rows: Vec<LogViewRow>,
    pub exceptions: Vec<LogViewException>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportLogsRequest {
    pub format: String,
    pub session_id: Option<u64>,
    pub log_ids: Option<Vec<u64>>,
}

#[derive(Debug)]
pub struct LogExport {
    pub session_id: u64,
    pub filename: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionViewPayload {
    pub session_id: u64,
    pub columns: Vec<Column>,
    pub filter: SessionFilter,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplaceSessionViewRequest {
    pub columns: Vec<Column>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaderItem {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestDetail {
    pub method: String,
    pub uri: String,
    pub version: String,
    pub headers: Vec<HeaderItem>,
    pub tags: BTreeMap<String, String>,
    pub body: BodyPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseDetail {
    pub status: u16,
    pub status_text: String,
    pub version: String,
    pub headers: Vec<HeaderItem>,
    pub body: BodyPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogDetail {
    pub id: u64,
    pub session_id: u64,
    pub created_at: u64,
    pub updated_at: u64,
    pub source_type: String,
    pub source_addr: Option<String>,
    pub stage: String,
    pub outcome: String,
    pub error: Option<CaptureError>,
    pub request: RequestDetail,
    pub response: Option<ResponseDetail>,
    pub request_interceptors: Vec<InterceptorExecution>,
    pub response_interceptors: Vec<InterceptorExecution>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakpointQuery {
    pub session_id: Option<u64>,
    pub phase: Option<InterceptorKind>,
    pub interceptor_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakpointDetailPayload {
    pub breakpoint: BreakpointSummary,
    pub log: LogDetail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtendBreakpointRequest {
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecuteTemporaryScriptRequest {
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptRequest {
    pub name: String,
    #[serde(default)]
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateScriptRequest {
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugFilterScriptRequest {
    pub session_id: Option<u64>,
    pub log_id: u64,
    pub input: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterceptorCreateRequest {
    pub kind: InterceptorKind,
    pub name: String,
    #[serde(default)]
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InterceptorUpdateRequest {
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingSelection {
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BypassQuery {
    pub before_id: Option<u64>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BypassPage {
    pub rows: Vec<BypassEntry>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteBypassRequest {
    pub ids: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteCount {
    pub deleted: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterceptorLibraryList {
    pub kind: InterceptorKind,
    pub items: Vec<InterceptorLibraryItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterceptorDetail {
    pub kind: InterceptorKind,
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInterceptorInput {
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplaceSessionInterceptorsRequest {
    pub request: Vec<SessionInterceptorInput>,
    pub response: Vec<SessionInterceptorInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInterceptorItem {
    pub name: String,
    pub enabled: bool,
    pub valid: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInterceptorsPayload {
    pub session_id: u64,
    pub request: Vec<SessionInterceptorItem>,
    pub response: Vec<SessionInterceptorItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemLogsQuery {
    pub after_seq: Option<u64>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertificateResponse {
    pub pem: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpServiceStatus {
    pub running: bool,
    pub host: String,
    pub port: u16,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HttpApiResource {
    Workspace,
    Config,
    Proxy,
    Sessions,
    ArchivedSessions,
    ActiveSession,
    SessionView,
    ColumnScripts,
    FilterScripts,
    RoutingScripts,
    RoutingSelection,
    Bypass,
    Interceptors,
    SessionInterceptors,
    Certificate,
    SystemLogs,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HttpApiChange {
    pub resources: Vec<HttpApiResource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<u64>,
}

pub use proxy_crab_mitm::model::{
    AppConfig as Config, Column as NetLogColumn, InterceptorKind as Kind,
    ProxyStatus as MitmStatus, SessionMetadata as Session,
};

pub type AgentsPresetsResponse = AgentsPresetState;
pub type ConfigResponse = AppConfig;
pub type ProxyStatusResponse = ProxyStatus;
pub type SessionsResponse = Vec<SessionMetadata>;
pub type ScriptsResponse = Vec<Script>;
pub type SystemLogsResponse = Vec<SystemLogEntry>;
