use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{FromRequest, FromRequestParts, Path, Query, Request, State},
    http::{
        StatusCode,
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
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::{
    dto::{
        ColumnInput, CreateSessionRequest, FilterHistoryRequest, FilterLogsRequest,
        InterceptorCreateRequest, InterceptorUpdateRequest, LogsQuery, ManagerError, ScriptRequest,
        SetInterceptorOrderRequest, SetWorkspaceRequest, SystemLogsQuery, UpdateScriptRequest,
        UpdateSessionRequest,
    },
    manager::ProxyCrabManager,
};

type ManagerState = Arc<dyn ProxyCrabManager>;
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
}

impl HttpServerHandle {
    pub fn is_running(&self) -> bool {
        !self.task.is_finished()
    }

    pub async fn shutdown(self) {
        self.cancellation.cancel();
        let _ = self.task.await;
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
    let app = secured_router(manager);
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
    })
}

pub fn router(manager: ManagerState) -> Router {
    Router::new()
        .route("/api/workspace", get(get_workspace).put(set_workspace))
        .route("/api/config", get(get_config).put(replace_config))
        .route("/api/proxy/status", get(proxy_status))
        .route("/api/proxy/start", post(start_proxy))
        .route("/api/proxy/stop", post(stop_proxy))
        .route("/api/sessions", get(sessions).post(create_session))
        .route(
            "/api/sessions/{id}",
            put(update_session).delete(delete_session),
        )
        .route("/api/sessions/{id}/activate", post(activate_session))
        .route("/api/sessions/{session_id}/logs", get(session_logs))
        .route("/api/sessions/{session_id}/logs/{id}", get(session_log))
        .route(
            "/api/sessions/{session_id}/logs/filter",
            post(filter_session_logs),
        )
        .route("/api/logs", get(active_logs))
        .route("/api/logs/filter", post(filter_active_logs))
        .route("/api/logs/{id}", get(active_log))
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
        .route("/api/columns", get(columns).post(append_column))
        .route(
            "/api/columns/{index}",
            put(replace_column).delete(delete_column),
        )
        .route("/api/interceptors/order", put(set_interceptor_order))
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
        .route(
            "/api/interceptors/{kind}/{name}/enable",
            post(enable_interceptor),
        )
        .route(
            "/api/interceptors/{kind}/{name}/disable",
            post(disable_interceptor),
        )
        .route(
            "/api/filter-history",
            get(filter_history)
                .post(add_filter_history)
                .delete(remove_filter_history),
        )
        .route("/api/ca", get(certificate).post(regenerate_certificate))
        .route(
            "/api/system-logs",
            get(system_logs).delete(clear_system_logs),
        )
        .fallback(not_found)
        .with_state(manager)
}

fn secured_router(manager: ManagerState) -> Router {
    router(manager).layer(middleware::from_fn(validate_local_browser_request))
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

async fn activate_session(
    State(manager): State<ManagerState>,
    ApiPath(id): ApiPath<u64>,
) -> ApiResult {
    success(manager.activate_session(id).await?)
}

async fn active_logs(
    State(manager): State<ManagerState>,
    ApiQuery(mut query): ApiQuery<LogsQuery>,
) -> ApiResult {
    query.session_id = None;
    success(manager.logs(query).await?)
}

async fn session_logs(
    State(manager): State<ManagerState>,
    ApiPath(session_id): ApiPath<u64>,
    ApiQuery(mut query): ApiQuery<LogsQuery>,
) -> ApiResult {
    query.session_id = Some(session_id);
    success(manager.logs(query).await?)
}

async fn active_log(State(manager): State<ManagerState>, ApiPath(id): ApiPath<u64>) -> ApiResult {
    success(manager.log(None, id).await?)
}

async fn session_log(
    State(manager): State<ManagerState>,
    ApiPath((session_id, id)): ApiPath<(u64, u64)>,
) -> ApiResult {
    success(manager.log(Some(session_id), id).await?)
}

async fn filter_active_logs(
    State(manager): State<ManagerState>,
    ApiJson(mut request): ApiJson<FilterLogsRequest>,
) -> ApiResult {
    request.session_id = None;
    success(manager.filter_logs(request).await?)
}

async fn filter_session_logs(
    State(manager): State<ManagerState>,
    ApiPath(session_id): ApiPath<u64>,
    ApiJson(mut request): ApiJson<FilterLogsRequest>,
) -> ApiResult {
    request.session_id = Some(session_id);
    success(manager.filter_logs(request).await?)
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

async fn columns(State(manager): State<ManagerState>) -> ApiResult {
    success(manager.columns().await?)
}

async fn append_column(
    State(manager): State<ManagerState>,
    ApiJson(input): ApiJson<ColumnInput>,
) -> ApiResult {
    success(manager.append_column(input).await?)
}

async fn replace_column(
    State(manager): State<ManagerState>,
    ApiPath(index): ApiPath<usize>,
    ApiJson(input): ApiJson<ColumnInput>,
) -> ApiResult {
    success(manager.replace_column(index, input).await?)
}

async fn delete_column(
    State(manager): State<ManagerState>,
    ApiPath(index): ApiPath<usize>,
) -> ApiResult {
    success(manager.delete_column(index).await?)
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

async fn enable_interceptor(
    State(manager): State<ManagerState>,
    ApiPath((kind, name)): ApiPath<(String, String)>,
) -> ApiResult {
    manager
        .set_interceptor_enabled(parse_kind(&kind)?, name, true)
        .await?;
    success(json!({}))
}

async fn disable_interceptor(
    State(manager): State<ManagerState>,
    ApiPath((kind, name)): ApiPath<(String, String)>,
) -> ApiResult {
    manager
        .set_interceptor_enabled(parse_kind(&kind)?, name, false)
        .await?;
    success(json!({}))
}

async fn set_interceptor_order(
    State(manager): State<ManagerState>,
    ApiJson(request): ApiJson<SetInterceptorOrderRequest>,
) -> ApiResult {
    success(manager.set_interceptor_order(request).await?)
}

async fn filter_history(State(manager): State<ManagerState>) -> ApiResult {
    success(manager.filter_history().await?)
}

async fn add_filter_history(
    State(manager): State<ManagerState>,
    ApiJson(request): ApiJson<FilterHistoryRequest>,
) -> ApiResult {
    success(manager.add_filter_history(request.script).await?)
}

#[derive(Deserialize)]
struct FilterHistoryQuery {
    script: Option<String>,
}

async fn remove_filter_history(
    State(manager): State<ManagerState>,
    ApiQuery(query): ApiQuery<FilterHistoryQuery>,
) -> ApiResult {
    success(manager.remove_filter_history(query.script).await?)
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
            "conflict" | "active_session_delete_forbidden" | "session_in_use" => {
                StatusCode::CONFLICT
            }
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
        http::Request,
    };
    use proxy_crab_mitm::{ProxyCrab, log_buffer::LogBuffer};
    use tempfile::tempdir;
    use tower::ServiceExt;

    use crate::{
        dto::ColumnInput,
        http::{router, secured_router},
        manager::{MitmManager, ProxyCrabManager},
    };

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
    async fn columns_keep_legacy_visible_shape_and_width_defaults() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let manager = MitmManager::new(runtime);

        let initial = manager.columns().await.unwrap();
        assert_eq!(initial[0].index, 0);
        assert_eq!(initial[0].key, "method");
        assert_eq!(initial[0].width, 50.0);

        let columns = manager
            .append_column(ColumnInput {
                kind: "source".into(),
                width: None,
                script_name: None,
            })
            .await
            .unwrap();
        assert_eq!(columns.last().unwrap().width, 130.0);
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
