use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    extract::{Extension, Path, Query, Request, State},
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{CACHE_CONTROL, REFERRER_POLICY},
    },
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use proxy_crab_mitm::{model::Script, storage::BodySide};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::{
    ProxyCrabManager,
    dto::{BodyQuery, LogIdsRequest, LogViewsRequest, ManagerError, SessionViewPayload},
    http::body_response,
};

pub const DEFAULT_SHARE_HOURS: u16 = 24;
pub const MAX_SHARE_HOURS: u16 = 720;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreateSessionShareRequest {
    pub session_id: u64,
    pub hours: u16,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreatedSessionShare {
    pub token: String,
    pub session_id: u64,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionShareBootstrap {
    pub session: proxy_crab_mitm::model::SessionMetadata,
    pub view: SessionViewPayload,
    pub proxy_status: SharedProxyStatus,
    pub api_port: u16,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SharedProxyStatus {
    Stopped,
    Starting,
    Running {
        host: String,
        port: u16,
        started_at: u64,
        active_netlog: BTreeMap<u64, Vec<u64>>,
    },
    Stopping,
    Failed {
        message: String,
    },
}

fn scope_proxy_status(
    status: proxy_crab_mitm::model::ProxyStatus,
    session_id: u64,
) -> SharedProxyStatus {
    use proxy_crab_mitm::model::ProxyStatus;

    match status {
        ProxyStatus::Stopped => SharedProxyStatus::Stopped,
        ProxyStatus::Starting => SharedProxyStatus::Starting,
        ProxyStatus::Running {
            host,
            port,
            started_at,
            mut active_netlog,
            ..
        } => {
            active_netlog.retain(|candidate, _| *candidate == session_id);
            SharedProxyStatus::Running {
                host,
                port,
                started_at,
                active_netlog,
            }
        }
        ProxyStatus::Stopping => SharedProxyStatus::Stopping,
        ProxyStatus::Failed { message } => SharedProxyStatus::Failed { message },
    }
}

#[derive(Clone)]
struct ShareEntry {
    digest: [u8; 32],
    session_id: u64,
    expires_at: u64,
    deadline: Instant,
}

#[derive(Debug, Clone)]
struct ShareScope {
    session_id: u64,
    expires_at: u64,
}

#[derive(Default)]
pub struct SessionShareService {
    entries: Mutex<Vec<ShareEntry>>,
}

impl SessionShareService {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub async fn create(
        &self,
        manager: &Arc<dyn ProxyCrabManager>,
        request: CreateSessionShareRequest,
    ) -> Result<CreatedSessionShare, ManagerError> {
        if !(1..=MAX_SHARE_HOURS).contains(&request.hours) {
            return Err(ManagerError::bad_request(format!(
                "share hours must be between 1 and {MAX_SHARE_HOURS}"
            )));
        }
        if !manager
            .sessions()
            .await?
            .iter()
            .any(|session| session.id == request.session_id)
        {
            return Err(ManagerError::not_found("session not found"));
        }
        self.create_with_duration(
            request.session_id,
            Duration::from_secs(u64::from(request.hours) * 60 * 60),
        )
    }

    fn create_with_duration(
        &self,
        session_id: u64,
        duration: Duration,
    ) -> Result<CreatedSessionShare, ManagerError> {
        let mut bytes = [0_u8; 32];
        getrandom::fill(&mut bytes)
            .map_err(|error| ManagerError::internal(format!("generate share token: {error}")))?;
        let token = format!("pcrab_share_{}", URL_SAFE_NO_PAD.encode(bytes));
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| ManagerError::internal(format!("read system clock: {error}")))?;
        let expires_at = now
            .checked_add(duration)
            .ok_or_else(|| ManagerError::bad_request("share expiry is out of range"))?
            .as_millis()
            .try_into()
            .map_err(|_| ManagerError::bad_request("share expiry is out of range"))?;
        let deadline = Instant::now()
            .checked_add(duration)
            .ok_or_else(|| ManagerError::bad_request("share expiry is out of range"))?;
        let entry = ShareEntry {
            digest: Sha256::digest(token.as_bytes()).into(),
            session_id,
            expires_at,
            deadline,
        };
        let mut entries = self.entries.lock().expect("session share lock poisoned");
        entries.retain(|candidate| candidate.deadline > Instant::now());
        entries.push(entry);
        Ok(CreatedSessionShare {
            token,
            session_id,
            expires_at,
        })
    }

    fn authenticate(&self, token: &str) -> Result<ShareScope, ShareAuthError> {
        if !token.starts_with("pcrab_share_") {
            return Err(ShareAuthError::Invalid);
        }
        let digest: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        let now = Instant::now();
        let mut entries = self.entries.lock().expect("session share lock poisoned");
        let matched = entries
            .iter()
            .find(|entry| bool::from(entry.digest.ct_eq(&digest)))
            .cloned();
        entries.retain(|entry| entry.deadline > now);
        match matched {
            Some(entry) if entry.deadline > now => Ok(ShareScope {
                session_id: entry.session_id,
                expires_at: entry.expires_at,
            }),
            Some(_) => Err(ShareAuthError::Expired),
            None => Err(ShareAuthError::Invalid),
        }
    }
}

fn query_token(request: &Request) -> Option<String> {
    let mut tokens = url::form_urlencoded::parse(request.uri().query()?.as_bytes())
        .filter_map(|(name, value)| (name == "token").then_some(value.into_owned()));
    let token = tokens.next()?;
    (!token.is_empty() && tokens.next().is_none()).then_some(token)
}

#[derive(Clone)]
struct ShareState {
    manager: Arc<dyn ProxyCrabManager>,
    shares: Arc<SessionShareService>,
}

pub fn router(manager: Arc<dyn ProxyCrabManager>, shares: Arc<SessionShareService>) -> Router {
    let state = Arc::new(ShareState { manager, shares });
    Router::new()
        .route("/share-api/bootstrap", get(bootstrap))
        .route("/share-api/proxy/status", get(proxy_status))
        .route("/share-api/proxy/changes", get(proxy_changes))
        .route("/share-api/session-view", get(session_view))
        .route("/share-api/logs/ids", post(log_ids))
        .route("/share-api/logs/views", post(log_views))
        .route("/share-api/logs/{id}", get(log))
        .route("/share-api/logs/{id}/body", get(log_body))
        .route("/share-api/column-scripts", get(column_scripts))
        .route("/share-api/filter-scripts", get(filter_scripts))
        .route(
            "/share-api/validate-filter-regex",
            post(validate_filter_regex),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            authorize_share,
        ))
        .layer(middleware::from_fn(no_store))
        .with_state(state)
}

async fn no_store(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert(REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    response
}

async fn authorize_share(
    State(state): State<Arc<ShareState>>,
    mut request: Request,
    next: Next,
) -> Response {
    let Some(token) = query_token(&request) else {
        return ShareAuthError::Invalid.into_response();
    };
    let scope = match state.shares.authenticate(&token) {
        Ok(scope) => scope,
        Err(error) => return error.into_response(),
    };
    match state.manager.sessions().await {
        Ok(sessions)
            if sessions
                .iter()
                .any(|session| session.id == scope.session_id) =>
        {
            request.extensions_mut().insert(scope);
            next.run(request).await
        }
        Ok(_) => ShareApiError::unavailable().into_response(),
        Err(error) => ShareApiError(error).into_response(),
    }
}

async fn bootstrap(
    State(state): State<Arc<ShareState>>,
    Extension(scope): Extension<ShareScope>,
) -> Result<Json<serde_json::Value>, ShareApiError> {
    let session = state
        .manager
        .sessions()
        .await?
        .into_iter()
        .find(|session| session.id == scope.session_id)
        .ok_or_else(ShareApiError::unavailable)?;
    let (view, proxy_status, config) = tokio::try_join!(
        state.manager.session_view(Some(scope.session_id)),
        state.manager.proxy_status(),
        state.manager.config(),
    )?;
    success(SessionShareBootstrap {
        session,
        view,
        proxy_status: scope_proxy_status(proxy_status, scope.session_id),
        api_port: config.api_port,
        expires_at: scope.expires_at,
    })
}

async fn proxy_status(
    State(state): State<Arc<ShareState>>,
    Extension(scope): Extension<ShareScope>,
) -> Result<Json<serde_json::Value>, ShareApiError> {
    success(scope_proxy_status(
        state.manager.proxy_status().await?,
        scope.session_id,
    ))
}

#[derive(Deserialize)]
struct ProxyChangesQuery {
    after: Option<String>,
}

#[derive(Serialize)]
struct SharedProxyChange {
    revision: String,
    changed: bool,
}

fn shared_proxy_revision(status: &SharedProxyStatus) -> Result<String, ManagerError> {
    let encoded = serde_json::to_vec(status)
        .map_err(|error| ManagerError::internal(format!("encode shared proxy status: {error}")))?;
    Ok(URL_SAFE_NO_PAD.encode(Sha256::digest(encoded)))
}

async fn proxy_changes(
    State(state): State<Arc<ShareState>>,
    Extension(scope): Extension<ShareScope>,
    Query(query): Query<ProxyChangesQuery>,
) -> Result<Json<serde_json::Value>, ShareApiError> {
    let mut changes = state.manager.subscribe_proxy_status_changes();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(25);
    loop {
        let status = scope_proxy_status(state.manager.proxy_status().await?, scope.session_id);
        let revision = shared_proxy_revision(&status)?;
        if query.after.as_ref() != Some(&revision) {
            return success(SharedProxyChange {
                revision,
                changed: true,
            });
        }
        if !matches!(
            tokio::time::timeout_at(deadline, changes.changed()).await,
            Ok(Ok(()))
        ) {
            return success(SharedProxyChange {
                revision,
                changed: false,
            });
        }
    }
}

async fn session_view(
    State(state): State<Arc<ShareState>>,
    Extension(scope): Extension<ShareScope>,
) -> Result<Json<serde_json::Value>, ShareApiError> {
    success(state.manager.session_view(Some(scope.session_id)).await?)
}

async fn log_ids(
    State(state): State<Arc<ShareState>>,
    Extension(scope): Extension<ShareScope>,
    Json(mut request): Json<LogIdsRequest>,
) -> Result<Json<serde_json::Value>, ShareApiError> {
    request.session_id = Some(scope.session_id);
    request.persist_filter = false;
    success(state.manager.log_ids(request).await?)
}

async fn log_views(
    State(state): State<Arc<ShareState>>,
    Extension(scope): Extension<ShareScope>,
    Json(mut request): Json<LogViewsRequest>,
) -> Result<Json<serde_json::Value>, ShareApiError> {
    request.session_id = Some(scope.session_id);
    success(state.manager.log_views(request).await?)
}

async fn log(
    State(state): State<Arc<ShareState>>,
    Extension(scope): Extension<ShareScope>,
    Path(id): Path<u64>,
) -> Result<Json<serde_json::Value>, ShareApiError> {
    success(state.manager.log(Some(scope.session_id), id).await?)
}

async fn log_body(
    State(state): State<Arc<ShareState>>,
    Extension(scope): Extension<ShareScope>,
    Path(id): Path<u64>,
    Query(query): Query<BodyQuery>,
    headers: HeaderMap,
) -> Result<Response, ShareApiError> {
    if query.max_size == Some(0) {
        return Err(ShareApiError(ManagerError::bad_request(
            "max_size must be greater than zero",
        )));
    }
    let side = match query.side.as_str() {
        "request" => BodySide::Request,
        "response" => BodySide::Response,
        _ => {
            return Err(ShareApiError(ManagerError::bad_request(
                "body side must be request or response",
            )));
        }
    };
    let source = state
        .manager
        .log_body_source(Some(scope.session_id), id, side)
        .await?;
    body_response(source, &headers, query.max_size)
        .await
        .map_err(ShareApiError)
}

async fn column_scripts(
    State(state): State<Arc<ShareState>>,
) -> Result<Json<serde_json::Value>, ShareApiError> {
    let scripts = state.manager.column_scripts().await?;
    success(script_names(scripts))
}

async fn filter_scripts(
    State(state): State<Arc<ShareState>>,
) -> Result<Json<serde_json::Value>, ShareApiError> {
    let scripts = state.manager.filter_scripts().await?;
    success(script_names(scripts))
}

fn script_names(scripts: Vec<Script>) -> Vec<Script> {
    scripts
        .into_iter()
        .map(|script| Script {
            name: script.name,
            content: String::new(),
        })
        .collect()
}

#[derive(Deserialize)]
struct RegexRequest {
    pattern: String,
}

async fn validate_filter_regex(
    Json(request): Json<RegexRequest>,
) -> Result<Json<serde_json::Value>, ShareApiError> {
    success(
        regex::Regex::new(&request.pattern)
            .err()
            .map(|error| error.to_string()),
    )
}

fn success(value: impl Serialize) -> Result<Json<serde_json::Value>, ShareApiError> {
    Ok(Json(json!({ "ok": true, "data": value })))
}

#[derive(Debug)]
enum ShareAuthError {
    Invalid,
    Expired,
}

impl IntoResponse for ShareAuthError {
    fn into_response(self) -> Response {
        let (code, message) = match self {
            Self::Invalid => ("invalid_share_token", "分享链接无效"),
            Self::Expired => ("share_expired", "分享链接已过期"),
        };
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "ok": false, "error": { "code": code, "message": message } })),
        )
            .into_response()
    }
}

struct ShareApiError(ManagerError);

impl ShareApiError {
    fn unavailable() -> Self {
        Self(ManagerError::new(
            "share_session_unavailable",
            "分享的 Session 已不可用",
        ))
    }
}

impl From<ManagerError> for ShareApiError {
    fn from(value: ManagerError) -> Self {
        Self(value)
    }
}

impl IntoResponse for ShareApiError {
    fn into_response(self) -> Response {
        let status = match self.0.code.as_str() {
            "bad_request" => StatusCode::BAD_REQUEST,
            "not_found" | "log_not_found" | "body_not_found" | "share_session_unavailable" => {
                StatusCode::NOT_FOUND
            }
            "body_too_large" => StatusCode::PAYLOAD_TOO_LARGE,
            "body_decode_failed" => StatusCode::UNPROCESSABLE_ENTITY,
            "not_acceptable_encoding" => StatusCode::NOT_ACCEPTABLE,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(json!({ "ok": false, "error": self.0 }))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use proxy_crab_mitm::{ProxyCrab, log_buffer::LogBuffer};
    use tempfile::tempdir;
    use tower::ServiceExt;

    use crate::{
        MitmManager,
        dto::{ActiveSession, CreateSessionRequest, ScriptRequest},
    };

    #[test]
    fn shared_proxy_status_contains_only_authorized_netlogs() {
        let status = proxy_crab_mitm::model::ProxyStatus::Running {
            host: "127.0.0.1".into(),
            port: 8080,
            started_at: 1,
            active_netlog: BTreeMap::from([(7, vec![1, 2]), (8, vec![3])]),
            active_bypass_count: 4,
        };

        let scoped = scope_proxy_status(status, 7);
        let revision = shared_proxy_revision(&scoped).unwrap();
        let value = serde_json::to_value(&scoped).unwrap();

        assert_eq!(value["active_netlog"]["7"], json!([1, 2]));
        assert!(value["active_netlog"].get("8").is_none());
        assert!(value.get("active_bypass_count").is_none());

        let unrelated_change = scope_proxy_status(
            proxy_crab_mitm::model::ProxyStatus::Running {
                host: "127.0.0.1".into(),
                port: 8080,
                started_at: 1,
                active_netlog: BTreeMap::from([(7, vec![1, 2]), (8, vec![3, 4])]),
                active_bypass_count: 9,
            },
            7,
        );
        assert_eq!(shared_proxy_revision(&unrelated_change).unwrap(), revision);

        let authorized_change = scope_proxy_status(
            proxy_crab_mitm::model::ProxyStatus::Running {
                host: "127.0.0.1".into(),
                port: 8080,
                started_at: 1,
                active_netlog: BTreeMap::from([(7, vec![1])]),
                active_bypass_count: 9,
            },
            7,
        );
        assert_ne!(shared_proxy_revision(&authorized_change).unwrap(), revision);
    }

    #[test]
    fn token_is_scoped_distinct_and_expires() {
        let service = SessionShareService::default();
        let first = service
            .create_with_duration(7, Duration::from_secs(60))
            .unwrap();
        let second = service
            .create_with_duration(7, Duration::from_secs(60))
            .unwrap();
        assert_ne!(first.token, second.token);
        assert_eq!(service.authenticate(&first.token).unwrap().session_id, 7);

        let expired = service.create_with_duration(8, Duration::ZERO).unwrap();
        assert!(matches!(
            service.authenticate(&expired.token),
            Err(ShareAuthError::Expired)
        ));
    }

    #[test]
    fn rejects_malformed_credentials() {
        let service = SessionShareService::default();
        assert!(matches!(
            service.authenticate("pcrab_ui_wrong-kind"),
            Err(ShareAuthError::Invalid)
        ));
        assert!(matches!(
            service.authenticate("Bearer pcrab_share_fake"),
            Err(ShareAuthError::Invalid)
        ));
    }

    #[tokio::test]
    async fn public_creation_validates_duration_and_session() {
        let directory = tempdir().unwrap();
        let runtime = ProxyCrab::open(directory.path(), Arc::new(LogBuffer::default())).unwrap();
        let manager: Arc<dyn ProxyCrabManager> = MitmManager::new(runtime);
        let session = manager
            .create_session(CreateSessionRequest {
                name: Some("shared".into()),
                description: None,
            })
            .await
            .unwrap();
        let service = SessionShareService::default();
        for hours in [0, MAX_SHARE_HOURS + 1] {
            let error = service
                .create(
                    &manager,
                    CreateSessionShareRequest {
                        session_id: session.id,
                        hours,
                    },
                )
                .await
                .unwrap_err();
            assert_eq!(error.code, "bad_request");
        }
        let error = service
            .create(
                &manager,
                CreateSessionShareRequest {
                    session_id: u64::MAX,
                    hours: DEFAULT_SHARE_HOURS,
                },
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "not_found");
    }

    #[tokio::test]
    async fn routes_scope_session_redact_scripts_and_expose_no_writes() {
        let directory = tempdir().unwrap();
        let runtime = ProxyCrab::open(directory.path(), Arc::new(LogBuffer::default())).unwrap();
        let manager: Arc<dyn ProxyCrabManager> = MitmManager::new(runtime);
        let shared = manager
            .create_session(CreateSessionRequest {
                name: Some("shared".into()),
                description: None,
            })
            .await
            .unwrap();
        let other = manager
            .create_session(CreateSessionRequest {
                name: Some("other".into()),
                description: None,
            })
            .await
            .unwrap();
        manager
            .create_column_script(ScriptRequest {
                name: "secret-column".into(),
                content: "return 'secret source'".into(),
            })
            .await
            .unwrap();
        let shares = SessionShareService::new();
        let created = shares
            .create(
                &manager,
                CreateSessionShareRequest {
                    session_id: shared.id,
                    hours: DEFAULT_SHARE_HOURS,
                },
            )
            .await
            .unwrap();
        let app = router(manager.clone(), shares);
        let token =
            url::form_urlencoded::byte_serialize(created.token.as_bytes()).collect::<String>();

        for request in [
            Request::builder()
                .uri("/share-api/bootstrap")
                .header("authorization", format!("Bearer {}", created.token))
                .body(Body::empty())
                .unwrap(),
            Request::builder()
                .uri(format!("/share-api/bootstrap?token={token}&token={token}"))
                .body(Body::empty())
                .unwrap(),
            Request::builder()
                .uri("/share-api/bootstrap?token=")
                .body(Body::empty())
                .unwrap(),
        ] {
            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
        }

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/share-api/session-view?session_id={}&token={token}",
                        other.id
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["data"]["session_id"], shared.id);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/share-api/column-scripts?token={token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["data"][0]["name"], "secret-column");
        assert_eq!(body["data"][0]["content"], "");

        let original_filter = manager.session_view(Some(shared.id)).await.unwrap().filter;
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/share-api/logs/ids?token={token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"session_id":{},"filter":{{"option":{{"kind":"column","column":{{"kind":"uri"}},"regex":false}},"input":"needle"}},"persist_filter":true}}"#,
                        other.id
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            manager.session_view(Some(shared.id)).await.unwrap().filter,
            original_filter
        );

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/share-api/session-view?token={token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"columns":[]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);

        manager
            .replace_active_session(ActiveSession {
                session_id: Some(other.id),
            })
            .await
            .unwrap();
        manager.archive_session(shared.id).await.unwrap();
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/share-api/bootstrap?token={token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/share-api/bootstrap?token=pcrab_share_unknown")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
    }
}
