use std::{collections::BTreeMap, pin::Pin};

use bytes::Bytes;
use futures::Stream;
use proxy_crab_mitm::{
    bypass::BypassEntry,
    model::{
        AppConfig, BodyPayload, BreakpointSummary, CaptureError, Column, InterceptorExecution,
        InterceptorKind, InterceptorLibraryItem, ProxyStatus, Script, SessionFilter,
        SessionMetadata, SystemLogEntry,
    },
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    BadRequest,
    NotFound,
    LogNotFound,
    BodyNotFound,
    AssetNotFound,
    ExecutionNotFound,
    SnapshotNotFound,
    SnapshotBodyNotFound,
    Conflict,
    ProxyRunning,
    ProxyNotRunning,
    SessionInUse,
    AssetAlreadyExists,
    AssetPathConflict,
    InvalidAssetId,
    InvalidAssetFormat,
    UnsupportedExportFormat,
    NotAcceptableEncoding,
    BodyTooLarge,
    BodyDecodeFailed,
    InternalError,
    BodyReadFailed,
    AssetStoreFailed,
    ReplayFailed,
    InvalidApiKey,
    InvalidUiToken,
    ShareSessionUnavailable,
}

impl ErrorCode {
    /// The stable snake_case wire representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BadRequest => "bad_request",
            Self::NotFound => "not_found",
            Self::LogNotFound => "log_not_found",
            Self::BodyNotFound => "body_not_found",
            Self::AssetNotFound => "asset_not_found",
            Self::ExecutionNotFound => "execution_not_found",
            Self::SnapshotNotFound => "snapshot_not_found",
            Self::SnapshotBodyNotFound => "snapshot_body_not_found",
            Self::Conflict => "conflict",
            Self::ProxyRunning => "proxy_running",
            Self::ProxyNotRunning => "proxy_not_running",
            Self::SessionInUse => "session_in_use",
            Self::AssetAlreadyExists => "asset_already_exists",
            Self::AssetPathConflict => "asset_path_conflict",
            Self::InvalidAssetId => "invalid_asset_id",
            Self::InvalidAssetFormat => "invalid_asset_format",
            Self::UnsupportedExportFormat => "unsupported_export_format",
            Self::NotAcceptableEncoding => "not_acceptable_encoding",
            Self::BodyTooLarge => "body_too_large",
            Self::BodyDecodeFailed => "body_decode_failed",
            Self::InternalError => "internal_error",
            Self::BodyReadFailed => "body_read_failed",
            Self::AssetStoreFailed => "asset_store_failed",
            Self::ReplayFailed => "replay_failed",
            Self::InvalidApiKey => "invalid_api_key",
            Self::InvalidUiToken => "invalid_ui_token",
            Self::ShareSessionUnavailable => "share_session_unavailable",
        }
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagerError {
    pub code: ErrorCode,
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
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            actual_size: None,
            max_size: None,
        }
    }

    pub fn body_too_large(actual_size: u64, max_size: u64) -> Self {
        Self {
            code: ErrorCode::BodyTooLarge,
            message: format!("body size {actual_size} exceeds the requested {max_size} byte limit"),
            actual_size: Some(actual_size),
            max_size: Some(max_size),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::BadRequest, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Conflict, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InternalError, message)
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
    pub execution_id: Option<u64>,
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

pub struct LogExport {
    pub session_id: u64,
    pub filename: String,
    pub body: Pin<Box<dyn Stream<Item = ManagerResult<Bytes>> + Send>>,
}

impl std::fmt::Debug for LogExport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LogExport")
            .field("session_id", &self.session_id)
            .field("filename", &self.filename)
            .field("body", &"<stream>")
            .finish()
    }
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
    SessionShare,
    SessionHarShare,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayRequestPayload {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub body: Option<ReplayBodyPayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReplayBodyPayload {
    Text {
        text: String,
        #[serde(default)]
        charset: Option<String>,
    },
    BodyRef {
        session_id: u64,
        log_id: u64,
        side: String,
    },
    Asset {
        asset_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayResult {
    pub log_id: u64,
}
