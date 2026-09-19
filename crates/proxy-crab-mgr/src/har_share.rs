use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    extract::{RawQuery, Request, State},
    http::{
        HeaderValue, StatusCode,
        header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE, REFERRER_POLICY},
    },
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::{
    ProxyCrabManager,
    dto::{ExportLogsRequest, ManagerError},
};

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HarShareScope {
    All,
    Filtered,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnableHarShareRequest {
    pub session_id: u64,
    pub scope: HarShareScope,
    pub log_ids: Vec<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct HarShareState {
    pub session_id: u64,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<HarShareScope>,
    pub log_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

#[derive(Clone)]
struct HarShareEntry {
    token: String,
    digest: [u8; 32],
    scope: HarShareScope,
    log_ids: Vec<u64>,
}

#[derive(Debug, Clone)]
struct HarDownloadScope {
    session_id: u64,
    log_ids: Vec<u64>,
}

#[derive(Default)]
pub struct HarShareService {
    entries: Mutex<BTreeMap<u64, HarShareEntry>>,
}

impl HarShareService {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    async fn ensure_session(
        manager: &Arc<dyn ProxyCrabManager>,
        session_id: u64,
    ) -> Result<(), ManagerError> {
        if manager
            .sessions()
            .await?
            .iter()
            .any(|session| session.id == session_id)
        {
            Ok(())
        } else {
            Err(ManagerError::not_found("session not found"))
        }
    }

    pub async fn status(
        &self,
        manager: &Arc<dyn ProxyCrabManager>,
        session_id: u64,
    ) -> Result<HarShareState, ManagerError> {
        Self::ensure_session(manager, session_id).await?;
        let entries = self.entries.lock().expect("HAR share lock poisoned");
        Ok(match entries.get(&session_id) {
            Some(entry) => Self::enabled_state(session_id, entry),
            None => Self::disabled_state(session_id),
        })
    }

    pub async fn enable(
        &self,
        manager: &Arc<dyn ProxyCrabManager>,
        request: EnableHarShareRequest,
    ) -> Result<HarShareState, ManagerError> {
        Self::ensure_session(manager, request.session_id).await?;
        let mut entries = self.entries.lock().expect("HAR share lock poisoned");
        if let Some(entry) = entries.get(&request.session_id) {
            return Ok(Self::enabled_state(request.session_id, entry));
        }

        let mut bytes = [0_u8; 32];
        getrandom::fill(&mut bytes).map_err(|error| {
            ManagerError::internal(format!("generate HAR share token: {error}"))
        })?;
        let token = format!("pcrab_har_{}", URL_SAFE_NO_PAD.encode(bytes));
        let entry = HarShareEntry {
            token: token.clone(),
            digest: Sha256::digest(token.as_bytes()).into(),
            scope: request.scope,
            log_ids: request
                .log_ids
                .into_iter()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
        };
        let state = Self::enabled_state(request.session_id, &entry);
        entries.insert(request.session_id, entry);
        Ok(state)
    }

    pub async fn disable(
        &self,
        manager: &Arc<dyn ProxyCrabManager>,
        session_id: u64,
    ) -> Result<HarShareState, ManagerError> {
        Self::ensure_session(manager, session_id).await?;
        self.entries
            .lock()
            .expect("HAR share lock poisoned")
            .remove(&session_id);
        Ok(Self::disabled_state(session_id))
    }

    fn enabled_state(session_id: u64, entry: &HarShareEntry) -> HarShareState {
        HarShareState {
            session_id,
            enabled: true,
            scope: Some(entry.scope),
            log_count: entry.log_ids.len(),
            token: Some(entry.token.clone()),
        }
    }

    fn disabled_state(session_id: u64) -> HarShareState {
        HarShareState {
            session_id,
            enabled: false,
            scope: None,
            log_count: 0,
            token: None,
        }
    }

    fn authenticate(&self, token: &str) -> Result<HarDownloadScope, HarShareAuthError> {
        if !token.starts_with("pcrab_har_") {
            return Err(HarShareAuthError);
        }
        let digest: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        let entries = self.entries.lock().expect("HAR share lock poisoned");
        entries
            .iter()
            .find(|(_, entry)| bool::from(entry.digest.ct_eq(&digest)))
            .map(|(session_id, entry)| HarDownloadScope {
                session_id: *session_id,
                log_ids: entry.log_ids.clone(),
            })
            .ok_or(HarShareAuthError)
    }
}

#[derive(Clone)]
struct HarShareRouterState {
    manager: Arc<dyn ProxyCrabManager>,
    shares: Arc<HarShareService>,
}

pub fn router(manager: Arc<dyn ProxyCrabManager>, shares: Arc<HarShareService>) -> Router {
    Router::new()
        .route("/session.har", get(download))
        .route_layer(middleware::from_fn(no_store))
        .with_state(Arc::new(HarShareRouterState { manager, shares }))
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

fn query_token(query: Option<&str>) -> Option<String> {
    let mut pairs = url::form_urlencoded::parse(query?.as_bytes());
    let (name, token) = pairs.next()?;
    (name == "token" && !token.is_empty() && pairs.next().is_none()).then(|| token.into_owned())
}

async fn download(
    State(state): State<Arc<HarShareRouterState>>,
    RawQuery(query): RawQuery,
) -> Result<Response, HarShareError> {
    let token = query_token(query.as_deref()).ok_or(HarShareAuthError)?;
    let scope = state.shares.authenticate(&token)?;
    HarShareService::ensure_session(&state.manager, scope.session_id)
        .await
        .map_err(HarShareError::Manager)?;
    let export = state
        .manager
        .export_logs(ExportLogsRequest {
            format: "har".into(),
            session_id: Some(scope.session_id),
            log_ids: Some(scope.log_ids),
        })
        .await
        .map_err(HarShareError::Manager)?;
    let mut response = Response::new(axum::body::Body::from_stream(export.body));
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    response.headers_mut().insert(
        CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{}\"", export.filename))
            .map_err(|error| HarShareError::Manager(ManagerError::internal(error.to_string())))?,
    );
    Ok(response)
}

#[derive(Debug)]
struct HarShareAuthError;

enum HarShareError {
    Auth(HarShareAuthError),
    Manager(ManagerError),
}

impl From<HarShareAuthError> for HarShareError {
    fn from(value: HarShareAuthError) -> Self {
        Self::Auth(value)
    }
}

impl IntoResponse for HarShareError {
    fn into_response(self) -> Response {
        match self {
            Self::Auth(_) => (
                StatusCode::UNAUTHORIZED,
                axum::Json(json!({
                    "ok": false,
                    "error": { "code": "invalid_har_share_token", "message": "HAR 分享链接无效" }
                })),
            )
                .into_response(),
            Self::Manager(error) => {
                let status = error.code.http_status();
                (status, axum::Json(json!({ "ok": false, "error": error }))).into_response()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, header::CONTENT_LENGTH},
    };
    use proxy_crab_mitm::{ProxyCrab, log_buffer::LogBuffer};
    use tempfile::tempdir;
    use tower::ServiceExt;

    use crate::{
        MitmManager,
        dto::{ActiveSession, CreateSessionRequest, ErrorCode},
    };

    async fn manager_and_session() -> (tempfile::TempDir, Arc<dyn ProxyCrabManager>, u64) {
        let directory = tempdir().unwrap();
        let runtime = ProxyCrab::open(directory.path(), Arc::new(LogBuffer::default())).unwrap();
        let manager: Arc<dyn ProxyCrabManager> = MitmManager::new(runtime);
        let session = manager
            .create_session(CreateSessionRequest {
                name: Some("HAR share".into()),
                description: None,
            })
            .await
            .unwrap();
        (directory, manager, session.id)
    }

    #[test]
    fn har_share_scope_serializes_for_the_management_contract() {
        assert_eq!(
            serde_json::to_value(HarShareScope::Filtered).unwrap(),
            serde_json::json!("filtered")
        );
    }

    #[tokio::test]
    async fn share_is_frozen_idempotent_and_revocable() {
        let (_directory, manager, session_id) = manager_and_session().await;
        let service = HarShareService::default();
        assert_eq!(
            service.status(&manager, session_id).await.unwrap(),
            HarShareService::disabled_state(session_id)
        );

        let first = service
            .enable(
                &manager,
                EnableHarShareRequest {
                    session_id,
                    scope: HarShareScope::Filtered,
                    log_ids: vec![9, 3, 9],
                },
            )
            .await
            .unwrap();
        let second = service
            .enable(
                &manager,
                EnableHarShareRequest {
                    session_id,
                    scope: HarShareScope::All,
                    log_ids: vec![],
                },
            )
            .await
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(first.scope, Some(HarShareScope::Filtered));
        assert_eq!(first.log_count, 2);
        let token = first.token.unwrap();
        let download = service.authenticate(&token).unwrap();
        assert_eq!(download.session_id, session_id);
        assert_eq!(download.log_ids, vec![3, 9]);

        service.disable(&manager, session_id).await.unwrap();
        assert!(service.authenticate(&token).is_err());
        assert_eq!(
            service.status(&manager, session_id).await.unwrap(),
            HarShareService::disabled_state(session_id)
        );
    }

    #[tokio::test]
    async fn empty_share_downloads_a_valid_har_and_rejects_old_token() {
        let (_directory, manager, session_id) = manager_and_session().await;
        let shares = HarShareService::new();
        let state = shares
            .enable(
                &manager,
                EnableHarShareRequest {
                    session_id,
                    scope: HarShareScope::All,
                    log_ids: vec![],
                },
            )
            .await
            .unwrap();
        let token = state.token.unwrap();
        let app = router(manager.clone(), shares.clone());
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/session.har?token={token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
        assert_eq!(response.headers()[REFERRER_POLICY], "no-referrer");
        assert!(!response.headers().contains_key(CONTENT_LENGTH));
        assert_eq!(
            response.headers()[CONTENT_DISPOSITION],
            format!("attachment; filename=\"proxycrab-session-{session_id}.har\"")
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let har: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(har["log"]["entries"], json!([]));

        shares.disable(&manager, session_id).await.unwrap();
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/session.har?token={token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
    }

    #[tokio::test]
    async fn download_requires_exactly_one_token_query_parameter() {
        let (_directory, manager, _) = manager_and_session().await;
        let app = router(manager, HarShareService::new());

        for uri in [
            "/session.har",
            "/session.har?token=",
            "/session.har?token=one&token=two",
            "/session.har?token=one&extra=value",
        ] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{uri}");
            assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
        }
    }

    #[tokio::test]
    async fn archived_session_is_unavailable_to_an_existing_share() {
        let (_directory, manager, session_id) = manager_and_session().await;
        let shares = HarShareService::new();
        let state = shares
            .enable(
                &manager,
                EnableHarShareRequest {
                    session_id,
                    scope: HarShareScope::All,
                    log_ids: vec![],
                },
            )
            .await
            .unwrap();
        manager
            .replace_active_session(ActiveSession { session_id: None })
            .await
            .unwrap();
        manager.archive_session(session_id).await.unwrap();

        let response = router(manager, shares)
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/session.har?token={}",
                        state.token.as_deref().unwrap()
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
    }

    #[tokio::test]
    async fn unknown_session_cannot_be_shared() {
        let (_directory, manager, _) = manager_and_session().await;
        let error = HarShareService::default()
            .enable(
                &manager,
                EnableHarShareRequest {
                    session_id: u64::MAX,
                    scope: HarShareScope::All,
                    log_ids: vec![],
                },
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::NotFound);
    }
}
