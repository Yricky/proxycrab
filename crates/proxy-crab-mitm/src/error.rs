//! 面向管理层的类型化错误。
//!
//! 管理层据此精确映射 HTTP 状态码，替代按消息文本猜测分类。

/// Typed errors surfaced to the management layer.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("session {0} not found")]
    SessionNotFound(u64),
    #[error("session {0} already exists")]
    SessionAlreadyExists(u64),
    #[error("active session {0} cannot be archived")]
    ActiveSessionArchive(u64),
    #[error("session {0} has requests in progress")]
    SessionRequestsInProgress(u64),
    #[error("archived session {0} not found")]
    ArchivedSessionNotFound(u64),
    #[error("archived session {0} already exists")]
    ArchivedSessionAlreadyExists(u64),
    #[error("script {0} not found")]
    ScriptNotFound(String),
    #[error("script {0} already exists")]
    ScriptAlreadyExists(String),
    #[error("invalid script name")]
    InvalidScriptName,
    #[error("invalid script: {0}")]
    InvalidScript(String),
    #[error("capture {0} not found")]
    CaptureNotFound(u64),
    #[error("breakpoint {0} not found")]
    BreakpointNotFound(u64),
    #[error("bypass entry {0} not found")]
    BypassEntryNotFound(u64),
    #[error("bypass entry {0} is still in progress")]
    BypassEntryInProgress(u64),
    #[error("proxy is already running or changing state")]
    ProxyAlreadyRunning,
    #[error("proxy must be stopped before regenerating the CA")]
    ProxyMustBeStopped,
    #[error("{0}")]
    InvalidArgument(String),
}
