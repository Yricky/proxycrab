use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, Request, State},
    http::{
        HeaderValue, StatusCode,
        header::{AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE, REFERRER_POLICY},
    },
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use proxy_crab_mgr::{
    ProxyCrabManager,
    dto::{CreateAgentsPresetRequest, HttpApiChange, ManagerError, UpdateAgentsPresetRequest},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::{Notify, broadcast};

use crate::permissions::{CliPermissionService, PermissionEntry, UiAccess};

include!(concat!(env!("OUT_DIR"), "/ui_assets.rs"));

struct ChangeLog {
    cursor: AtomicU64,
    entries: Mutex<VecDeque<(u64, HttpApiChange)>>,
    notify: Notify,
}

impl ChangeLog {
    fn new(mut receiver: broadcast::Receiver<HttpApiChange>) -> Arc<Self> {
        let value = Arc::new(Self {
            cursor: AtomicU64::new(0),
            entries: Mutex::new(VecDeque::new()),
            notify: Notify::new(),
        });
        let target = value.clone();
        tokio::spawn(async move {
            loop {
                match receiver.recv().await {
                    Ok(change) => {
                        let cursor = target.cursor.fetch_add(1, Ordering::SeqCst) + 1;
                        let mut entries = target.entries.lock().expect("change log lock poisoned");
                        entries.push_back((cursor, change));
                        while entries.len() > 256 {
                            entries.pop_front();
                        }
                        drop(entries);
                        target.notify.notify_waiters();
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        value
    }

    fn after(&self, cursor: u64) -> Vec<HttpApiChange> {
        self.entries
            .lock()
            .expect("change log lock poisoned")
            .iter()
            .filter(|(entry_cursor, _)| *entry_cursor > cursor)
            .map(|(_, change)| change.clone())
            .collect()
    }
}

#[derive(Clone)]
struct UiState {
    manager: Arc<dyn ProxyCrabManager>,
    permissions: Arc<CliPermissionService>,
    access: Arc<UiAccess>,
    changes: Arc<ChangeLog>,
}

#[derive(Deserialize)]
struct PermissionUpdate {
    permissions: Vec<PermissionEntry>,
}

#[derive(Deserialize)]
struct CreateKey {
    name: String,
}

#[derive(Deserialize)]
struct RegexRequest {
    pattern: String,
}

#[derive(Deserialize)]
struct ChangeQuery {
    #[serde(default)]
    after: u64,
}

#[derive(Serialize)]
struct ChangePayload {
    cursor: u64,
    changes: Vec<HttpApiChange>,
}

pub fn router(
    manager: Arc<dyn ProxyCrabManager>,
    permissions: Arc<CliPermissionService>,
    access: Arc<UiAccess>,
    changes: broadcast::Sender<HttpApiChange>,
) -> Router {
    let state = Arc::new(UiState {
        manager,
        permissions,
        access,
        changes: ChangeLog::new(changes.subscribe()),
    });
    let protected = Router::new()
        .route("/ui-api/bootstrap", get(bootstrap))
        .route("/ui-api/changes", get(changes_after))
        .route("/ui-api/local-ips", get(local_ips))
        .route("/ui-api/validate-filter-regex", post(validate_filter_regex))
        .route(
            "/ui-api/agents-presets",
            get(agents_presets).post(create_agents_preset),
        )
        .route(
            "/ui-api/agents-presets/reimport-defaults",
            post(reimport_agents_presets),
        )
        .route(
            "/ui-api/agents-presets/{id}/activate",
            post(activate_agents_preset),
        )
        .route(
            "/ui-api/agents-presets/{id}",
            put(update_agents_preset).delete(delete_agents_preset),
        )
        .route("/ui-api/permissions/catalog", get(permission_catalog))
        .route("/ui-api/permissions/identities", get(permission_identities))
        .route(
            "/ui-api/permissions/identities/{id}",
            get(identity_permissions).put(replace_permissions),
        )
        .route("/ui-api/api-keys", post(create_api_key))
        .route("/ui-api/api-keys/{id}", delete(delete_api_key))
        .route_layer(middleware::from_fn_with_state(state.clone(), authorize_ui))
        .with_state(state);
    protected.merge(
        Router::new()
            .route("/", get(index))
            .route("/{*path}", get(asset)),
    )
}

async fn authorize_ui(State(state): State<Arc<UiState>>, request: Request, next: Next) -> Response {
    let allowed = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| state.access.allows_authorization(Some(value)));
    if allowed {
        next.run(request).await
    } else {
        error_response(
            StatusCode::UNAUTHORIZED,
            ManagerError::new("invalid_ui_token", "Access Token 无效"),
        )
    }
}

async fn bootstrap(State(state): State<Arc<UiState>>) -> Response {
    match state.manager.config().await {
        Ok(config) => success(json!({
            "target": "cli",
            "http_service": {
                "running": true,
                "host": std::net::Ipv4Addr::UNSPECIFIED.to_string(),
                "port": config.api_port,
                "error": null,
            }
        })),
        Err(error) => manager_error(error),
    }
}

async fn changes_after(
    State(state): State<Arc<UiState>>,
    Query(query): Query<ChangeQuery>,
) -> Response {
    let mut values = state.changes.after(query.after);
    if values.is_empty() {
        let _ =
            tokio::time::timeout(Duration::from_secs(25), state.changes.notify.notified()).await;
        values = state.changes.after(query.after);
    }
    success(ChangePayload {
        cursor: state.changes.cursor.load(Ordering::SeqCst),
        changes: values,
    })
}

async fn local_ips() -> Response {
    let mut ips = if_addrs::get_if_addrs()
        .map(|interfaces| {
            interfaces
                .into_iter()
                .map(|interface| interface.addr.ip())
                .filter(|ip| ip.is_ipv4())
                .map(|ip| ip.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    ips.sort();
    ips.dedup();
    success(ips)
}

async fn validate_filter_regex(Json(request): Json<RegexRequest>) -> Response {
    success(
        regex::Regex::new(&request.pattern)
            .err()
            .map(|error| error.to_string()),
    )
}

async fn agents_presets(State(state): State<Arc<UiState>>) -> Response {
    result(state.manager.agents_presets().await)
}

async fn create_agents_preset(
    State(state): State<Arc<UiState>>,
    Json(request): Json<CreateAgentsPresetRequest>,
) -> Response {
    result(state.manager.create_agents_preset(request).await)
}

async fn update_agents_preset(
    State(state): State<Arc<UiState>>,
    Path(id): Path<String>,
    Json(request): Json<UpdateAgentsPresetRequest>,
) -> Response {
    result(state.manager.update_agents_preset(id, request).await)
}

async fn activate_agents_preset(
    State(state): State<Arc<UiState>>,
    Path(id): Path<String>,
) -> Response {
    result(state.manager.activate_agents_preset(id).await)
}

async fn delete_agents_preset(
    State(state): State<Arc<UiState>>,
    Path(id): Path<String>,
) -> Response {
    result(state.manager.delete_agents_preset(id).await)
}

async fn reimport_agents_presets(State(state): State<Arc<UiState>>) -> Response {
    result(state.manager.reimport_default_agents_presets().await)
}

async fn permission_catalog(State(state): State<Arc<UiState>>) -> Response {
    success(state.permissions.catalog())
}

async fn permission_identities(State(state): State<Arc<UiState>>) -> Response {
    result(state.permissions.identities())
}

async fn identity_permissions(
    State(state): State<Arc<UiState>>,
    Path(id): Path<String>,
) -> Response {
    result(state.permissions.identity_permissions(&id))
}

async fn replace_permissions(
    State(state): State<Arc<UiState>>,
    Path(id): Path<String>,
    Json(request): Json<PermissionUpdate>,
) -> Response {
    result(
        state
            .permissions
            .replace_permissions(&id, request.permissions),
    )
}

async fn create_api_key(
    State(state): State<Arc<UiState>>,
    Json(request): Json<CreateKey>,
) -> Response {
    result(state.permissions.create_api_key(request.name))
}

async fn delete_api_key(State(state): State<Arc<UiState>>, Path(id): Path<String>) -> Response {
    result(state.permissions.delete_api_key(&id))
}

async fn index() -> Response {
    static_asset("index.html")
}

async fn asset(Path(path): Path<String>) -> Response {
    let response = static_asset(&path);
    if response.status() == StatusCode::NOT_FOUND && !path.contains('.') {
        static_asset("index.html")
    } else {
        response
    }
}

fn static_asset(path: &str) -> Response {
    let path = if path.is_empty() { "index.html" } else { path };
    let Some((_, bytes)) = UI_ASSETS.iter().find(|(name, _)| *name == path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let content_type = match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        _ => "application/octet-stream",
    };
    let mut response = Response::new(Body::from(*bytes));
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    response.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static(if path == "index.html" {
            "no-store"
        } else {
            "public, max-age=31536000, immutable"
        }),
    );
    if path == "index.html" {
        response
            .headers_mut()
            .insert(REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    }
    response
}

fn result<T: Serialize>(value: Result<T, ManagerError>) -> Response {
    match value {
        Ok(value) => success(value),
        Err(error) => manager_error(error),
    }
}

fn success<T: Serialize>(value: T) -> Response {
    Json(json!({ "ok": true, "data": value })).into_response()
}

fn manager_error(error: ManagerError) -> Response {
    let status = match error.code.as_str() {
        "bad_request" => StatusCode::BAD_REQUEST,
        "not_found" => StatusCode::NOT_FOUND,
        "conflict" => StatusCode::CONFLICT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    error_response(status, error)
}

fn error_response(status: StatusCode, error: ManagerError) -> Response {
    (status, Json(json!({ "ok": false, "error": error }))).into_response()
}
