use std::{io, net::SocketAddr, pin::Pin, sync::Arc};

use async_compression::tokio::bufread::{BrotliDecoder, GzipDecoder, ZlibDecoder, ZstdDecoder};
use axum::{
    Extension, Json, Router,
    body::Body,
    extract::{
        ConnectInfo, FromRequest, FromRequestParts, MatchedPath, Path, Query, Request, State,
    },
    http::{
        HeaderValue, Method, StatusCode,
        header::{
            ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS,
            ACCESS_CONTROL_ALLOW_ORIGIN, AUTHORIZATION, CONTENT_DISPOSITION, CONTENT_ENCODING,
            CONTENT_LENGTH, CONTENT_TYPE, HOST, ORIGIN, VARY,
        },
        request::Parts,
    },
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use bytes::Bytes;
use futures::{StreamExt, stream};
use proxy_crab_mitm::model::{AppConfig, InterceptorKind};
use proxy_crab_mitm::storage::{BodySide, BodySource, BodySourceData};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use tokio::io::{AsyncRead, BufReader};
use tokio::{sync::broadcast, task::JoinHandle};
use tokio_util::{
    io::{ReaderStream, StreamReader},
    sync::CancellationToken,
};

use crate::{
    dto::{
        ActiveSession, AssetQuery, BodyQuery, BreakpointQuery, BypassQuery, CreateSessionRequest,
        DebugFilterScriptRequest, DeleteBypassRequest, ExecuteTemporaryScriptRequest,
        ExportLogsRequest, ExtendBreakpointRequest, HttpApiChange, HttpApiResource,
        InterceptorCreateRequest, InterceptorUpdateRequest, LogIdsRequest, LogViewsRequest,
        ManagerError, ManagerResult, ReplaceSessionInterceptorsRequest, ReplaceSessionViewRequest,
        RoutingSelection, ScriptRequest, SessionQuery, SetWorkspaceRequest, SystemLogsQuery,
        UpdateScriptRequest, UpdateSessionRequest, default_body_max_size,
    },
    manager::ProxyCrabManager,
    permission::{
        PermissionAction, PermissionDenied, PermissionDeniedStatus, PermissionManager, api_actions,
        api_actions_for_path, find_api_action,
    },
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

pub async fn start_http_server(
    manager: ManagerState,
    permissions: Arc<dyn PermissionManager>,
) -> Result<HttpServerHandle, ManagerError> {
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
    let app = secured_router_with_changes(manager, permissions, changes.clone());
    let task = tokio::spawn(async move {
        if let Err(error) = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
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

pub fn router(manager: ManagerState, permissions: Arc<dyn PermissionManager>) -> Router {
    let (changes, _) = broadcast::channel(128);
    router_with_changes(manager, permissions, changes)
}

fn router_with_changes(
    manager: ManagerState,
    permissions: Arc<dyn PermissionManager>,
    changes: ChangeSender,
) -> Router {
    Router::new()
        .route("/api/agents.md", get(agents_markdown))
        .route("/api/workspace", get(get_workspace).put(set_workspace))
        .route("/api/assets/{*asset_id}", get(asset).post(upload_asset))
        .route("/api/config", get(get_config).put(replace_config))
        .route("/api/proxy/status", get(proxy_status))
        .route("/api/proxy/start", post(start_proxy))
        .route("/api/proxy/stop", post(stop_proxy))
        .route("/api/sessions", get(sessions).post(create_session))
        .route("/api/archived-sessions", get(archived_sessions))
        .route("/api/sessions/{id}/archive", post(archive_session))
        .route("/api/archived-sessions/{id}/restore", post(restore_session))
        .route(
            "/api/archived-sessions/{id}",
            axum::routing::delete(delete_archived_session),
        )
        .route(
            "/api/active-session",
            get(active_session).put(replace_active_session),
        )
        .route(
            "/api/sessions/{id}",
            put(update_session).fallback(|| async { StatusCode::NOT_FOUND }),
        )
        .route("/api/logs/export", post(export_logs))
        .route("/api/logs/ids", post(log_ids))
        .route("/api/logs/views", post(log_views))
        .route("/api/logs/{id}/body", get(log_body))
        .route("/api/logs/{id}", get(log))
        .route("/api/breakpoints", get(breakpoints))
        .route("/api/breakpoints/{id}/body", get(breakpoint_body))
        .route("/api/breakpoints/{id}", get(breakpoint))
        .route("/api/breakpoints/{id}/extend", post(extend_breakpoint))
        .route("/api/breakpoints/{id}/release", post(release_breakpoint))
        .route(
            "/api/breakpoints/{id}/execute",
            post(execute_breakpoint_script),
        )
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
        .route_layer(middleware::from_fn_with_state(
            permissions,
            authorize_request,
        ))
        .fallback(not_found)
        .with_state(manager)
        .layer(Extension(changes.clone()))
        .layer(middleware::from_fn_with_state(
            changes,
            publish_successful_http_changes,
        ))
}

#[cfg(test)]
fn secured_router(manager: ManagerState, permissions: Arc<dyn PermissionManager>) -> Router {
    let (changes, _) = broadcast::channel(128);
    secured_router_with_changes(manager, permissions, changes)
}

fn secured_router_with_changes(
    manager: ManagerState,
    permissions: Arc<dyn PermissionManager>,
    changes: ChangeSender,
) -> Router {
    router_with_changes(manager, permissions, changes)
        .layer(middleware::from_fn(validate_local_browser_request))
}

const BODY_PREVIEW_LIMIT: usize = 16 * 1024;

async fn authorize_request(
    State(permissions): State<Arc<dyn PermissionManager>>,
    request: Request,
    next: Next,
) -> Response {
    let route_template = request
        .extensions()
        .get::<MatchedPath>()
        .map(|path| path.as_str().to_owned());
    let Some(route_template) = route_template else {
        return next.run(request).await;
    };
    let Some(action) = find_api_action(request.method().as_str(), &route_template) else {
        let known_template = api_actions()
            .iter()
            .any(|action| action.route_template == route_template);
        if known_template || !route_template.starts_with("/api/") {
            return next.run(request).await;
        }
        return permission_denied_response(PermissionDenied::internal(
            "permission_check_failed",
            "management API route is missing from the permission catalog",
        ));
    };

    let authorization = match request
        .headers()
        .get_all(AUTHORIZATION)
        .iter()
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => None,
        [value] => match value.to_str() {
            Ok(value) => Some(value.to_owned()),
            Err(_) => {
                return permission_denied_response(PermissionDenied::unauthorized(
                    "invalid_api_key",
                    "Authorization must contain one valid Bearer API key",
                ));
            }
        },
        _ => {
            return permission_denied_response(PermissionDenied::unauthorized(
                "invalid_api_key",
                "Authorization must contain exactly one Bearer API key",
            ));
        }
    };
    let source = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|value| value.0);
    let content_type = request
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let content_length = request
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok());
    let actual_path = request.uri().path().to_owned();
    let query = request.uri().query().map(str::to_owned);
    let (request, body_preview, body_preview_truncated) =
        preview_request_body(request, content_type.as_deref()).await;

    let permission = PermissionAction {
        action,
        authorization,
        actual_path,
        query,
        source,
        content_type,
        content_length,
        body_preview,
        body_preview_truncated: body_preview_truncated
            || content_length.is_some_and(|length| length > BODY_PREVIEW_LIMIT as u64),
    };
    if let Some(denied) = permissions.check_permission(permission).await {
        return permission_denied_response(denied);
    }
    next.run(request).await
}

async fn preview_request_body(
    request: Request,
    content_type: Option<&str>,
) -> (Request, Option<String>, bool) {
    let textual = content_type.is_some_and(|value| {
        value.starts_with("text/")
            || value.contains("json")
            || value.contains("xml")
            || value.contains("x-www-form-urlencoded")
    });
    if !textual {
        return (request, None, false);
    }
    let (parts, body) = request.into_parts();
    let mut stream = body.into_data_stream();
    let mut buffered = Vec::new();
    let mut preview = Vec::new();
    let mut truncated = false;
    while !truncated {
        let Some(item) = stream.next().await else {
            break;
        };
        match item {
            Ok(bytes) => {
                let remaining = BODY_PREVIEW_LIMIT - preview.len();
                let has_bytes = !bytes.is_empty();
                preview.extend_from_slice(&bytes[..bytes.len().min(remaining)]);
                truncated |= bytes.len() > remaining;
                buffered.push(Ok::<Bytes, axum::Error>(bytes));
                if remaining == 0 && has_bytes {
                    truncated = true;
                }
            }
            Err(error) => {
                buffered.push(Err(error));
                break;
            }
        }
    }
    let body = Body::from_stream(stream::iter(buffered).chain(stream));
    let preview = String::from_utf8(preview).ok();
    (Request::from_parts(parts, body), preview, truncated)
}

fn permission_denied_response(denied: PermissionDenied) -> Response {
    let status = match denied.status {
        PermissionDeniedStatus::Unauthorized => StatusCode::UNAUTHORIZED,
        PermissionDeniedStatus::Forbidden => StatusCode::FORBIDDEN,
        PermissionDeniedStatus::Internal => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (
        status,
        Json(json!({
            "ok": false,
            "error": { "code": denied.code, "message": denied.message }
        })),
    )
        .into_response()
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
        (&Method::POST, value) if value.ends_with("/archive") => {
            vec![HttpApiResource::Sessions, HttpApiResource::ArchivedSessions]
        }
        (&Method::POST, value) if value.ends_with("/restore") => {
            vec![HttpApiResource::Sessions, HttpApiResource::ArchivedSessions]
        }
        (&Method::DELETE, value) if value.starts_with("/api/archived-sessions/") => {
            vec![HttpApiResource::ArchivedSessions]
        }
        (&Method::PUT, value) if value.starts_with("/api/sessions/") => {
            vec![HttpApiResource::Sessions]
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
    let allowed_origin = request.headers().get(ORIGIN).cloned();
    if let Some(origin) = &allowed_origin {
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
    let is_preflight = request.method() == Method::OPTIONS;
    let path = request.uri().path().to_owned();
    let mut response = if is_preflight {
        let methods = api_actions_for_path(&path)
            .map(|action| action.method)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join(", ");
        if methods.is_empty() {
            ApiError(ManagerError::not_found("api endpoint not found")).into_response()
        } else {
            StatusCode::NO_CONTENT.into_response()
        }
    } else {
        next.run(request).await
    };
    if let Some(origin) = allowed_origin {
        response
            .headers_mut()
            .insert(ACCESS_CONTROL_ALLOW_ORIGIN, origin);
        response
            .headers_mut()
            .insert(VARY, HeaderValue::from_static("Origin"));
    }
    if is_preflight && response.status().is_success() {
        let methods = api_actions_for_path(&path)
            .map(|action| action.method)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join(", ");
        if let Ok(value) = HeaderValue::from_str(&methods) {
            response
                .headers_mut()
                .insert(ACCESS_CONTROL_ALLOW_METHODS, value);
        }
        response.headers_mut().insert(
            ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("Authorization, Content-Type"),
        );
    }
    response
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

async fn agents_markdown(State(manager): State<ManagerState>) -> Result<Response, ApiError> {
    let markdown = manager.agents_markdown().await?;
    Ok(([(CONTENT_TYPE, "text/plain; charset=utf-8")], markdown).into_response())
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

async fn archived_sessions(State(manager): State<ManagerState>) -> ApiResult {
    success(manager.archived_sessions().await?)
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

async fn archive_session(
    State(manager): State<ManagerState>,
    ApiPath(id): ApiPath<u64>,
) -> ApiResult {
    success(manager.archive_session(id).await?)
}

async fn restore_session(
    State(manager): State<ManagerState>,
    ApiPath(id): ApiPath<u64>,
) -> ApiResult {
    success(manager.restore_session(id).await?)
}

async fn delete_archived_session(
    State(manager): State<ManagerState>,
    ApiPath(id): ApiPath<u64>,
) -> ApiResult {
    manager.delete_archived_session(id).await?;
    success(json!({}))
}

async fn log_ids(
    State(manager): State<ManagerState>,
    Extension(changes): Extension<ChangeSender>,
    ApiJson(request): ApiJson<LogIdsRequest>,
) -> ApiResult {
    let requested_filter = request.filter.clone();
    let persist_filter = request.persist_filter;
    let affected_session_id = match request.session_id {
        Some(id) => Some(id),
        None => manager.active_session().await?.session_id,
    };
    let previous_filter = if requested_filter.is_some() && persist_filter {
        manager
            .session_view(affected_session_id)
            .await
            .ok()
            .map(|view| view.filter)
    } else {
        None
    };
    let payload = manager.log_ids(request).await?;
    if requested_filter.is_some()
        && persist_filter
        && previous_filter.as_ref() != Some(&payload.filter)
    {
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

async fn export_logs(
    State(manager): State<ManagerState>,
    ApiJson(request): ApiJson<ExportLogsRequest>,
) -> Result<Response, ApiError> {
    let export = manager.export_logs(request).await?;
    let content_length = export.bytes.len();
    let mut response = Response::new(Body::from(export.bytes));
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    response.headers_mut().insert(
        CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{}\"", export.filename))
            .map_err(|error| ApiError(ManagerError::internal(error.to_string())))?,
    );
    response.headers_mut().insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&content_length.to_string())
            .expect("usize is always a valid Content-Length"),
    );
    Ok(response)
}

async fn upload_asset(
    State(manager): State<ManagerState>,
    ApiPath(asset_id): ApiPath<String>,
    request: Request,
) -> Result<Response, ApiError> {
    let content_type = request
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream")
        .trim()
        .to_owned();
    let mut upload = manager.begin_asset_upload(asset_id, content_type).await?;
    let mut body = request.into_body().into_data_stream();
    while let Some(bytes) = body.next().await {
        let bytes = bytes.map_err(|error| {
            ApiError(ManagerError::new(
                "asset_store_failed",
                format!("failed to read asset upload: {error}"),
            ))
        })?;
        upload.write(&bytes).await.map_err(|error| {
            ApiError(ManagerError::new("asset_store_failed", error.to_string()))
        })?;
    }
    let metadata = upload.finish().await.map_err(|error| {
        let error = match error {
            proxy_crab_mitm::asset::AssetError::AlreadyExists(id) => {
                ManagerError::new("asset_already_exists", format!("asset {id} already exists"))
            }
            proxy_crab_mitm::asset::AssetError::PathConflict(id) => ManagerError::new(
                "asset_path_conflict",
                format!("asset path conflicts with an existing file or directory: {id}"),
            ),
            other => ManagerError::new("asset_store_failed", other.to_string()),
        };
        ApiError(error)
    })?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "ok": true, "data": metadata })),
    )
        .into_response())
}

async fn asset(
    State(manager): State<ManagerState>,
    ApiPath(asset_id): ApiPath<String>,
    ApiQuery(query): ApiQuery<AssetQuery>,
) -> Result<Response, ApiError> {
    let asset = manager.asset(asset_id).await?;
    match query.format.as_deref() {
        None => Ok(Json(json!({ "ok": true, "data": asset.metadata })).into_response()),
        Some("raw") => {
            let filename = asset
                .metadata
                .id
                .rsplit('/')
                .next()
                .expect("validated asset id has a filename");
            let file = tokio::fs::File::open(asset.path()).await.map_err(|error| {
                ApiError(ManagerError::new(
                    "asset_store_failed",
                    format!("failed to open asset: {error}"),
                ))
            })?;
            let stream = ReaderStream::new(file);
            let mut response = Response::new(Body::from_stream(stream));
            let headers = response.headers_mut();
            headers.insert(
                CONTENT_TYPE,
                HeaderValue::from_str(&asset.metadata.content_type).map_err(|error| {
                    ApiError(ManagerError::new("asset_store_failed", error.to_string()))
                })?,
            );
            headers.insert(
                CONTENT_LENGTH,
                HeaderValue::from_str(&asset.metadata.size.to_string())
                    .expect("u64 is always a valid Content-Length"),
            );
            headers.insert(
                CONTENT_DISPOSITION,
                HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
                    .expect("validated asset filenames are safe header values"),
            );
            headers.insert(
                "x-proxycrab-asset-sha256",
                HeaderValue::from_str(&asset.metadata.sha256)
                    .expect("SHA-256 hex is a safe header value"),
            );
            Ok(response)
        }
        Some(_) => Err(ApiError(ManagerError::new(
            "invalid_asset_format",
            "asset format must be raw when present",
        ))),
    }
}

async fn log(
    State(manager): State<ManagerState>,
    ApiPath(id): ApiPath<u64>,
    ApiQuery(query): ApiQuery<SessionQuery>,
) -> ApiResult {
    success(manager.log(query.session_id, id).await?)
}

async fn log_body(
    State(manager): State<ManagerState>,
    ApiPath(id): ApiPath<u64>,
    ApiQuery(query): ApiQuery<BodyQuery>,
) -> Result<Response, ApiError> {
    let side = parse_body_side(&query.side)?;
    validate_body_query(&query)?;
    let source = manager.log_body_source(query.session_id, id, side).await?;
    body_response(source, query.decompress, query.max_size)
        .await
        .map_err(ApiError)
}

async fn breakpoints(
    State(manager): State<ManagerState>,
    ApiQuery(query): ApiQuery<BreakpointQuery>,
) -> ApiResult {
    success(manager.breakpoints(query).await?)
}

async fn breakpoint(State(manager): State<ManagerState>, ApiPath(id): ApiPath<u64>) -> ApiResult {
    success(manager.breakpoint(id).await?)
}

async fn breakpoint_body(
    State(manager): State<ManagerState>,
    ApiPath(id): ApiPath<u64>,
    ApiQuery(query): ApiQuery<BodyQuery>,
) -> Result<Response, ApiError> {
    let side = parse_body_side(&query.side)?;
    validate_body_query(&query)?;
    let source = manager.breakpoint_body_source(id, side).await?;
    body_response(source, query.decompress, query.max_size)
        .await
        .map_err(ApiError)
}

async fn extend_breakpoint(
    State(manager): State<ManagerState>,
    ApiPath(id): ApiPath<u64>,
    ApiJson(request): ApiJson<ExtendBreakpointRequest>,
) -> ApiResult {
    success(manager.extend_breakpoint(id, request).await?)
}

async fn release_breakpoint(
    State(manager): State<ManagerState>,
    ApiPath(id): ApiPath<u64>,
) -> ApiResult {
    manager.release_breakpoint(id).await?;
    success(json!({}))
}

async fn execute_breakpoint_script(
    State(manager): State<ManagerState>,
    ApiPath(id): ApiPath<u64>,
    ApiJson(request): ApiJson<ExecuteTemporaryScriptRequest>,
) -> ApiResult {
    success(manager.execute_breakpoint_script(id, request).await?)
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

fn parse_body_side(side: &str) -> Result<BodySide, ApiError> {
    match side {
        "request" => Ok(BodySide::Request),
        "response" => Ok(BodySide::Response),
        _ => Err(ApiError(ManagerError::bad_request(
            "body side must be request or response",
        ))),
    }
}

fn validate_body_query(query: &BodyQuery) -> Result<(), ApiError> {
    if query.decompress && query.max_size.is_some() {
        return Err(ApiError(ManagerError::bad_request(
            "max_size is not supported when decompress=true",
        )));
    }
    if query.max_size == Some(0) {
        Err(ApiError(ManagerError::bad_request(
            "max_size must be greater than zero",
        )))
    } else {
        Ok(())
    }
}

pub(crate) type DynBodyReader = Pin<Box<dyn AsyncRead + Send>>;

pub(crate) async fn body_reader(
    source: &BodySource,
    decompress: bool,
) -> Result<DynBodyReader, ManagerError> {
    let mut reader: DynBodyReader = match &source.data {
        BodySourceData::File(path) => Box::pin(
            tokio::fs::File::open(path)
                .await
                .map_err(|error| ManagerError::new("body_read_failed", error.to_string()))?,
        ),
        BodySourceData::Bytes(bytes) => {
            let stream = stream::once({
                let bytes = Bytes::copy_from_slice(bytes);
                async move { Ok::<Bytes, io::Error>(bytes) }
            });
            Box::pin(StreamReader::new(stream))
        }
    };
    if !decompress {
        return Ok(reader);
    }
    for encoding in source.content_encodings.iter().rev() {
        let buffered = BufReader::new(reader);
        reader = match encoding.as_str() {
            "gzip" => Box::pin(GzipDecoder::new(buffered)),
            "br" => Box::pin(BrotliDecoder::new(buffered)),
            "deflate" => Box::pin(ZlibDecoder::new(buffered)),
            "zstd" => Box::pin(ZstdDecoder::new(buffered)),
            other => {
                return Err(ManagerError::new(
                    "body_decode_failed",
                    format!("unsupported content encoding {other}"),
                ));
            }
        };
    }
    Ok(reader)
}

async fn body_response(
    source: BodySource,
    decompress: bool,
    max_size: Option<u64>,
) -> ManagerResult<Response> {
    if !decompress {
        let max_size = max_size.unwrap_or_else(default_body_max_size);
        if source.stored_size > max_size {
            return Err(ManagerError::body_too_large(source.stored_size, max_size));
        }
    }

    let reader = body_reader(&source, decompress).await?;
    let stream = ReaderStream::new(reader);
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = StatusCode::OK;
    let content_type = source
        .content_type
        .as_deref()
        .and_then(|value| HeaderValue::from_str(value).ok())
        .unwrap_or_else(|| HeaderValue::from_static("application/octet-stream"));
    response.headers_mut().insert(CONTENT_TYPE, content_type);
    if !decompress {
        response.headers_mut().insert(
            CONTENT_LENGTH,
            HeaderValue::from_str(&source.stored_size.to_string())
                .expect("u64 is always a valid Content-Length"),
        );
        if !source.content_encodings.is_empty() {
            response.headers_mut().insert(
                CONTENT_ENCODING,
                HeaderValue::from_str(&source.content_encodings.join(", "))
                    .map_err(|error| ManagerError::new("body_read_failed", error.to_string()))?,
            );
        }
    }
    response.headers_mut().insert(
        "x-proxycrab-body-size",
        HeaderValue::from_str(&source.stored_size.to_string())
            .expect("u64 is always a valid header value"),
    );
    Ok(response)
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
            "bad_request"
            | "unsupported_export_format"
            | "invalid_asset_id"
            | "invalid_asset_format" => StatusCode::BAD_REQUEST,
            "not_found" | "log_not_found" | "body_not_found" | "asset_not_found" => {
                StatusCode::NOT_FOUND
            }
            "forbidden_origin" => StatusCode::FORBIDDEN,
            "conflict"
            | "proxy_running"
            | "session_in_use"
            | "asset_already_exists"
            | "asset_path_conflict" => StatusCode::CONFLICT,
            "body_too_large" => StatusCode::PAYLOAD_TOO_LARGE,
            "body_decode_failed" => StatusCode::UNPROCESSABLE_ENTITY,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (
            status,
            Json(json!({
                "ok": false,
                "error": self.0
            })),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::Write,
        sync::{Arc, Mutex},
    };

    use async_trait::async_trait;
    use axum::{
        body::{Body, to_bytes},
        http::{
            Method, Request, StatusCode,
            header::{
                ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS,
                ACCESS_CONTROL_ALLOW_ORIGIN, AUTHORIZATION,
            },
        },
        response::IntoResponse,
    };
    use flate2::{
        Compression,
        write::{GzEncoder, ZlibEncoder},
    };
    use proxy_crab_mitm::{ProxyCrab, log_buffer::LogBuffer};
    use proxy_crab_mitm::{
        model::{CaptureError, ErrorStage, HeaderValues, RequestData, ResponseData},
        storage::{BodySide, BodySource, BodySourceData, CaptureStore},
    };
    use serde_json::Value;
    use tempfile::tempdir;
    use tower::ServiceExt;

    use crate::{
        dto::HttpApiResource,
        http::{
            ApiError, body_response, change_for_request, router, router_with_changes,
            secured_router,
        },
        manager::{MitmManager, ProxyCrabManager},
        permission::{PermissionAction, PermissionDenied, PermissionManager},
    };

    struct AllowAll;

    #[async_trait]
    impl PermissionManager for AllowAll {
        async fn check_permission(&self, _action: PermissionAction) -> Option<PermissionDenied> {
            None
        }
    }

    fn allow_all() -> Arc<dyn PermissionManager> {
        Arc::new(AllowAll)
    }

    #[derive(Debug, PartialEq, Eq)]
    struct RecordedPermission {
        id: &'static str,
        authorization: Option<String>,
        actual_path: String,
        query: Option<String>,
        body_preview: Option<String>,
        body_preview_truncated: bool,
    }

    struct RecordingPermissions {
        items: Arc<Mutex<Vec<RecordedPermission>>>,
        denied: Option<PermissionDenied>,
    }

    #[async_trait]
    impl PermissionManager for RecordingPermissions {
        async fn check_permission(&self, action: PermissionAction) -> Option<PermissionDenied> {
            self.items.lock().unwrap().push(RecordedPermission {
                id: action.action.id,
                authorization: action.authorization,
                actual_path: action.actual_path,
                query: action.query,
                body_preview: action.body_preview,
                body_preview_truncated: action.body_preview_truncated,
            });
            self.denied.clone()
        }
    }

    fn recording_permissions(
        denied: Option<PermissionDenied>,
    ) -> (
        Arc<dyn PermissionManager>,
        Arc<Mutex<Vec<RecordedPermission>>>,
    ) {
        let items = Arc::new(Mutex::new(Vec::new()));
        (
            Arc::new(RecordingPermissions {
                items: items.clone(),
                denied,
            }),
            items,
        )
    }

    #[tokio::test]
    async fn permission_middleware_records_template_and_preserves_json_body() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let manager = MitmManager::new(runtime);
        let (permissions, recorded) = recording_permissions(None);
        let app = router(manager, permissions);
        let body = r#"{"name":"through approval","description":"complete body"}"#;

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/sessions?source=test")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer pcrab_example")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            *recorded.lock().unwrap(),
            vec![RecordedPermission {
                id: "POST /api/sessions",
                authorization: Some("Bearer pcrab_example".into()),
                actual_path: "/api/sessions".into(),
                query: Some("source=test".into()),
                body_preview: Some(body.into()),
                body_preview_truncated: false,
            }]
        );
    }

    #[tokio::test]
    async fn permission_denial_uses_stable_envelope_and_skips_handler() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        assert!(runtime.sessions().is_empty());
        let manager = MitmManager::new(runtime.clone());
        let (permissions, recorded) = recording_permissions(Some(PermissionDenied::forbidden(
            "permission_denied",
            "this action is blocked",
        )));
        let app = router(manager, permissions);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"must not exist"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["error"]["code"], "permission_denied");
        assert_eq!(recorded.lock().unwrap().len(), 1);
        assert!(runtime.sessions().is_empty());
    }

    #[tokio::test]
    async fn permission_middleware_uses_route_template_for_path_parameters() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let session = runtime.create_session(None, None).unwrap();
        let manager = MitmManager::new(runtime);
        let (permissions, recorded) = recording_permissions(None);
        let app = router(manager, permissions);

        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/sessions/{}", session.id))
                    .method(Method::PUT)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"renamed"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(recorded.lock().unwrap()[0].id, "PUT /api/sessions/{id}");
    }

    #[tokio::test]
    async fn duplicate_authorization_headers_are_rejected_before_permission_check() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let (permissions, recorded) = recording_permissions(None);
        let app = router(MitmManager::new(runtime), permissions);

        let mut request = Request::builder()
            .uri("/api/config")
            .header("authorization", "Bearer first")
            .body(Body::empty())
            .unwrap();
        request
            .headers_mut()
            .append(AUTHORIZATION, "Bearer second".parse().unwrap());
        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(recorded.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn local_cors_preflight_allows_authorization_without_permission_check() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let manager = MitmManager::new(runtime);
        let (permissions, recorded) = recording_permissions(Some(PermissionDenied::forbidden(
            "permission_denied",
            "must not run for preflight",
        )));
        let app = secured_router(manager, permissions);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/api/sessions/42")
                    .header("host", "127.0.0.1:18089")
                    .header("origin", "http://localhost:5173")
                    .header("access-control-request-method", "PUT")
                    .header(
                        "access-control-request-headers",
                        "authorization, content-type",
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            response.headers()[ACCESS_CONTROL_ALLOW_ORIGIN],
            "http://localhost:5173"
        );
        assert!(
            response.headers()[ACCESS_CONTROL_ALLOW_METHODS]
                .to_str()
                .unwrap()
                .contains("PUT")
        );
        assert_eq!(
            response.headers()[ACCESS_CONTROL_ALLOW_HEADERS],
            "Authorization, Content-Type"
        );
        assert!(recorded.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn raw_body_response_defaults_to_stored_bytes_and_optionally_decodes() {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"decoded body").unwrap();
        let compressed = encoder.finish().unwrap();
        let source = BodySource {
            stored_size: compressed.len() as u64,
            data: BodySourceData::Bytes(compressed),
            path: None,
            content_type: Some("text/plain; charset=utf-8".into()),
            content_encodings: vec!["gzip".into()],
        };

        let response = body_response(source.clone(), false, Some(64))
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(
            response.headers()["content-length"],
            source.stored_size.to_string()
        );
        assert_eq!(
            response.headers()["x-proxycrab-body-size"],
            source.stored_size.to_string()
        );
        assert_eq!(response.headers()["content-encoding"], "gzip");
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(body.len() as u64, source.stored_size);

        let response = body_response(source.clone(), true, None).await.unwrap();
        assert!(response.headers().get("content-length").is_none());
        assert!(response.headers().get("content-encoding").is_none());
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(body.as_ref(), b"decoded body");

        let error = body_response(source.clone(), false, Some(4))
            .await
            .unwrap_err();
        assert_eq!(error.code, "body_too_large");
        assert_eq!(error.actual_size, Some(source.stored_size));
        assert_eq!(error.max_size, Some(4));
        let response = ApiError(error).into_response();
        assert_eq!(response.status(), axum::http::StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn server_decompression_supports_brotli_deflate_zstd_and_stacks() {
        let original = b"all encodings";
        let mut brotli = Vec::new();
        {
            let mut writer = brotli::CompressorWriter::new(&mut brotli, 4096, 5, 22);
            writer.write_all(original).unwrap();
        }
        let mut deflate = ZlibEncoder::new(Vec::new(), Compression::default());
        deflate.write_all(original).unwrap();
        let deflate = deflate.finish().unwrap();
        let zstd = zstd::stream::encode_all(original.as_slice(), 1).unwrap();

        for (encoding, bytes) in [("br", brotli), ("deflate", deflate), ("zstd", zstd)] {
            let source = BodySource {
                stored_size: bytes.len() as u64,
                data: BodySourceData::Bytes(bytes),
                path: None,
                content_type: None,
                content_encodings: vec![encoding.into()],
            };
            let response = body_response(source, true, None).await.unwrap();
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            assert_eq!(body.as_ref(), original, "{encoding}");
        }

        let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
        gzip.write_all(original).unwrap();
        let gzip = gzip.finish().unwrap();
        let mut stacked = Vec::new();
        {
            let mut writer = brotli::CompressorWriter::new(&mut stacked, 4096, 5, 22);
            writer.write_all(&gzip).unwrap();
        }
        let source = BodySource {
            stored_size: stacked.len() as u64,
            data: BodySourceData::Bytes(stacked),
            path: None,
            content_type: None,
            content_encodings: vec!["gzip".into(), "br".into()],
        };
        let response = body_response(source, true, None).await.unwrap();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(body.as_ref(), original);
    }

    #[tokio::test]
    async fn log_body_route_returns_the_capture_file() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let session = runtime.create_session(None, None).unwrap();
        let session_dir = std::path::Path::new(&runtime.workspace_paths().current_path)
            .join("sessions")
            .join(session.id.to_string());
        let store = CaptureStore::open(session.id, &session_dir).unwrap();
        let request = RequestData {
            method: "POST".into(),
            uri: "https://example.com/body".into(),
            version: "HTTP/1.1".into(),
            headers: HeaderValues::from([(
                "content-type".into(),
                vec!["application/octet-stream".into()],
            )]),
            tags: Default::default(),
        };
        let id = store.begin("127.0.0.1", &request, "request").unwrap();
        store
            .save_body(id, BodySide::Request, false, b"captured bytes")
            .unwrap();
        let app = router(MitmManager::new(runtime), allow_all());

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/logs/{id}/body?session_id={}&side=request&decompress=false&max_size=64",
                        session.id
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(response.headers()["x-proxycrab-body-size"], "14");
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(body.as_ref(), b"captured bytes");
    }

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
                "POST",
                "/api/sessions/42/archive",
                None,
                vec![HttpApiResource::Sessions, HttpApiResource::ArchivedSessions],
                None,
            ),
            (
                "POST",
                "/api/archived-sessions/42/restore",
                None,
                vec![HttpApiResource::Sessions, HttpApiResource::ArchivedSessions],
                None,
            ),
            (
                "DELETE",
                "/api/archived-sessions/42",
                None,
                vec![HttpApiResource::ArchivedSessions],
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
        let app = router(MitmManager::new(runtime), allow_all());

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
    async fn agents_markdown_is_returned_as_raw_plain_text() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let app = router(MitmManager::new(runtime), allow_all());

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/agents.md")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(
            response.headers()["content-type"],
            "text/plain; charset=utf-8"
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = std::str::from_utf8(&body).unwrap();
        assert!(text.starts_with("# ProxyCrab Agent 行为：充分使用能力"));
        assert!(!text.contains("\"ok\""));
    }

    #[tokio::test]
    async fn active_session_http_api_is_nullable_and_validates_session_ids() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let session = runtime
            .create_session(Some("capture".into()), None)
            .unwrap();
        let app = router(MitmManager::new(runtime), allow_all());

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
    async fn session_archive_routes_replace_direct_session_deletion() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let active = runtime.create_session(Some("active".into()), None).unwrap();
        let archived = runtime
            .create_session(Some("archive-me".into()), Some("kept".into()))
            .unwrap();
        let app = router(MitmManager::new(runtime), allow_all());

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/api/sessions/{}", archived.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/api/sessions/{}/archive", active.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::CONFLICT);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/api/sessions/{}/archive", archived.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/session-view?session_id={}", archived.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/archived-sessions")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["data"][0]["id"], archived.id);
        assert_eq!(body["data"][0]["description"], "kept");

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/api/archived-sessions/{}/restore", archived.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);

        app.clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/api/sessions/{}/archive", archived.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/api/archived-sessions/{}", archived.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn http_and_trait_return_the_same_config_dto() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let manager = MitmManager::new(runtime);
        let expected = serde_json::to_value(manager.config().await.unwrap()).unwrap();
        let app = router(manager, allow_all());

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
        let app = router(MitmManager::new(runtime), allow_all());

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
        let app = router_with_changes(MitmManager::new(runtime), allow_all(), changes);

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
        let app = router_with_changes(MitmManager::new(runtime), allow_all(), changes);
        let body = r#"{"filter":{"option":{"kind":"column","column":{"kind":"uri"},"regex":false},"input":"example"}}"#;

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

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/logs/ids")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"filter":{"option":null,"input":""},"persist_filter":false}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let unexpected = receiver.try_recv();
        assert!(
            matches!(
                unexpected,
                Err(tokio::sync::broadcast::error::TryRecvError::Empty
                    | tokio::sync::broadcast::error::TryRecvError::Closed)
            ),
            "unexpected UI event after stateless filter: {unexpected:?}"
        );
    }

    #[tokio::test]
    async fn invalid_path_uses_the_standard_error_envelope() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let app = router(MitmManager::new(runtime), allow_all());

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
        let app = router(MitmManager::new(runtime), allow_all());

        for (method, uri, body) in [
            ("POST", "/api/logs/ids", r#"{}"#),
            (
                "POST",
                "/api/logs/ids",
                r#"{"filter":{"option":{"kind":"column","column":{"kind":"uri"},"regex":false},"input":""}}"#,
            ),
            ("POST", "/api/logs/views", r#"{"logs":[]}"#),
            ("GET", "/api/session-view", ""),
            ("GET", "/api/active-session", ""),
            ("GET", "/api/breakpoints", ""),
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
        let app = router(MitmManager::new(runtime), allow_all());

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
    async fn export_route_returns_a_complete_har_download() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let session = runtime.create_session(None, None).unwrap();
        let session_dir = std::path::Path::new(&runtime.workspace_paths().current_path)
            .join("sessions")
            .join(session.id.to_string());
        let store = CaptureStore::open(session.id, &session_dir).unwrap();
        let request = RequestData {
            method: "GET".into(),
            uri: "https://example.com/export?q=one&q=two".into(),
            version: "HTTP/1.1".into(),
            headers: HeaderValues::new(),
            tags: [("export".into(), "yes".into())].into(),
        };
        let success = store.begin("127.0.0.1:1234", &request, "request").unwrap();
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"complete response body").unwrap();
        let compressed = encoder.finish().unwrap();
        let response = ResponseData {
            status: 200,
            version: "HTTP/1.1".into(),
            headers: HeaderValues::from([
                ("content-type".into(), vec!["text/plain".into()]),
                ("content-encoding".into(), vec!["gzip".into()]),
            ]),
        };
        store
            .save_body(success, BodySide::Response, false, &compressed)
            .unwrap();
        store.complete(success, &response, &[]).unwrap();
        let failed = store.begin("127.0.0.1:1234", &request, "request").unwrap();
        store
            .fail(
                failed,
                &CaptureError {
                    stage: ErrorStage::Upstream,
                    kind: "test".into(),
                    message: "not exported".into(),
                },
            )
            .unwrap();
        let app = router(MitmManager::new(runtime), allow_all());

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/logs/export")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"format":"har","session_id":{}}}"#,
                        session.id
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(
            response.headers()["content-type"],
            "application/json; charset=utf-8"
        );
        assert_eq!(
            response.headers()["content-disposition"],
            format!(
                "attachment; filename=\"proxycrab-session-{}.har\"",
                session.id
            )
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let har: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let entries = har["log"]["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["_proxyCrab"]["logId"], success);
        assert_eq!(
            entries[0]["response"]["content"]["text"],
            "complete response body"
        );
    }

    #[tokio::test]
    async fn export_route_validates_request_contract() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let session = runtime.create_session(None, None).unwrap();
        let app = router(MitmManager::new(runtime), allow_all());

        for (body, expected_status, expected_code) in [
            (r#"{}"#, axum::http::StatusCode::BAD_REQUEST, "bad_request"),
            (
                r#"{"format":"json"}"#,
                axum::http::StatusCode::BAD_REQUEST,
                "unsupported_export_format",
            ),
            (
                r#"{"format":"har","unknown":true}"#,
                axum::http::StatusCode::BAD_REQUEST,
                "bad_request",
            ),
            (
                r#"{"format":"har","log_ids":[999]}"#,
                axum::http::StatusCode::NOT_FOUND,
                "log_not_found",
            ),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/logs/export")
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected_status);
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(payload["error"]["code"], expected_code);
        }

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/logs/export")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"format":"har","session_id":{},"log_ids":[]}}"#,
                        session.id
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);

        let no_session_data = tempdir().unwrap();
        let no_session_runtime =
            ProxyCrab::open(no_session_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let response = router(MitmManager::new(no_session_runtime), allow_all())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/logs/export")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"format":"har"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn rejects_non_local_host_and_origin() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let manager = MitmManager::new(runtime);
        let app = secured_router(manager, allow_all());

        let response = app
            .clone()
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

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/proxy/status")
                    .header("host", "127.0.0.1:18089")
                    .header("origin", "tauri://localhost")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(
            response.headers()["access-control-allow-origin"],
            "tauri://localhost"
        );
    }

    #[tokio::test]
    async fn uploads_assets_and_returns_metadata_or_raw_bytes() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let app = router(MitmManager::new(runtime), allow_all());

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/assets/fixtures/example.json")
                    .header("content-type", "application/json; charset=utf-8")
                    .body(Body::from(r#"{"ok":true}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let payload: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(payload["data"]["id"], "fixtures/example.json");
        assert_eq!(payload["data"]["size"], 11);
        assert_eq!(
            payload["data"]["content_type"],
            "application/json; charset=utf-8"
        );
        assert_eq!(payload["data"]["sha256"].as_str().unwrap().len(), 64);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/assets/fixtures/example.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let metadata: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(metadata["data"], payload["data"]);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/assets/fixtures/example.json?format=raw")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[axum::http::header::CONTENT_TYPE],
            "application/json; charset=utf-8"
        );
        assert_eq!(response.headers()[axum::http::header::CONTENT_LENGTH], "11");
        assert_eq!(
            to_bytes(response.into_body(), usize::MAX).await.unwrap(),
            r#"{"ok":true}"#
        );

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/assets/fixtures/example.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/assets/fixtures/example.json?format=decoded")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
