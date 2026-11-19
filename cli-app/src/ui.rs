use std::sync::Arc;

use axum::{
    Json, Router,
    body::Body,
    extract::{Extension, Path, Request, State},
    http::{
        HeaderValue, StatusCode,
        header::{AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE, REFERRER_POLICY},
    },
    middleware::{self, Next},
    response::{IntoResponse, Response, sse::Event},
    routing::{delete, get, post, put},
};
use futures::StreamExt;
use proxy_crab_mgr::{
    ProxyCrabManager,
    dto::{
        CreateAgentsPresetRequest, ErrorCode, HttpApiChange, ManagerError,
        UpdateAgentsPresetRequest,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::permissions::{CliPermissionService, PermissionEntry, UiAccess};

include!(concat!(env!("OUT_DIR"), "/ui_assets.rs"));

#[derive(Clone)]
struct UiState {
    manager: Arc<dyn ProxyCrabManager>,
    workspace: String,
    permissions: Arc<CliPermissionService>,
    access: Arc<UiAccess>,
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

pub fn router(
    manager: Arc<dyn ProxyCrabManager>,
    workspace: String,
    permissions: Arc<CliPermissionService>,
    access: Arc<UiAccess>,
) -> Router {
    let state = Arc::new(UiState {
        manager,
        workspace,
        permissions,
        access,
    });
    let protected = Router::new()
        .route("/ui-api/bootstrap", get(bootstrap))
        .route("/ui-api/changes", get(changes))
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
        .route(
            "/ui-api/permissions/identities/{id}/redaction",
            put(set_identity_redaction),
        )
        .route("/ui-api/api-keys", post(create_api_key))
        .route("/ui-api/api-keys/{id}", delete(delete_api_key))
        .route_layer(middleware::from_fn_with_state(state.clone(), authorize_ui))
        .with_state(state);
    protected.merge(
        Router::new()
            .route("/", get(index))
            .route("/session", get(session))
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
            ManagerError::new(ErrorCode::InvalidUiToken, "Access Token 无效"),
        )
    }
}

async fn bootstrap(State(state): State<Arc<UiState>>) -> Response {
    match state.manager.config().await {
        Ok(config) => success(json!({
            "target": "cli",
            "workspace": {
                "current_path": state.workspace.clone(),
                "configured_path": state.workspace.clone(),
            },
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

async fn changes(
    State(state): State<Arc<UiState>>,
    cancellation: Option<Extension<CancellationToken>>,
) -> Response {
    proxy_crab_mgr::changes::response(
        proxy_crab_mgr::changes::stream(state.manager.clone()).map(|change| {
            Event::default()
                .json_data(change)
                .expect("serialize resource change")
        }),
        cancellation
            .map(|Extension(token)| token)
            .unwrap_or_default(),
    )
}

async fn validate_filter_regex(Json(request): Json<RegexRequest>) -> Response {
    success(
        fancy_regex::Regex::new(&request.pattern)
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
    let value = state.permissions.delete_api_key(&id);
    if value.is_ok() {
        let _ = state.manager.change_sender().send(HttpApiChange {
            resources: vec![proxy_crab_mgr::dto::HttpApiResource::All],
            session_id: None,
        });
    }
    result(value)
}

async fn index() -> Response {
    static_asset("index.html")
}

async fn session() -> Response {
    static_asset("session.html")
}

async fn asset(Path(path): Path<String>) -> Response {
    let response = static_asset(&path);
    if response.status() == StatusCode::NOT_FOUND
        && !path.contains('.')
        && !path.starts_with("ui-api/")
    {
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
    let is_html = path.ends_with(".html");
    response.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static(if is_html || path == "assets/session-page-runtime.js" {
            "no-store"
        } else {
            "public, max-age=31536000, immutable"
        }),
    );
    if is_html {
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
    let status = error.code.http_status();
    error_response(status, error)
}

fn error_response(status: StatusCode, error: ManagerError) -> Response {
    (status, Json(json!({ "ok": false, "error": error }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    #[tokio::test]
    async fn filter_regex_validation_supports_lookaround() {
        for (pattern, valid) in [(r"(?i)^get$", true), (r"^(?!xxx).*$", true), ("(", false)] {
            let response = validate_filter_regex(Json(RegexRequest {
                pattern: pattern.into(),
            }))
            .await;
            assert_eq!(response.status(), StatusCode::OK);
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["data"].is_null(), valid, "{pattern}");
            if !valid {
                assert!(body["data"].is_string());
            }
        }
    }
}

async fn set_identity_redaction(
    State(state): State<Arc<UiState>>,
    Path(id): Path<String>,
    Json(binding): Json<proxy_crab_mgr::dto::RedactionBinding>,
) -> Response {
    if let Some(name) = binding
        .redaction_script
        .as_ref()
        .filter(|name| !name.is_empty())
    {
        if let Err(error) = state.manager.redaction_script(name.clone()).await {
            return result::<()>(Err(error));
        }
    }
    result(
        state
            .permissions
            .set_redaction(&id, binding.redaction_script)
            .inspect(|_| {
                let _ = state.manager.change_sender().send(HttpApiChange {
                    resources: vec![proxy_crab_mgr::dto::HttpApiResource::All],
                    session_id: None,
                });
            }),
    )
}

#[cfg(test)]
mod sse_tests {
    use super::*;
    use proxy_crab_mgr::{MitmManager, dto::ScriptRequest, session_share::SessionShareService};
    use proxy_crab_mitm::{ProxyCrab, log_buffer::LogBuffer};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn read_until(connection: &mut tokio::net::TcpStream, marker: &str) -> String {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            let mut value = String::new();
            while !value.contains(marker) {
                let mut bytes = [0; 4096];
                let count = connection.read(&mut bytes).await.unwrap();
                assert!(count > 0, "stream closed before {marker}: {value}");
                value.push_str(std::str::from_utf8(&bytes[..count]).unwrap());
            }
            value
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn ui_sse_requires_bearer_streams_manager_changes_and_closes_on_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = ProxyCrab::open(dir.path(), Arc::new(LogBuffer::default())).unwrap();
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        runtime
            .workspace()
            .override_config_in_memory(|config| config.api_port = port);
        let manager: Arc<dyn ProxyCrabManager> = MitmManager::new(runtime);
        let access = Arc::new(UiAccess::new("test-ui-key"));
        let permissions = CliPermissionService::open(dir.path(), Some(access.clone())).unwrap();
        let ui_manager = manager.clone();
        let ui_permissions = permissions.clone();
        let workspace = dir.path().to_string_lossy().into_owned();
        let server = proxy_crab_mgr::http::start_http_server_with_routes(
            manager.clone(),
            permissions,
            SessionShareService::new(),
            move || router(ui_manager, workspace, ui_permissions, access),
        )
        .await
        .unwrap();
        let mut denied = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        denied
            .write_all(
                b"GET /ui-api/changes HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        assert!(
            read_until(&mut denied, "invalid_ui_token")
                .await
                .starts_with("HTTP/1.1 401")
        );
        let mut connection = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        connection.write_all(b"GET /ui-api/changes HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer test-ui-key\r\n\r\n").await.unwrap();
        let initial = read_until(&mut connection, "\"kind\":\"all\"").await;
        assert!(initial.starts_with("HTTP/1.1 200"));
        assert!(initial.contains("text/event-stream"));
        manager
            .create_column_script(ScriptRequest {
                name: "status".into(),
                content: "return 'ok'".into(),
            })
            .await
            .unwrap();
        let update = read_until(&mut connection, "\"names\":[\"status\"]").await;
        assert!(update.contains("column_scripts"));
        tokio::time::timeout(std::time::Duration::from_secs(2), server.shutdown())
            .await
            .unwrap();
    }
}
