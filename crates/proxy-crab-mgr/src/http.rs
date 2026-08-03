use std::sync::Arc;

use axum::{
    Extension, Json, Router,
    extract::{FromRequest, FromRequestParts, Path, Query, Request, State},
    http::{
        Method, StatusCode,
        header::{HOST, ORIGIN},
        request::Parts,
    },
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use proxy_crab_mitm::model::{AppConfig, InterceptorKind};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use tokio::{sync::broadcast, task::JoinHandle};
use tokio_util::sync::CancellationToken;

use crate::{
    dto::{
        ActiveSession, BypassQuery, CreateSessionRequest, DebugFilterScriptRequest,
        DeleteBypassRequest, HttpApiChange, HttpApiResource, InterceptorCreateRequest,
        InterceptorUpdateRequest, LogIdsRequest, LogViewsRequest, ManagerError,
        ReplaceSessionInterceptorsRequest, ReplaceSessionViewRequest, RoutingSelection,
        ScriptRequest, SessionQuery, SetWorkspaceRequest, SystemLogsQuery, UpdateScriptRequest,
        UpdateSessionRequest,
    },
    manager::ProxyCrabManager,
};

type ManagerState = Arc<dyn ProxyCrabManager>;
type ChangeSender = broadcast::Sender<HttpApiChange>;
type ApiResult = Result<Json<Value>, ApiError>;

struct ApiJson<T>(T);
struct ApiPath<T>(T);
struct ApiQuery<T>(T);

impl<T, S> FromRequest<S> for ApiJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        Json::<T>::from_request(request, state)
            .await
            .map(|Json(value)| Self(value))
            .map_err(|error| ApiError(ManagerError::bad_request(error.body_text())))
    }
}

impl<T, S> FromRequestParts<S> for ApiPath<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Path::<T>::from_request_parts(parts, state)
            .await
            .map(|Path(value)| Self(value))
            .map_err(|error| ApiError(ManagerError::bad_request(error.body_text())))
    }
}

impl<T, S> FromRequestParts<S> for ApiQuery<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Query::<T>::from_request_parts(parts, state)
            .await
            .map(|Query(value)| Self(value))
            .map_err(|error| ApiError(ManagerError::bad_request(error.body_text())))
    }
}

pub struct HttpServerHandle {
    pub host: String,
    pub port: u16,
    cancellation: CancellationToken,
    task: JoinHandle<()>,
    changes: ChangeSender,
}

impl HttpServerHandle {
    pub fn is_running(&self) -> bool {
        !self.task.is_finished()
    }

    pub async fn shutdown(self) {
        self.cancellation.cancel();
        let _ = self.task.await;
    }

    pub fn subscribe_changes(&self) -> broadcast::Receiver<HttpApiChange> {
        self.changes.subscribe()
    }
}

pub async fn start_http_server(manager: ManagerState) -> Result<HttpServerHandle, ManagerError> {
    let config = manager.config().await?;
    let ip = config
        .api_host
        .parse::<std::net::IpAddr>()
        .map_err(|_| ManagerError::bad_request("management API host must be an IP address"))?;
    if !ip.is_loopback() {
        return Err(ManagerError::bad_request(
            "management API host must be a loopback address",
        ));
    }
    let address = std::net::SocketAddr::new(ip, config.api_port);
    let listener = tokio::net::TcpListener::bind(&address)
        .await
        .map_err(|error| ManagerError::internal(format!("failed to bind {address}: {error}")))?;
    let cancellation = CancellationToken::new();
    let shutdown = cancellation.clone();
    let (changes, _) = broadcast::channel(128);
    let app = secured_router_with_changes(manager, changes.clone());
    let task = tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app)
            .with_graceful_shutdown(async move { shutdown.cancelled().await })
            .await
        {
            tracing::error!("management HTTP server failed: {error}");
        }
    });
    Ok(HttpServerHandle {
        host: config.api_host,
        port: config.api_port,
        cancellation,
        task,
        changes,
    })
}

pub fn router(manager: ManagerState) -> Router {
    let (changes, _) = broadcast::channel(128);
    router_with_changes(manager, changes)
}

fn router_with_changes(manager: ManagerState, changes: ChangeSender) -> Router {
    Router::new()
        .route("/api/workspace", get(get_workspace).put(set_workspace))
        .route("/api/config", get(get_config).put(replace_config))
        .route("/api/proxy/status", get(proxy_status))
        .route("/api/proxy/start", post(start_proxy))
        .route("/api/proxy/stop", post(stop_proxy))
        .route("/api/sessions", get(sessions).post(create_session))
        .route(
            "/api/active-session",
            get(active_session).put(replace_active_session),
        )
        .route(
            "/api/sessions/{id}",
            put(update_session).delete(delete_session),
        )
        .route("/api/logs/ids", post(log_ids))
        .route("/api/logs/views", post(log_views))
        .route("/api/logs/{id}", get(log))
        .route(
            "/api/session-view",
            get(session_view).put(replace_session_view),
        )
        .route(
            "/api/session-interceptors",
            get(session_interceptors).put(replace_session_interceptors),
        )
        .route(
            "/api/column-scripts",
            get(column_scripts).post(create_column_script),
        )
        .route(
            "/api/column-scripts/{name}",
            get(column_script)
                .put(update_column_script)
                .delete(delete_column_script),
        )
        .route(
            "/api/filter-scripts",
            get(filter_scripts).post(create_filter_script),
        )
        .route(
            "/api/filter-scripts/{name}/debug",
            post(debug_filter_script),
        )
        .route(
            "/api/filter-scripts/{name}",
            get(filter_script)
                .put(update_filter_script)
                .delete(delete_filter_script),
        )
        .route(
            "/api/routing-scripts",
            get(routing_scripts).post(create_routing_script),
        )
        .route(
            "/api/routing-scripts/{name}",
            get(routing_script)
                .put(update_routing_script)
                .delete(delete_routing_script),
        )
        .route(
            "/api/routing-script-selection",
            get(routing_selection).put(replace_routing_selection),
        )
        .route(
            "/api/interceptors",
            get(interceptors).post(create_interceptor),
        )
        .route(
            "/api/interceptors/{kind}/{name}",
            get(interceptor)
                .put(update_interceptor)
                .delete(delete_interceptor),
        )
        .route("/api/ca", get(certificate).post(regenerate_certificate))
        .route(
            "/api/system-logs",
            get(system_logs).delete(clear_system_logs),
        )
        .route(
            "/api/bypass",
            get(bypass_entries).delete(clear_bypass_entries),
        )
        .route("/api/bypass/delete", post(delete_bypass_entries))
        .route(
            "/api/bypass/{id}",
            axum::routing::delete(delete_bypass_entry),
        )
        .fallback(not_found)
        .with_state(manager)
        .layer(Extension(changes.clone()))
        .layer(middleware::from_fn_with_state(
            changes,
            publish_successful_http_changes,
        ))
}

#[cfg(test)]
fn secured_router(manager: ManagerState) -> Router {
    let (changes, _) = broadcast::channel(128);
    secured_router_with_changes(manager, changes)
}

fn secured_router_with_changes(manager: ManagerState, changes: ChangeSender) -> Router {
    router_with_changes(manager, changes).layer(middleware::from_fn(validate_local_browser_request))
}

async fn publish_successful_http_changes(
    State(changes): State<ChangeSender>,
    request: Request,
    next: Next,
) -> Response {
    let change = change_for_request(
        request.method(),
        request.uri().path(),
        request.uri().query(),
    );
    let response = next.run(request).await;
    if response.status().is_success()
        && let Some(change) = change
    {
        let _ = changes.send(change);
    }
    response
}

fn change_for_request(method: &Method, path: &str, query: Option<&str>) -> Option<HttpApiChange> {
    let resources = match (method, path) {
        (&Method::PUT, "/api/workspace") => vec![HttpApiResource::Workspace],
        (&Method::PUT, "/api/config") => {
            vec![
                HttpApiResource::Config,
                HttpApiResource::RoutingSelection,
                HttpApiResource::ActiveSession,
            ]
        }
        (&Method::POST, "/api/proxy/start" | "/api/proxy/stop") => {
            vec![HttpApiResource::Proxy]
        }
        (&Method::POST, "/api/sessions") => {
            vec![HttpApiResource::Sessions, HttpApiResource::ActiveSession]
        }
        (&Method::PUT, value) if value.starts_with("/api/sessions/") => {
            vec![HttpApiResource::Sessions]
        }
        (&Method::DELETE, value) if value.starts_with("/api/sessions/") => {
            vec![HttpApiResource::Sessions, HttpApiResource::ActiveSession]
        }
        (&Method::PUT, "/api/active-session") => {
            vec![HttpApiResource::ActiveSession, HttpApiResource::Config]
        }
        (&Method::PUT, "/api/session-view") => vec![HttpApiResource::SessionView],
        (&Method::POST, "/api/column-scripts") => {
            vec![HttpApiResource::ColumnScripts, HttpApiResource::SessionView]
        }
        (&Method::PUT | &Method::DELETE, value) if value.starts_with("/api/column-scripts/") => {
            vec![HttpApiResource::ColumnScripts, HttpApiResource::SessionView]
        }
        (&Method::POST, "/api/filter-scripts") => {
            vec![HttpApiResource::FilterScripts, HttpApiResource::SessionView]
        }
        (&Method::PUT | &Method::DELETE, value) if value.starts_with("/api/filter-scripts/") => {
            vec![HttpApiResource::FilterScripts, HttpApiResource::SessionView]
        }
        (&Method::POST, "/api/routing-scripts") => vec![HttpApiResource::RoutingScripts],
        (&Method::PUT | &Method::DELETE, value) if value.starts_with("/api/routing-scripts/") => {
            vec![
                HttpApiResource::RoutingScripts,
                HttpApiResource::RoutingSelection,
            ]
        }
        (&Method::PUT, "/api/routing-script-selection") => {
            vec![HttpApiResource::RoutingSelection]
        }
        (&Method::POST, "/api/interceptors") => vec![
            HttpApiResource::Interceptors,
            HttpApiResource::SessionInterceptors,
        ],
        (&Method::PUT | &Method::DELETE, value) if value.starts_with("/api/interceptors/") => {
            vec![
                HttpApiResource::Interceptors,
                HttpApiResource::SessionInterceptors,
            ]
        }
        (&Method::PUT, "/api/session-interceptors") => {
            vec![HttpApiResource::SessionInterceptors]
        }
        (&Method::POST, "/api/ca") => vec![HttpApiResource::Certificate],
        (&Method::DELETE, "/api/system-logs") => vec![HttpApiResource::SystemLogs],
        (&Method::DELETE, "/api/bypass") | (&Method::POST, "/api/bypass/delete") => {
            vec![HttpApiResource::Bypass]
        }
        (&Method::DELETE, value) if value.starts_with("/api/bypass/") => {
            vec![HttpApiResource::Bypass]
        }
        _ => return None,
    };
    Some(HttpApiChange {
        resources,
        session_id: session_id_from_query(query),
    })
}

fn session_id_from_query(query: Option<&str>) -> Option<u64> {
    query?
        .split('&')
        .filter_map(|part| part.split_once('='))
        .find_map(|(key, value)| (key == "session_id").then(|| value.parse().ok()).flatten())
}

async fn validate_local_browser_request(request: Request, next: Next) -> Response {
    let local_authority = request
        .headers()
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<axum::http::uri::Authority>().ok())
        .is_some_and(|authority| is_local_host(authority.host()));
    if !local_authority {
        return ApiError(ManagerError::bad_request(
            "management API Host must be loopback or localhost",
        ))
        .into_response();
    }
    if let Some(origin) = request.headers().get(ORIGIN) {
        let local_origin = origin
            .to_str()
            .ok()
            .and_then(|value| value.parse::<axum::http::Uri>().ok())
            .is_some_and(|origin| {
                matches!(origin.scheme_str(), Some("http" | "https" | "tauri"))
                    && origin.host().is_some_and(is_local_host)
            });
        if !local_origin {
            return ApiError(ManagerError::new(
                "forbidden_origin",
                "management API Origin must be local",
            ))
            .into_response();
        }
    }
    next.run(request).await
}

fn is_local_host(host: &str) -> bool {
    let host = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host)
        .trim_end_matches('.');
    host.eq_ignore_ascii_case("localhost")
        || host.eq_ignore_ascii_case("tauri.localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

async fn get_workspace(State(manager): State<ManagerState>) -> ApiResult {
    success(manager.workspace().await?)
}

async fn set_workspace(
    State(manager): State<ManagerState>,
    ApiJson(request): ApiJson<SetWorkspaceRequest>,
) -> ApiResult {
    success(manager.set_workspace_for_next_start(request.path).await?)
}

async fn get_config(State(manager): State<ManagerState>) -> ApiResult {
    success(manager.config().await?)
}

async fn replace_config(
    State(manager): State<ManagerState>,
    ApiJson(config): ApiJson<AppConfig>,
) -> ApiResult {
    success(manager.replace_config(config).await?)
}

async fn proxy_status(State(manager): State<ManagerState>) -> ApiResult {
    success(manager.proxy_status().await?)
}

async fn start_proxy(State(manager): State<ManagerState>) -> ApiResult {
    success(manager.start_proxy().await?)
}

async fn stop_proxy(State(manager): State<ManagerState>) -> ApiResult {
    success(manager.stop_proxy().await?)
}

async fn sessions(State(manager): State<ManagerState>) -> ApiResult {
    success(manager.sessions().await?)
}

async fn active_session(State(manager): State<ManagerState>) -> ApiResult {
    success(manager.active_session().await?)
}

async fn replace_active_session(
    State(manager): State<ManagerState>,
    ApiJson(active): ApiJson<ActiveSession>,
) -> ApiResult {
    success(manager.replace_active_session(active).await?)
}

async fn create_session(
    State(manager): State<ManagerState>,
    ApiJson(request): ApiJson<CreateSessionRequest>,
) -> ApiResult {
    success(manager.create_session(request).await?)
}

async fn update_session(
    State(manager): State<ManagerState>,
    ApiPath(id): ApiPath<u64>,
    ApiJson(request): ApiJson<UpdateSessionRequest>,
) -> ApiResult {
    success(manager.update_session(id, request).await?)
}

async fn delete_session(
    State(manager): State<ManagerState>,
    ApiPath(id): ApiPath<u64>,
) -> ApiResult {
    manager.delete_session(id).await?;
    success(json!({}))
}

async fn log_ids(
    State(manager): State<ManagerState>,
    Extension(changes): Extension<ChangeSender>,
    ApiJson(request): ApiJson<LogIdsRequest>,
) -> ApiResult {
    let requested_filter = request.filter.clone();
    let affected_session_id = match request.session_id {
        Some(id) => Some(id),
        None => manager.active_session().await?.session_id,
    };
    let previous_filter = if requested_filter.is_some() {
        manager
            .session_view(affected_session_id)
            .await
            .ok()
            .map(|view| view.filter)
    } else {
        None
    };
    let payload = manager.log_ids(request).await?;
    if requested_filter.is_some() && previous_filter.as_ref() != Some(&payload.filter) {
        let _ = changes.send(HttpApiChange {
            resources: vec![HttpApiResource::SessionView],
            session_id: affected_session_id,
        });
    }
    success(payload)
}

async fn log_views(
    State(manager): State<ManagerState>,
    ApiJson(request): ApiJson<LogViewsRequest>,
) -> ApiResult {
    success(manager.log_views(request).await?)
}

async fn log(
    State(manager): State<ManagerState>,
    ApiPath(id): ApiPath<u64>,
    ApiQuery(query): ApiQuery<SessionQuery>,
) -> ApiResult {
    success(manager.log(query.session_id, id).await?)
}

async fn session_view(
    State(manager): State<ManagerState>,
    ApiQuery(query): ApiQuery<SessionQuery>,
) -> ApiResult {
    success(manager.session_view(query.session_id).await?)
}

async fn replace_session_view(
    State(manager): State<ManagerState>,
    ApiQuery(query): ApiQuery<SessionQuery>,
    ApiJson(request): ApiJson<ReplaceSessionViewRequest>,
) -> ApiResult {
    success(
        manager
            .replace_session_view(query.session_id, request)
            .await?,
    )
}

async fn session_interceptors(
    State(manager): State<ManagerState>,
    ApiQuery(query): ApiQuery<SessionQuery>,
) -> ApiResult {
    success(manager.session_interceptors(query.session_id).await?)
}

async fn replace_session_interceptors(
    State(manager): State<ManagerState>,
    ApiQuery(query): ApiQuery<SessionQuery>,
    ApiJson(request): ApiJson<ReplaceSessionInterceptorsRequest>,
) -> ApiResult {
    success(
        manager
            .replace_session_interceptors(query.session_id, request)
            .await?,
    )
}

async fn column_scripts(State(manager): State<ManagerState>) -> ApiResult {
    success(manager.column_scripts().await?)
}

async fn create_column_script(
    State(manager): State<ManagerState>,
    ApiJson(request): ApiJson<ScriptRequest>,
) -> ApiResult {
    manager.create_column_script(request).await?;
    success(json!({}))
}

async fn column_script(
    State(manager): State<ManagerState>,
    ApiPath(name): ApiPath<String>,
) -> ApiResult {
    success(manager.column_script(name).await?)
}

async fn update_column_script(
    State(manager): State<ManagerState>,
    ApiPath(name): ApiPath<String>,
    ApiJson(request): ApiJson<UpdateScriptRequest>,
) -> ApiResult {
    manager.update_column_script(name, request).await?;
    success(json!({}))
}

async fn delete_column_script(
    State(manager): State<ManagerState>,
    ApiPath(name): ApiPath<String>,
) -> ApiResult {
    manager.delete_column_script(name).await?;
    success(json!({}))
}

async fn filter_scripts(State(manager): State<ManagerState>) -> ApiResult {
    success(manager.filter_scripts().await?)
}

async fn create_filter_script(
    State(manager): State<ManagerState>,
    ApiJson(request): ApiJson<ScriptRequest>,
) -> ApiResult {
    manager.create_filter_script(request).await?;
    success(json!({}))
}

async fn filter_script(
    State(manager): State<ManagerState>,
    ApiPath(name): ApiPath<String>,
) -> ApiResult {
    success(manager.filter_script(name).await?)
}

async fn update_filter_script(
    State(manager): State<ManagerState>,
    ApiPath(name): ApiPath<String>,
    ApiJson(request): ApiJson<UpdateScriptRequest>,
) -> ApiResult {
    manager.update_filter_script(name, request).await?;
    success(json!({}))
}

async fn delete_filter_script(
    State(manager): State<ManagerState>,
    ApiPath(name): ApiPath<String>,
) -> ApiResult {
    manager.delete_filter_script(name).await?;
    success(json!({}))
}

async fn debug_filter_script(
    State(manager): State<ManagerState>,
    ApiPath(name): ApiPath<String>,
    ApiJson(request): ApiJson<DebugFilterScriptRequest>,
) -> ApiResult {
    success(manager.debug_filter_script(name, request).await?)
}

async fn routing_scripts(State(manager): State<ManagerState>) -> ApiResult {
    success(manager.routing_scripts().await?)
}

async fn create_routing_script(
    State(manager): State<ManagerState>,
    ApiJson(request): ApiJson<ScriptRequest>,
) -> ApiResult {
    manager.create_routing_script(request).await?;
    success(json!({}))
}

async fn routing_script(
    State(manager): State<ManagerState>,
    ApiPath(name): ApiPath<String>,
) -> ApiResult {
    success(manager.routing_script(name).await?)
}

async fn update_routing_script(
    State(manager): State<ManagerState>,
    ApiPath(name): ApiPath<String>,
    ApiJson(request): ApiJson<UpdateScriptRequest>,
) -> ApiResult {
    manager.update_routing_script(name, request).await?;
    success(json!({}))
}

async fn delete_routing_script(
    State(manager): State<ManagerState>,
    ApiPath(name): ApiPath<String>,
) -> ApiResult {
    manager.delete_routing_script(name).await?;
    success(json!({}))
}

async fn routing_selection(State(manager): State<ManagerState>) -> ApiResult {
    success(manager.routing_selection().await?)
}

async fn replace_routing_selection(
    State(manager): State<ManagerState>,
    ApiJson(selection): ApiJson<RoutingSelection>,
) -> ApiResult {
    success(manager.replace_routing_selection(selection).await?)
}

#[derive(Deserialize)]
struct KindQuery {
    kind: InterceptorKind,
}

async fn interceptors(
    State(manager): State<ManagerState>,
    ApiQuery(query): ApiQuery<KindQuery>,
) -> ApiResult {
    success(manager.interceptors(query.kind).await?)
}

async fn create_interceptor(
    State(manager): State<ManagerState>,
    ApiJson(request): ApiJson<InterceptorCreateRequest>,
) -> ApiResult {
    manager.create_interceptor(request).await?;
    success(json!({}))
}

async fn interceptor(
    State(manager): State<ManagerState>,
    ApiPath((kind, name)): ApiPath<(String, String)>,
) -> ApiResult {
    let kind = parse_kind(&kind)?;
    success(manager.interceptor(kind, name).await?)
}

async fn update_interceptor(
    State(manager): State<ManagerState>,
    ApiPath((kind, name)): ApiPath<(String, String)>,
    ApiJson(request): ApiJson<InterceptorUpdateRequest>,
) -> ApiResult {
    manager
        .update_interceptor(parse_kind(&kind)?, name, request)
        .await?;
    success(json!({}))
}

async fn delete_interceptor(
    State(manager): State<ManagerState>,
    ApiPath((kind, name)): ApiPath<(String, String)>,
) -> ApiResult {
    manager.delete_interceptor(parse_kind(&kind)?, name).await?;
    success(json!({}))
}

async fn certificate(State(manager): State<ManagerState>) -> ApiResult {
    success(manager.certificate().await?)
}

async fn regenerate_certificate(State(manager): State<ManagerState>) -> ApiResult {
    success(manager.regenerate_certificate().await?)
}

async fn system_logs(
    State(manager): State<ManagerState>,
    ApiQuery(query): ApiQuery<SystemLogsQuery>,
) -> ApiResult {
    success(manager.system_logs(query).await?)
}

async fn clear_system_logs(State(manager): State<ManagerState>) -> ApiResult {
    manager.clear_system_logs().await?;
    success(json!({}))
}

async fn bypass_entries(
    State(manager): State<ManagerState>,
    ApiQuery(query): ApiQuery<BypassQuery>,
) -> ApiResult {
    success(manager.bypass_entries(query).await?)
}

async fn delete_bypass_entry(
    State(manager): State<ManagerState>,
    ApiPath(id): ApiPath<u64>,
) -> ApiResult {
    manager.delete_bypass_entry(id).await?;
    success(json!({}))
}

async fn delete_bypass_entries(
    State(manager): State<ManagerState>,
    ApiJson(request): ApiJson<DeleteBypassRequest>,
) -> ApiResult {
    success(manager.delete_bypass_entries(request.ids).await?)
}

async fn clear_bypass_entries(State(manager): State<ManagerState>) -> ApiResult {
    success(manager.clear_bypass_entries().await?)
}

async fn not_found() -> ApiResult {
    Err(ApiError(ManagerError::not_found("api endpoint not found")))
}

fn parse_kind(kind: &str) -> Result<InterceptorKind, ApiError> {
    match kind {
        "request" => Ok(InterceptorKind::Request),
        "response" => Ok(InterceptorKind::Response),
        _ => Err(ApiError(ManagerError::bad_request(
            "interceptor kind must be request or response",
        ))),
    }
}

fn success(value: impl serde::Serialize) -> ApiResult {
    Ok(Json(json!({ "ok": true, "data": value })))
}

struct ApiError(ManagerError);

impl From<ManagerError> for ApiError {
    fn from(error: ManagerError) -> Self {
        Self(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match self.0.code.as_str() {
            "bad_request" => StatusCode::BAD_REQUEST,
            "not_found" => StatusCode::NOT_FOUND,
            "forbidden_origin" => StatusCode::FORBIDDEN,
            "conflict" | "proxy_running" | "session_in_use" => StatusCode::CONFLICT,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (
            status,
            Json(json!({
                "ok": false,
                "error": {
                    "code": self.0.code,
                    "message": self.0.message,
                }
            })),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        body::{Body, to_bytes},
        http::{Method, Request},
    };
    use proxy_crab_mitm::{ProxyCrab, log_buffer::LogBuffer};
    use tempfile::tempdir;
    use tower::ServiceExt;

    use crate::{
        dto::HttpApiResource,
        http::{change_for_request, router, router_with_changes, secured_router},
        manager::{MitmManager, ProxyCrabManager},
    };

    #[test]
    fn maps_http_mutations_to_their_ui_resources() {
        let cases = [
            (
                "PUT",
                "/api/workspace",
                None,
                vec![HttpApiResource::Workspace],
                None,
            ),
            (
                "PUT",
                "/api/config",
                None,
                vec![
                    HttpApiResource::Config,
                    HttpApiResource::RoutingSelection,
                    HttpApiResource::ActiveSession,
                ],
                None,
            ),
            (
                "PUT",
                "/api/active-session",
                None,
                vec![HttpApiResource::ActiveSession, HttpApiResource::Config],
                None,
            ),
            (
                "PUT",
                "/api/sessions/42",
                None,
                vec![HttpApiResource::Sessions],
                None,
            ),
            (
                "PUT",
                "/api/session-view",
                Some("session_id=42"),
                vec![HttpApiResource::SessionView],
                Some(42),
            ),
            (
                "PUT",
                "/api/column-scripts/host",
                None,
                vec![HttpApiResource::ColumnScripts, HttpApiResource::SessionView],
                None,
            ),
            (
                "DELETE",
                "/api/filter-scripts/errors",
                None,
                vec![HttpApiResource::FilterScripts, HttpApiResource::SessionView],
                None,
            ),
            (
                "PUT",
                "/api/routing-script-selection",
                None,
                vec![HttpApiResource::RoutingSelection],
                None,
            ),
            (
                "POST",
                "/api/interceptors",
                None,
                vec![
                    HttpApiResource::Interceptors,
                    HttpApiResource::SessionInterceptors,
                ],
                None,
            ),
            (
                "PUT",
                "/api/session-interceptors",
                Some("session_id=42"),
                vec![HttpApiResource::SessionInterceptors],
                Some(42),
            ),
            (
                "POST",
                "/api/ca",
                None,
                vec![HttpApiResource::Certificate],
                None,
            ),
            (
                "POST",
                "/api/bypass/delete",
                None,
                vec![HttpApiResource::Bypass],
                None,
            ),
            (
                "DELETE",
                "/api/system-logs",
                None,
                vec![HttpApiResource::SystemLogs],
                None,
            ),
        ];

        for (method, path, query, resources, session_id) in cases {
            let change =
                change_for_request(&method.parse().unwrap(), path, query).expect("mapped change");
            assert_eq!(change.resources, resources, "{method} {path}");
            assert_eq!(change.session_id, session_id, "{method} {path}");
        }
        assert!(change_for_request(&Method::GET, "/api/sessions", None).is_none());
        assert!(
            change_for_request(&Method::POST, "/api/filter-scripts/name/debug", None).is_none()
        );
        assert!(change_for_request(&Method::POST, "/api/logs/ids", None).is_none());
    }

    #[tokio::test]
    async fn returns_the_standard_success_envelope() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let app = router(MitmManager::new(runtime));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/proxy/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn active_session_http_api_is_nullable_and_validates_session_ids() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let session = runtime
            .create_session(Some("capture".into()), None)
            .unwrap();
        let app = router(MitmManager::new(runtime));

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/active-session")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["data"]["session_id"], session.id);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/active-session")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"session_id":null}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);

        let response = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/active-session")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"session_id":18446744073709551615}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn http_and_trait_return_the_same_config_dto() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let manager = MitmManager::new(runtime);
        let expected = serde_json::to_value(manager.config().await.unwrap()).unwrap();
        let app = router(manager);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/config")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(body["data"], expected);
    }

    #[tokio::test]
    async fn invalid_json_uses_the_standard_error_envelope() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let app = router(MitmManager::new(runtime));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from("{"))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"]["code"], "bad_request");
    }

    #[tokio::test]
    async fn publishes_ui_resources_only_after_a_successful_http_write() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let (changes, mut receiver) = tokio::sync::broadcast::channel(8);
        let app = router_with_changes(MitmManager::new(runtime), changes);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"external"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let change = receiver.try_recv().unwrap();
        assert_eq!(
            change.resources,
            vec![HttpApiResource::Sessions, HttpApiResource::ActiveSession]
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from("{"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
        assert!(matches!(
            receiver.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty
                | tokio::sync::broadcast::error::TryRecvError::Closed)
        ));
    }

    #[tokio::test]
    async fn publishes_session_view_only_when_an_http_log_filter_changes() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let session = runtime.create_session(None, None).unwrap();
        let (changes, mut receiver) = tokio::sync::broadcast::channel(8);
        let app = router_with_changes(MitmManager::new(runtime), changes);
        let body = r#"{"filter":{"option":{"kind":"column","column":{"kind":"uri"},"case_sensitive":false},"input":"example"}}"#;

        for _ in 0..2 {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/logs/ids")
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), axum::http::StatusCode::OK);
        }

        let change = receiver.try_recv().unwrap();
        assert_eq!(change.resources, vec![HttpApiResource::SessionView]);
        assert_eq!(change.session_id, Some(session.id));
        assert!(matches!(
            receiver.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn invalid_path_uses_the_standard_error_envelope() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let app = router(MitmManager::new(runtime));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/logs/not-a-number")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"]["code"], "bad_request");
    }

    #[tokio::test]
    async fn exposes_only_the_new_log_and_session_view_routes() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        runtime.create_session(None, None).unwrap();
        let app = router(MitmManager::new(runtime));

        for (method, uri, body) in [
            ("POST", "/api/logs/ids", r#"{}"#),
            (
                "POST",
                "/api/logs/ids",
                r#"{"filter":{"option":{"kind":"column","column":{"kind":"uri"},"case_sensitive":false},"input":""}}"#,
            ),
            ("POST", "/api/logs/views", r#"{"logs":[]}"#),
            ("GET", "/api/session-view", ""),
            ("GET", "/api/active-session", ""),
            ("PUT", "/api/session-view", r#"{"columns":[]}"#),
            ("GET", "/api/session-interceptors", ""),
            (
                "PUT",
                "/api/session-interceptors",
                r#"{"request":[],"response":[]}"#,
            ),
            (
                "POST",
                "/api/filter-scripts",
                r#"{"name":"example","content":"return true"}"#,
            ),
            ("GET", "/api/filter-scripts", ""),
            (
                "POST",
                "/api/routing-scripts",
                r#"{"name":"route","content":"return true"}"#,
            ),
            (
                "PUT",
                "/api/routing-script-selection",
                r#"{"name":"route"}"#,
            ),
            ("PUT", "/api/active-session", r#"{"session_id":null}"#),
            ("GET", "/api/bypass", ""),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), axum::http::StatusCode::OK, "{uri}");
        }

        for (method, uri) in [
            ("GET", "/api/logs"),
            ("POST", "/api/logs/filter"),
            ("GET", "/api/sessions/1/logs"),
            ("GET", "/api/columns"),
            ("PUT", "/api/interceptors/order"),
            ("GET", "/api/filter-history"),
            ("POST", "/api/sessions/1/activate"),
            ("POST", "/api/interceptors/request/example/enable"),
            ("POST", "/api/interceptors/request/example/disable"),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert!(
                matches!(
                    response.status(),
                    axum::http::StatusCode::NOT_FOUND | axum::http::StatusCode::METHOD_NOT_ALLOWED
                ),
                "{uri}: {}",
                response.status()
            );
        }
    }

    #[tokio::test]
    async fn script_updates_reject_the_removed_rename_shape() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let app = router(MitmManager::new(runtime));

        for uri in [
            "/api/column-scripts/old",
            "/api/filter-scripts/old",
            "/api/routing-scripts/old",
            "/api/interceptors/request/old",
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("PUT")
                        .uri(uri)
                        .header("content-type", "application/json")
                        .body(Body::from(r#"{"name":"new","content":"return true"}"#))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
        }
    }

    #[tokio::test]
    async fn rejects_non_local_host_and_origin() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let manager = MitmManager::new(runtime);
        let app = secured_router(manager);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/proxy/status")
                    .header("host", "attacker.example")
                    .header("origin", "https://attacker.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    }
}
