use std::net::SocketAddr;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    Allow,
    Approval,
    Deny,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct ApiAction {
    pub id: &'static str,
    pub method: &'static str,
    pub route_template: &'static str,
    pub default_mode: PermissionMode,
}

pub struct PermissionAction {
    pub action: &'static ApiAction,
    pub credential: ManagementCredential,
    pub actual_path: String,
    pub query: Option<String>,
    pub source: Option<SocketAddr>,
    pub content_type: Option<String>,
    pub content_length: Option<u64>,
    pub body_preview: Option<String>,
    pub body_preview_truncated: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub enum ManagementCredential {
    LocalLoopback,
    Bearer(String),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDeniedStatus {
    Unauthorized,
    Forbidden,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PermissionDenied {
    pub status: PermissionDeniedStatus,
    pub code: String,
    pub message: String,
}

impl PermissionDenied {
    pub fn unauthorized(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: PermissionDeniedStatus::Unauthorized,
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn forbidden(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: PermissionDeniedStatus::Forbidden,
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn internal(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: PermissionDeniedStatus::Internal,
            code: code.into(),
            message: message.into(),
        }
    }
}

#[async_trait]
pub trait PermissionManager: Send + Sync {
    async fn check_permission(&self, action: PermissionAction) -> Option<PermissionDenied>;
}

macro_rules! action {
    ($method:literal, $route:literal, $mode:ident) => {
        ApiAction {
            id: concat!($method, " ", $route),
            method: $method,
            route_template: $route,
            default_mode: PermissionMode::$mode,
        }
    };
}

pub static API_ACTIONS: &[ApiAction] = &[
    action!("GET", "/api/agents.md", Allow),
    action!("GET", "/api/assets/{*asset_id}", Allow),
    action!("POST", "/api/assets/{*asset_id}", Approval),
    action!("GET", "/api/config", Allow),
    action!("GET", "/api/proxy/status", Allow),
    action!("POST", "/api/proxy/start", Approval),
    action!("POST", "/api/proxy/stop", Approval),
    action!("GET", "/api/sessions", Allow),
    action!("POST", "/api/sessions", Approval),
    action!("GET", "/api/archived-sessions", Allow),
    action!("POST", "/api/sessions/{id}/archive", Deny),
    action!("POST", "/api/archived-sessions/{id}/restore", Approval),
    action!("DELETE", "/api/archived-sessions/{id}", Deny),
    action!("GET", "/api/active-session", Allow),
    action!("PUT", "/api/active-session", Approval),
    action!("PUT", "/api/sessions/{id}", Approval),
    action!("PUT", "/api/sessions/{id}/filter", Approval),
    action!("GET", "/api/session-shares/{id}", Allow),
    action!("POST", "/api/session-shares", Approval),
    action!("DELETE", "/api/session-shares/{id}", Approval),
    action!("GET", "/api/session-har-shares/{id}", Allow),
    action!("POST", "/api/session-har-shares", Approval),
    action!("DELETE", "/api/session-har-shares/{id}", Approval),
    action!("POST", "/api/logs/ids", Allow),
    action!("POST", "/api/logs/views", Allow),
    action!("GET", "/api/logs/{id}/body", Allow),
    action!("GET", "/api/logs/{id}", Allow),
    action!("GET", "/api/session-view", Allow),
    action!("PUT", "/api/session-view", Approval),
    action!("GET", "/api/session-interceptors", Allow),
    action!("PUT", "/api/session-interceptors", Approval),
    action!("GET", "/api/column-scripts", Allow),
    action!("POST", "/api/column-scripts", Approval),
    action!("GET", "/api/column-scripts/{name}", Allow),
    action!("PUT", "/api/column-scripts/{name}", Approval),
    action!("DELETE", "/api/column-scripts/{name}", Deny),
    action!("GET", "/api/filter-scripts", Allow),
    action!("POST", "/api/filter-scripts", Approval),
    action!("POST", "/api/filter-scripts/{name}/debug", Allow),
    action!("GET", "/api/filter-scripts/{name}", Allow),
    action!("PUT", "/api/filter-scripts/{name}", Approval),
    action!("DELETE", "/api/filter-scripts/{name}", Deny),
    action!("GET", "/api/routing-scripts", Allow),
    action!("POST", "/api/routing-scripts", Approval),
    action!("GET", "/api/routing-scripts/{name}", Allow),
    action!("PUT", "/api/routing-scripts/{name}", Approval),
    action!("DELETE", "/api/routing-scripts/{name}", Deny),
    action!("GET", "/api/routing-script-selection", Allow),
    action!("PUT", "/api/routing-script-selection", Approval),
    action!("GET", "/api/interceptors", Allow),
    action!("POST", "/api/interceptors", Approval),
    action!("GET", "/api/interceptors/{kind}/{name}", Allow),
    action!("PUT", "/api/interceptors/{kind}/{name}", Approval),
    action!("DELETE", "/api/interceptors/{kind}/{name}", Deny),
    action!("GET", "/api/breakpoints", Allow),
    action!("GET", "/api/breakpoints/{id}/body", Allow),
    action!("GET", "/api/breakpoints/{id}", Allow),
    action!("POST", "/api/breakpoints/{id}/extend", Approval),
    action!("POST", "/api/breakpoints/{id}/release", Approval),
    action!("POST", "/api/breakpoints/{id}/execute", Approval),
    action!("GET", "/api/bypass", Allow),
    action!("DELETE", "/api/bypass", Deny),
    action!("POST", "/api/bypass/delete", Deny),
    action!("DELETE", "/api/bypass/{id}", Deny),
    action!("GET", "/api/ca", Deny),
    action!("GET", "/api/system-logs", Allow),
    action!("DELETE", "/api/system-logs", Deny),
];

pub const OBSOLETE_API_ACTION_IDS: &[&str] = &[
    "GET /api/workspace",
    "PUT /api/workspace",
    "PUT /api/config",
    "POST /api/ca",
    "POST /api/logs/export",
];

pub fn api_actions() -> &'static [ApiAction] {
    API_ACTIONS
}

pub fn find_api_action(method: &str, route_template: &str) -> Option<&'static ApiAction> {
    API_ACTIONS
        .iter()
        .find(|action| action.method == method && action.route_template == route_template)
}

pub fn api_actions_for_path(path: &str) -> impl Iterator<Item = &'static ApiAction> + '_ {
    API_ACTIONS
        .iter()
        .filter(move |action| route_template_matches(action.route_template, path))
}

fn route_template_matches(template: &str, path: &str) -> bool {
    let mut template = template.trim_matches('/').split('/');
    let mut path = path.trim_matches('/').split('/');
    loop {
        match (template.next(), path.next()) {
            (None, None) => return true,
            (Some(expected), Some(_)) if expected.starts_with("{*") && expected.ends_with('}') => {
                return true;
            }
            (Some(expected), Some(actual))
                if !actual.is_empty()
                    && (expected == actual
                        || (expected.starts_with('{') && expected.ends_with('}'))) => {}
            _ => return false,
        }
    }
}
