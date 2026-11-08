use std::collections::BTreeMap;

use proxy_crab_mitm::{
    bypass::BypassEntry,
    model::{
        AppConfig, BodyPayload, BreakpointSummary, CaptureError, CaptureOutcome, Column,
        InterceptorExecution, InterceptorKind, InterceptorLibraryItem, ProxyStatus, Script,
        SessionFilter, SessionInterceptors, SessionMetadata, SessionView, SystemLogEntry,
    },
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ErrorCode {
    RedactionFailed,
    ShareRedactionForbidden,
    MessageContentUnavailable,
    MessageContentHidden,
    RedactionManagementDisabled,
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
            Self::RedactionManagementDisabled => "redaction_management_disabled",
            Self::RedactionFailed => "redaction_failed",
            Self::ShareRedactionForbidden => "share_redaction_forbidden",
            Self::MessageContentUnavailable => "message_content_unavailable",
            Self::MessageContentHidden => "message_content_hidden",
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct ManagerError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub actual_size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CreateSessionRequest {
    #[ts(optional = nullable)]
    pub name: Option<String>,
    #[ts(optional = nullable)]
    pub description: Option<String>,
    #[ts(optional = nullable)]
    pub view: Option<SessionView>,
    #[ts(optional = nullable)]
    pub interceptors: Option<SessionInterceptors>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UpdateSessionRequest {
    #[ts(optional = nullable)]
    pub name: Option<String>,
    #[ts(optional = nullable)]
    pub description: Option<Option<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CreateAgentsPresetRequest {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UpdateAgentsPresetRequest {
    #[ts(optional = nullable)]
    pub name: Option<String>,
    #[ts(optional = nullable)]
    pub content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AgentsPreset {
    pub id: String,
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AgentsPresetState {
    pub active_id: String,
    pub presets: Vec<AgentsPreset>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct ActiveSession {
    pub session_id: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SessionsPayload {
    pub sessions: Vec<SessionMetadata>,
    pub active_session_id: Option<u64>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketQuery {
    pub session_id: Option<u64>,
    pub side: String,
    #[serde(default)]
    pub after_id: u32,
    pub limit: Option<usize>,
    pub max_size: Option<u64>,
}
impl WebSocketQuery {
    pub fn body_side(&self) -> ManagerResult<proxy_crab_mitm::storage::BodySide> {
        match self.side.as_str() {
            "request" => Ok(proxy_crab_mitm::storage::BodySide::Request),
            "response" => Ok(proxy_crab_mitm::storage::BodySide::Response),
            _ => Err(ManagerError::bad_request(
                "side must be request or response",
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AssetQuery {
    pub format: Option<String>,
}

pub const fn default_body_max_size() -> u64 {
    16 * 1024 * 1024
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LogIdsRequest {
    #[ts(optional = nullable)]
    pub session_id: Option<u64>,
    #[ts(optional = nullable)]
    pub filter: Option<SessionFilter>,
    #[ts(optional = nullable)]
    pub ids: Option<Vec<u64>>,
    #[ts(optional = nullable)]
    pub min_id: Option<u64>,
    #[ts(optional = nullable)]
    pub max_id: Option<u64>,
    #[ts(optional = nullable)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LogIdsPayload {
    pub matched_ids: Vec<u64>,
    pub in_progress_ids: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LogViewItem {
    pub id: u64,
    #[ts(optional = nullable)]
    pub updated_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SessionViewInput {
    pub columns: Vec<Column>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LogViewsRequest {
    #[ts(optional = nullable)]
    pub session_id: Option<u64>,
    pub logs: Vec<LogViewItem>,
    #[ts(optional = nullable)]
    pub view: Option<SessionViewInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LogViewRow {
    pub id: u64,
    pub created_at: u64,
    pub updated_at: u64,
    pub outcome: CaptureOutcome,
    pub cells: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LogViewException {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub column_index: Option<usize>,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LogViewsPayload {
    pub columns: Vec<Column>,
    pub rows: Vec<LogViewRow>,
    pub exceptions: Vec<LogViewException>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SessionViewPayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub redaction_revision: Option<String>,
    pub session_id: u64,
    pub columns: Vec<Column>,
    pub filter: SessionFilter,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReplaceSessionViewRequest {
    pub columns: Vec<Column>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HeaderItem {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RequestDetail {
    pub method: String,
    pub uri: String,
    pub version: String,
    pub headers: Vec<HeaderItem>,
    pub tags: BTreeMap<String, String>,
    pub body: BodyPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ResponseDetail {
    pub status: u16,
    pub status_text: String,
    pub version: String,
    pub headers: Vec<HeaderItem>,
    pub body: BodyPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LogDetail {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub websocket: Option<proxy_crab_mitm::websocket::WebSocketInfo>,
    pub id: u64,
    pub session_id: u64,
    pub created_at: u64,
    pub updated_at: u64,
    pub source_type: String,
    pub source_addr: Option<String>,
    pub stage: String,
    pub outcome: CaptureOutcome,
    pub error: Option<CaptureError>,
    pub request: RequestDetail,
    pub response: Option<ResponseDetail>,
    pub request_interceptors: Vec<InterceptorExecution>,
    pub response_interceptors: Vec<InterceptorExecution>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BreakpointQuery {
    #[ts(optional = nullable)]
    pub session_id: Option<u64>,
    #[ts(optional = nullable)]
    pub phase: Option<InterceptorKind>,
    #[ts(optional = nullable)]
    pub interceptor_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BreakpointDetailPayload {
    pub breakpoint: BreakpointSummary,
    pub log: LogDetail,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ExtendBreakpointRequest {
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ExecuteTemporaryScriptRequest {
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ScriptRequest {
    pub name: String,
    #[serde(default)]
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct UpdateScriptRequest {
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DebugFilterScriptRequest {
    #[ts(optional = nullable)]
    pub session_id: Option<u64>,
    pub log_id: u64,
    pub input: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InterceptorCreateRequest {
    pub kind: InterceptorKind,
    pub name: String,
    #[serde(default)]
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct InterceptorUpdateRequest {
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RoutingSelection {
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BypassQuery {
    #[ts(optional = nullable)]
    pub before_id: Option<u64>,
    #[ts(optional = nullable)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BypassPage {
    pub rows: Vec<BypassEntry>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteBypassRequest {
    pub ids: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DeleteCount {
    pub deleted: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InterceptorLibraryList {
    pub kind: InterceptorKind,
    pub items: Vec<InterceptorLibraryItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InterceptorDetail {
    pub kind: InterceptorKind,
    pub name: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SessionInterceptorInput {
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReplaceSessionInterceptorsRequest {
    pub request: Vec<SessionInterceptorInput>,
    pub response: Vec<SessionInterceptorInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SessionInterceptorItem {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub display_name: Option<String>,
    pub enabled: bool,
    pub valid: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SessionInterceptorsPayload {
    pub session_id: u64,
    pub request: Vec<SessionInterceptorItem>,
    pub response: Vec<SessionInterceptorItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SystemLogsQuery {
    #[ts(optional = nullable)]
    pub after_seq: Option<u64>,
    #[ts(optional = nullable)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CertificateResponse {
    pub pem: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HttpServiceStatus {
    pub running: bool,
    pub host: String,
    pub port: u16,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum HttpApiResource {
    Workspace,
    Config,
    Proxy,
    Sessions,
    ArchivedSessions,
    ActiveSession,
    SessionShare,
    SessionView,
    ColumnScripts,
    FilterScripts,
    RoutingScripts,
    RoutingSelection,
    Bypass,
    Interceptors,
    SessionInterceptors,
    Certificate,
    ClashSubscription,
    ClashConfig,
    SystemLogs,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct HttpApiChange {
    pub resources: Vec<HttpApiResource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct ReplayRequestPayload {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    #[ts(optional)]
    pub body: Option<ReplayBodyPayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct ReplayResult {
    pub log_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedactionBinding {
    pub redaction_script: Option<String>,
}
