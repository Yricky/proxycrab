use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path as FilePath, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Json, Router,
    body::Body,
    extract::{Extension, Path, Query, Request, State},
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, REFERRER_POLICY},
    },
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use proxy_crab_mitm::{
    model::{InterceptorScriptContent, Script},
    storage::BodySide,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::{
    ProxyCrabManager,
    dto::{BodyQuery, ErrorCode, LogIdsRequest, LogViewsRequest, ManagerError, SessionViewPayload},
    http::body_response,
};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionShareConfig {
    #[serde(default)]
    pub persistent: bool,
    pub redaction_script: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnableSessionShareRequest {
    pub session_id: u64,
    pub session_share_config: Option<SessionShareConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct SessionShareState {
    pub persistent: bool,
    pub redaction_script: Option<String>,
    pub session_id: u64,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SessionShareSummary {
    pub session_id: u64,
    pub persistent: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SessionSharesPayload {
    pub shares: Vec<SessionShareSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionShareBootstrap {
    pub session: proxy_crab_mitm::model::SessionMetadata,
    pub view: SessionViewPayload,
    pub proxy_status: SharedProxyStatus,
    pub api_port: u16,
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
    persistent: bool,
    redaction_script: Option<String>,
    token: String,
    digest: [u8; 32],
}

#[derive(Debug, Clone)]
struct ShareScope {
    redaction_script: Option<String>,
    session_id: u64,
}

#[derive(Deserialize, Serialize, PartialEq, Eq)]
struct StoredShare {
    session_id: u64,
    token: String,
    redaction_script: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct ShareFile {
    version: u32,
    shares: Vec<StoredShare>,
}

#[derive(Default)]
pub struct SessionShareService {
    path: Option<PathBuf>,
    entries: Mutex<BTreeMap<u64, ShareEntry>>,
}

impl SessionShareService {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn open(workspace: &FilePath) -> Result<Arc<Self>, ManagerError> {
        let path = workspace.join("session-shares.json");
        let mut entries = BTreeMap::new();
        match fs::read(&path) {
            Ok(bytes) => {
                let stored: ShareFile = serde_json::from_slice(&bytes)
                    .map_err(|_| ManagerError::internal("invalid session-shares.json"))?;
                if stored.version != 1 {
                    return Err(ManagerError::internal(
                        "unsupported session-shares.json version",
                    ));
                }
                for share in stored.shares {
                    let valid_token = share
                        .token
                        .strip_prefix("pcrab_share_")
                        .and_then(|value| URL_SAFE_NO_PAD.decode(value).ok())
                        .is_some_and(|bytes| bytes.len() == 32);
                    if !valid_token
                        || entries.contains_key(&share.session_id)
                        || entries
                            .values()
                            .any(|entry: &ShareEntry| entry.token == share.token)
                    {
                        return Err(ManagerError::internal("invalid session-shares.json entry"));
                    }
                    entries.insert(
                        share.session_id,
                        ShareEntry {
                            persistent: true,
                            digest: Sha256::digest(share.token.as_bytes()).into(),
                            token: share.token,
                            redaction_script: share.redaction_script,
                        },
                    );
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(ManagerError::internal(format!(
                    "read session-shares.json: {error}"
                )));
            }
        }
        Ok(Arc::new(Self {
            path: Some(path),
            entries: Mutex::new(entries),
        }))
    }

    // Persist a proposed state before publishing it to readers. Temporary entries never reach disk.
    fn commit(
        &self,
        entries: &mut BTreeMap<u64, ShareEntry>,
        next: BTreeMap<u64, ShareEntry>,
    ) -> Result<(), ManagerError> {
        let persisted = |entries: &BTreeMap<u64, ShareEntry>| {
            entries
                .iter()
                .filter(|(_, entry)| entry.persistent)
                .map(|(id, entry)| StoredShare {
                    session_id: *id,
                    token: entry.token.clone(),
                    redaction_script: entry.redaction_script.clone(),
                })
                .collect::<Vec<_>>()
        };
        let stored = ShareFile {
            version: 1,
            shares: persisted(&next),
        };
        if persisted(entries) != stored.shares {
            let path = self.path.as_ref().ok_or_else(|| {
                ManagerError::internal("persistent sharing requires a workspace store")
            })?;
            let bytes = serde_json::to_vec_pretty(&stored)
                .map_err(|_| ManagerError::internal("encode session shares"))?;
            let temporary = path.with_extension("tmp");
            let result = (|| -> std::io::Result<()> {
                let mut options = OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                let mut file = options.open(&temporary)?;
                file.write_all(&bytes)?;
                file.sync_all()?;
                fs::rename(&temporary, path)?;
                #[cfg(unix)]
                fs::File::open(path.parent().expect("share path has a parent"))?.sync_all()?;
                Ok(())
            })();
            if let Err(error) = result {
                let _ = fs::remove_file(&temporary);
                return Err(ManagerError::internal(format!(
                    "save session-shares.json: {error}"
                )));
            }
        }
        *entries = next;
        Ok(())
    }

    pub async fn delete_archived_session(
        &self,
        manager: &Arc<dyn ProxyCrabManager>,
        session_id: u64,
    ) -> Result<(), ManagerError> {
        manager.delete_archived_session(session_id).await?;
        self.remove(session_id)
    }

    pub fn remove(&self, session_id: u64) -> Result<(), ManagerError> {
        let mut entries = self.entries.lock().expect("session share lock poisoned");
        if !entries.contains_key(&session_id) {
            return Ok(());
        }
        let mut next = entries.clone();
        next.remove(&session_id);
        self.commit(&mut entries, next)
    }

    async fn ensure_session(
        manager: &Arc<dyn ProxyCrabManager>,
        session_id: u64,
    ) -> Result<(), ManagerError> {
        if manager
            .sessions()
            .await?
            .sessions
            .iter()
            .any(|session| session.id == session_id)
        {
            Ok(())
        } else {
            Err(ManagerError::not_found("session not found"))
        }
    }

    pub async fn list(
        &self,
        manager: &Arc<dyn ProxyCrabManager>,
    ) -> Result<SessionSharesPayload, ManagerError> {
        let sessions = manager.sessions().await?.sessions;
        let entries = self.entries.lock().expect("session share lock poisoned");
        let shares = sessions
            .iter()
            .filter_map(|session| {
                entries.get(&session.id).map(|entry| SessionShareSummary {
                    session_id: session.id,
                    persistent: entry.persistent,
                })
            })
            .collect();
        Ok(SessionSharesPayload { shares })
    }

    pub async fn status(
        &self,
        manager: &Arc<dyn ProxyCrabManager>,
        session_id: u64,
    ) -> Result<SessionShareState, ManagerError> {
        Self::ensure_session(manager, session_id).await?;
        let entries = self.entries.lock().expect("session share lock poisoned");
        Ok(match entries.get(&session_id) {
            Some(entry) => SessionShareState {
                persistent: entry.persistent,
                redaction_script: entry.redaction_script.clone(),
                session_id,
                enabled: true,
                token: Some(entry.token.clone()),
            },
            None => SessionShareState {
                persistent: false,
                redaction_script: None,
                session_id,
                enabled: false,
                token: None,
            },
        })
    }

    pub async fn enable(
        &self,
        manager: &Arc<dyn ProxyCrabManager>,
        session_id: u64,
    ) -> Result<SessionShareState, ManagerError> {
        self.enable_with_config(manager, session_id, None).await
    }

    pub async fn enable_with_script(
        &self,
        manager: &Arc<dyn ProxyCrabManager>,
        session_id: u64,
        redaction_script: Option<String>,
    ) -> Result<SessionShareState, ManagerError> {
        self.enable_with_config(
            manager,
            session_id,
            Some(SessionShareConfig {
                redaction_script,
                persistent: false,
            }),
        )
        .await
    }

    pub async fn enable_with_config(
        &self,
        manager: &Arc<dyn ProxyCrabManager>,
        session_id: u64,
        config: Option<SessionShareConfig>,
    ) -> Result<SessionShareState, ManagerError> {
        Self::ensure_session(manager, session_id).await?;
        if let Some(name) = config
            .as_ref()
            .and_then(|config| config.redaction_script.as_ref())
        {
            manager.redaction_script(name.clone()).await?;
        }
        let mut entries = self.entries.lock().expect("session share lock poisoned");
        let mut entry = match entries.get(&session_id) {
            Some(entry) => entry.clone(),
            None => {
                let mut bytes = [0_u8; 32];
                getrandom::fill(&mut bytes).map_err(|error| {
                    ManagerError::internal(format!("generate share token: {error}"))
                })?;
                let token = format!("pcrab_share_{}", URL_SAFE_NO_PAD.encode(bytes));
                ShareEntry {
                    persistent: false,
                    redaction_script: None,
                    digest: Sha256::digest(token.as_bytes()).into(),
                    token,
                }
            }
        };
        if let Some(config) = config {
            entry.persistent = config.persistent;
            entry.redaction_script = config.redaction_script;
        }
        let result = SessionShareState {
            persistent: entry.persistent,
            redaction_script: entry.redaction_script.clone(),
            session_id,
            enabled: true,
            token: Some(entry.token.clone()),
        };
        let mut next = entries.clone();
        next.insert(session_id, entry);
        self.commit(&mut entries, next)?;
        Ok(result)
    }

    pub async fn set_redaction(
        &self,
        manager: &Arc<dyn ProxyCrabManager>,
        session_id: u64,
        script: Option<String>,
    ) -> Result<SessionShareState, ManagerError> {
        if let Some(name) = &script {
            manager.redaction_script(name.clone()).await?;
        }
        {
            let mut entries = self.entries.lock().expect("session share lock poisoned");
            let mut next = entries.clone();
            next.get_mut(&session_id)
                .ok_or_else(|| ManagerError::not_found("share is not enabled"))?
                .redaction_script = script;
            self.commit(&mut entries, next)?;
        }
        self.status(manager, session_id).await
    }

    pub async fn disable(
        &self,
        manager: &Arc<dyn ProxyCrabManager>,
        session_id: u64,
    ) -> Result<SessionShareState, ManagerError> {
        Self::ensure_session(manager, session_id).await?;
        self.remove(session_id)?;
        Ok(SessionShareState {
            persistent: false,
            redaction_script: None,
            session_id,
            enabled: false,
            token: None,
        })
    }

    fn authenticate(&self, token: &str) -> Result<ShareScope, ShareAuthError> {
        if !token.starts_with("pcrab_share_") {
            return Err(ShareAuthError::Invalid);
        }
        let digest: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        let entries = self.entries.lock().expect("session share lock poisoned");
        entries
            .iter()
            .find(|(_, entry)| bool::from(entry.digest.ct_eq(&digest)))
            .map(|(session_id, entry)| ShareScope {
                redaction_script: entry.redaction_script.clone(),
                session_id: *session_id,
            })
            .ok_or(ShareAuthError::Invalid)
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
        .route(
            "/share-api/logs/{id}/websocket/messages",
            get(websocket_messages),
        )
        .route(
            "/share-api/logs/{id}/websocket/messages/{message_id}/body",
            get(websocket_message_body),
        )
        .route(
            "/share-api/logs/{id}/interceptors/{execution_id}/content",
            get(log_interceptor_content),
        )
        .route(
            "/share-api/logs/{id}/interceptors/{execution_id}/snapshot",
            get(log_interceptor_snapshot),
        )
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
                .sessions
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
    let manager = state
        .manager
        .reader(scope.redaction_script.as_deref(), "/share-api/bootstrap")?;
    let session = manager
        .sessions()
        .await?
        .sessions
        .into_iter()
        .find(|session| session.id == scope.session_id)
        .ok_or_else(ShareApiError::unavailable)?;
    let (view, proxy_status, config) = tokio::try_join!(
        manager.session_view(Some(scope.session_id)),
        manager.proxy_status(),
        manager.config(),
    )?;
    success(SessionShareBootstrap {
        session,
        view,
        proxy_status: scope_proxy_status(proxy_status, scope.session_id),
        api_port: config.api_port,
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
    let manager = state
        .manager
        .reader(scope.redaction_script.as_deref(), "/share-api/session-view")?;
    success(manager.session_view(Some(scope.session_id)).await?)
}

async fn log_ids(
    State(state): State<Arc<ShareState>>,
    Extension(scope): Extension<ShareScope>,
    Json(mut request): Json<LogIdsRequest>,
) -> Result<Json<serde_json::Value>, ShareApiError> {
    let manager = state
        .manager
        .reader(scope.redaction_script.as_deref(), "/share-api/logs/ids")?;
    request.session_id = Some(scope.session_id);
    success(manager.log_ids(request).await?)
}

async fn log_views(
    State(state): State<Arc<ShareState>>,
    Extension(scope): Extension<ShareScope>,
    Json(mut request): Json<LogViewsRequest>,
) -> Result<Json<serde_json::Value>, ShareApiError> {
    let manager = state
        .manager
        .reader(scope.redaction_script.as_deref(), "/share-api/logs/views")?;
    request.session_id = Some(scope.session_id);
    success(manager.log_views(request).await?)
}

async fn log(
    State(state): State<Arc<ShareState>>,
    Extension(scope): Extension<ShareScope>,
    Path(id): Path<u64>,
) -> Result<Json<serde_json::Value>, ShareApiError> {
    let manager = state
        .manager
        .reader(scope.redaction_script.as_deref(), "/share-api/logs/{id}")?;
    success(manager.log(Some(scope.session_id), id).await?)
}

async fn websocket_messages(
    State(state): State<Arc<ShareState>>,
    Extension(scope): Extension<ShareScope>,
    Path(id): Path<u64>,
    Query(query): Query<crate::dto::WebSocketQuery>,
) -> Result<Json<serde_json::Value>, ShareApiError> {
    let manager = state.manager.reader(
        scope.redaction_script.as_deref(),
        "/share-api/logs/{id}/websocket/messages",
    )?;
    success(
        manager
            .websocket_messages(
                Some(scope.session_id),
                id,
                query.body_side()?,
                query.after_id,
                query.limit.unwrap_or(500),
            )
            .await?,
    )
}
async fn websocket_message_body(
    State(state): State<Arc<ShareState>>,
    Extension(scope): Extension<ShareScope>,
    Path((id, message_id)): Path<(u64, u32)>,
    Query(query): Query<crate::dto::WebSocketQuery>,
    headers: HeaderMap,
) -> Result<Response, ShareApiError> {
    let manager = state.manager.reader(
        scope.redaction_script.as_deref(),
        "/share-api/logs/{id}/websocket/messages/{message_id}/body",
    )?;
    let source = manager
        .websocket_message_body(
            Some(scope.session_id),
            id,
            query.body_side()?,
            message_id,
            query
                .max_size
                .unwrap_or_else(crate::dto::default_body_max_size),
        )
        .await?;
    let mut response = body_response(source, &headers, query.max_size)
        .await
        .map_err(ShareApiError)?;
    response.headers_mut().insert(
        "cache-control",
        axum::http::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}

async fn log_body(
    State(state): State<Arc<ShareState>>,
    Extension(scope): Extension<ShareScope>,
    Path(id): Path<u64>,
    Query(query): Query<BodyQuery>,
    headers: HeaderMap,
) -> Result<Response, ShareApiError> {
    let manager = state.manager.reader(
        scope.redaction_script.as_deref(),
        "/share-api/logs/{id}/body",
    )?;
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
    let source = match query.execution_id {
        Some(execution_id) => {
            manager
                .interceptor_snapshot_body_source(Some(scope.session_id), id, execution_id, side)
                .await?
        }
        None => {
            manager
                .log_body_source(Some(scope.session_id), id, side)
                .await?
        }
    };
    body_response(source, &headers, query.max_size)
        .await
        .map_err(ShareApiError)
}

async fn log_interceptor_content(
    State(state): State<Arc<ShareState>>,
    Extension(scope): Extension<ShareScope>,
    Path((id, execution_id)): Path<(u64, u64)>,
) -> Result<Response, ShareApiError> {
    let InterceptorScriptContent { hash, content } = state
        .manager
        .interceptor_script_content(Some(scope.session_id), id, execution_id)
        .await?;
    let length = content.len();
    let mut response = Response::new(Body::from(content));
    let headers = response.headers_mut();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    headers.insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&length.to_string()).expect("usize is always a valid Content-Length"),
    );
    headers.insert(
        "x-proxycrab-script-sha256",
        HeaderValue::from_str(&hash).expect("SHA-256 hex is a safe header value"),
    );
    Ok(response)
}

async fn log_interceptor_snapshot(
    State(state): State<Arc<ShareState>>,
    Extension(scope): Extension<ShareScope>,
    Path((id, execution_id)): Path<(u64, u64)>,
) -> Result<Json<serde_json::Value>, ShareApiError> {
    let manager = state.manager.reader(
        scope.redaction_script.as_deref(),
        "/share-api/logs/{id}/interceptors/{execution_id}/snapshot",
    )?;
    success(
        manager
            .interceptor_snapshot(Some(scope.session_id), id, execution_id)
            .await?,
    )
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

fn script_names(scripts: Vec<Script>) -> Vec<serde_json::Value> {
    scripts
        .into_iter()
        .map(|script| {
            let display_name = proxy_crab_mitm::model::script_display_name(&script.content);
            serde_json::json!({ "name": script.name, "content": "", "display_name": display_name })
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
}

impl IntoResponse for ShareAuthError {
    fn into_response(self) -> Response {
        let (code, message) = match self {
            Self::Invalid => ("invalid_share_token", "分享链接无效"),
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
            ErrorCode::ShareSessionUnavailable,
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
        let status = self.0.code.http_status();
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
        dto::{ActiveSession, CreateSessionRequest, ErrorCode, ScriptRequest},
    };

    #[tokio::test]
    async fn share_summary_excludes_credentials_disabled_and_archived_sessions() {
        let directory = tempdir().unwrap();
        let runtime = ProxyCrab::open(directory.path(), Arc::new(LogBuffer::default())).unwrap();
        let manager: Arc<dyn ProxyCrabManager> = MitmManager::new(runtime);
        let first = manager
            .create_session(CreateSessionRequest::default())
            .await
            .unwrap();
        let second = manager
            .create_session(CreateSessionRequest::default())
            .await
            .unwrap();
        let service = SessionShareService::open(directory.path()).unwrap();
        assert!(service.list(&manager).await.unwrap().shares.is_empty());
        service.enable(&manager, first.id).await.unwrap();
        service
            .enable_with_config(
                &manager,
                second.id,
                Some(SessionShareConfig {
                    persistent: true,
                    redaction_script: None,
                }),
            )
            .await
            .unwrap();
        let payload = service.list(&manager).await.unwrap();
        let value = serde_json::to_value(&payload).unwrap();
        assert_eq!(payload.shares.len(), 2);
        for share in value["shares"].as_array().unwrap() {
            assert_eq!(share.as_object().unwrap().len(), 2);
            assert!(share.get("session_id").is_some());
            assert!(share.get("persistent").is_some());
        }
        service.disable(&manager, first.id).await.unwrap();
        manager.archive_session(second.id).await.unwrap();
        assert!(service.list(&manager).await.unwrap().shares.is_empty());
        manager.restore_session(second.id).await.unwrap();
        assert_eq!(
            service.list(&manager).await.unwrap().shares,
            vec![SessionShareSummary {
                session_id: second.id,
                persistent: true
            }]
        );
    }

    #[tokio::test]
    async fn persistent_shares_restore_convert_and_revoke() {
        let directory = tempdir().unwrap();
        let runtime = ProxyCrab::open(directory.path(), Arc::new(LogBuffer::default())).unwrap();
        let manager: Arc<dyn ProxyCrabManager> = MitmManager::new(runtime);
        let session = manager
            .create_session(CreateSessionRequest::default())
            .await
            .unwrap();
        let service = SessionShareService::open(directory.path()).unwrap();
        let temporary = service.enable(&manager, session.id).await.unwrap();
        assert!(!temporary.persistent);
        assert!(
            !SessionShareService::open(directory.path())
                .unwrap()
                .status(&manager, session.id)
                .await
                .unwrap()
                .enabled
        );
        let persistent = service
            .enable_with_config(
                &manager,
                session.id,
                Some(SessionShareConfig {
                    persistent: true,
                    redaction_script: None,
                }),
            )
            .await
            .unwrap();
        assert_eq!(persistent.token, temporary.token);
        assert!(persistent.persistent);
        let restored = SessionShareService::open(directory.path()).unwrap();
        assert_eq!(
            restored.status(&manager, session.id).await.unwrap(),
            persistent
        );
        // Repeated enable must not reset persistence or rotate the token.
        assert_eq!(
            restored.enable(&manager, session.id).await.unwrap(),
            persistent
        );
        restored
            .enable_with_config(
                &manager,
                session.id,
                Some(SessionShareConfig {
                    persistent: false,
                    redaction_script: None,
                }),
            )
            .await
            .unwrap();
        assert!(
            restored
                .authenticate(temporary.token.as_deref().unwrap())
                .is_ok()
        );
        assert!(
            !SessionShareService::open(directory.path())
                .unwrap()
                .status(&manager, session.id)
                .await
                .unwrap()
                .enabled
        );
        restored
            .enable_with_config(
                &manager,
                session.id,
                Some(SessionShareConfig {
                    persistent: true,
                    redaction_script: None,
                }),
            )
            .await
            .unwrap();
        restored.disable(&manager, session.id).await.unwrap();
        let reopened = SessionShareService::open(directory.path()).unwrap();
        assert!(
            reopened
                .authenticate(temporary.token.as_deref().unwrap())
                .is_err()
        );
        let fresh = reopened
            .enable_with_config(
                &manager,
                session.id,
                Some(SessionShareConfig {
                    redaction_script: None,
                    persistent: true,
                }),
            )
            .await
            .unwrap();
        assert_ne!(fresh.token, temporary.token);
        reopened.remove(session.id).unwrap();
        assert!(
            !SessionShareService::open(directory.path())
                .unwrap()
                .status(&manager, session.id)
                .await
                .unwrap()
                .enabled
        );
    }

    #[tokio::test]
    async fn persistence_write_failure_preserves_memory_and_disk() {
        let directory = tempdir().unwrap();
        let runtime = ProxyCrab::open(directory.path(), Arc::new(LogBuffer::default())).unwrap();
        let manager: Arc<dyn ProxyCrabManager> = MitmManager::new(runtime);
        let session = manager
            .create_session(CreateSessionRequest::default())
            .await
            .unwrap();
        let service = SessionShareService::open(directory.path()).unwrap();
        let enabled = service
            .enable_with_config(
                &manager,
                session.id,
                Some(SessionShareConfig {
                    redaction_script: None,
                    persistent: true,
                }),
            )
            .await
            .unwrap();
        let path = directory.path().join("session-shares.json");
        let disk = std::fs::read(&path).unwrap();
        // A directory at the staging path deterministically prevents atomic replacement.
        std::fs::create_dir(path.with_extension("tmp")).unwrap();
        manager
            .create_redaction_script(ScriptRequest {
                name: "mask".into(),
                content: "return true".into(),
            })
            .await
            .unwrap();
        assert!(
            service
                .set_redaction(&manager, session.id, Some("mask".into()))
                .await
                .is_err()
        );
        let other = manager
            .create_session(CreateSessionRequest::default())
            .await
            .unwrap();
        assert!(
            service
                .enable_with_config(
                    &manager,
                    other.id,
                    Some(SessionShareConfig {
                        redaction_script: None,
                        persistent: true
                    })
                )
                .await
                .is_err()
        );
        assert!(!service.status(&manager, other.id).await.unwrap().enabled);
        assert!(service.disable(&manager, session.id).await.is_err());
        assert!(
            service
                .enable_with_config(
                    &manager,
                    session.id,
                    Some(SessionShareConfig {
                        persistent: false,
                        redaction_script: None
                    })
                )
                .await
                .is_err()
        );
        assert_eq!(service.status(&manager, session.id).await.unwrap(), enabled);
        assert_eq!(std::fs::read(path).unwrap(), disk);
    }

    #[tokio::test]
    async fn persistent_redaction_and_archive_lifecycle() {
        let directory = tempdir().unwrap();
        let runtime = ProxyCrab::open(directory.path(), Arc::new(LogBuffer::default())).unwrap();
        let manager: Arc<dyn ProxyCrabManager> = MitmManager::new(runtime);
        let session = manager
            .create_session(CreateSessionRequest::default())
            .await
            .unwrap();
        manager
            .create_redaction_script(ScriptRequest {
                name: "mask".into(),
                content: "return true".into(),
            })
            .await
            .unwrap();
        let service = SessionShareService::open(directory.path()).unwrap();
        let enabled = service
            .enable_with_config(
                &manager,
                session.id,
                Some(SessionShareConfig {
                    redaction_script: Some("mask".into()),
                    persistent: true,
                }),
            )
            .await
            .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(directory.path().join("session-shares.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        let restored = SessionShareService::open(directory.path()).unwrap();
        assert_eq!(
            restored.status(&manager, session.id).await.unwrap(),
            enabled
        );
        restored
            .set_redaction(&manager, session.id, None)
            .await
            .unwrap();
        assert_eq!(
            SessionShareService::open(directory.path())
                .unwrap()
                .status(&manager, session.id)
                .await
                .unwrap()
                .redaction_script,
            None
        );
        restored
            .set_redaction(&manager, session.id, Some("mask".into()))
            .await
            .unwrap();
        assert_eq!(
            SessionShareService::open(directory.path())
                .unwrap()
                .status(&manager, session.id)
                .await
                .unwrap()
                .redaction_script
                .as_deref(),
            Some("mask")
        );
        // A rejected delete of a live Session must not revoke its share.
        assert!(
            restored
                .delete_archived_session(&manager, session.id)
                .await
                .is_err()
        );
        assert!(
            restored
                .authenticate(enabled.token.as_deref().unwrap())
                .is_ok()
        );
        manager.archive_session(session.id).await.unwrap();
        let restored = SessionShareService::open(directory.path()).unwrap();
        let app = router(manager.clone(), restored.clone());
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/share-api/bootstrap?token={}",
                        enabled.token.as_deref().unwrap()
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        manager.restore_session(session.id).await.unwrap();
        assert_eq!(
            restored.status(&manager, session.id).await.unwrap(),
            enabled
        );
        manager.archive_session(session.id).await.unwrap();
        let blocked = directory
            .path()
            .join("sessions_archived")
            .join(format!(".deleted-{}", session.id));
        fs::create_dir_all(&blocked).unwrap();
        fs::write(blocked.join("block"), "block").unwrap();
        assert!(
            restored
                .delete_archived_session(&manager, session.id)
                .await
                .is_err()
        );
        assert!(
            restored
                .authenticate(enabled.token.as_deref().unwrap())
                .is_ok()
        );
        fs::remove_dir_all(blocked).unwrap();
        restored
            .delete_archived_session(&manager, session.id)
            .await
            .unwrap();
        let disk = SessionShareService::open(directory.path()).unwrap();
        assert!(
            disk.authenticate(enabled.token.as_deref().unwrap())
                .is_err()
        );
        assert!(manager.archived_sessions().await.unwrap().is_empty());
    }

    #[test]
    fn share_config_rejects_obsolete_or_misspelled_fields() {
        for value in [
            json!({"session_id":3,"redaction_script":"mask"}),
            json!({"session_id":3,"persistent":true}),
            json!({"session_id":3,"session_share_config":{"redactionScript":"mask","persistent":true}}),
        ] {
            assert!(serde_json::from_value::<EnableSessionShareRequest>(value).is_err());
        }
        let request: EnableSessionShareRequest = serde_json::from_value(json!({"session_id":3,"session_share_config":{"redaction_script":"mask","persistent":true}})).unwrap();
        let config = request.session_share_config.unwrap();
        assert!(config.persistent);
        assert_eq!(config.redaction_script.as_deref(), Some("mask"));
    }

    #[test]
    fn corrupt_persistent_shares_are_not_overwritten() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("session-shares.json");
        std::fs::write(&path, b"broken").unwrap();
        assert!(SessionShareService::open(directory.path()).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"broken");
    }

    #[tokio::test]
    async fn shared_body_selects_snapshot_and_preserves_session_scope_and_limits() {
        use proxy_crab_mitm::{
            model::{
                BodySourceType, HeaderValues, InterceptorExecutionOrigin, InterceptorKind,
                InterceptorRun, Modification, NewSession, RequestData, RequestInterceptorSnapshot,
            },
            storage::{BodySide, CaptureStore},
        };
        let directory = tempdir().unwrap();
        let runtime = ProxyCrab::open(directory.path(), Arc::new(LogBuffer::default())).unwrap();
        let shared = runtime.create_session(NewSession::default()).unwrap();
        let other = runtime.create_session(NewSession::default()).unwrap();
        let request = RequestData {
            method: "POST".into(),
            uri: "https://example.com/body".into(),
            version: "HTTP/1.1".into(),
            headers: HeaderValues::new(),
            tags: Default::default(),
        };
        let mut selected = (0, 0);
        for (session, content) in [(&shared, "snapshot bytes"), (&other, "private bytes")] {
            let store = CaptureStore::open(session.id, runtime.workspace().root()).unwrap();
            let id = store.begin("127.0.0.1", &request, "request").unwrap();
            store
                .save_body(id, BodySide::Request, b"final bytes")
                .unwrap();
            let run = InterceptorRun {
                origin: InterceptorExecutionOrigin::Saved,
                completed: true,
                phase: InterceptorKind::Request,
                position: 0,
                name: "snapshot".into(),
                script_hash: "hash".into(),
                content: "-- snapshot".into(),
                error: None,
                modifications: vec![Modification::Snapshot {
                    request: Some(RequestInterceptorSnapshot {
                        method: request.method.clone(),
                        uri: request.uri.clone(),
                        version: request.version.clone(),
                        headers: HeaderValues::new(),
                        body: BodySourceType::String {
                            content: content.into(),
                        },
                    }),
                    response: None,
                }],
            };
            let execution = store.begin_interceptor_run(id, &run).unwrap();
            if session.id == shared.id {
                selected = (id, execution);
            }
        }
        let manager: Arc<dyn ProxyCrabManager> = MitmManager::new(runtime);
        let shares = SessionShareService::new();
        let token = shares
            .enable(&manager, shared.id)
            .await
            .unwrap()
            .token
            .unwrap();
        let app = router(manager, shares);
        let (id, execution) = selected;
        let base = format!(
            "/share-api/logs/{id}/body?side=request&session_id={}&token={token}",
            other.id
        );
        for (query, status, expected) in [
            (String::new(), StatusCode::OK, "final bytes"),
            (
                format!("&execution_id={execution}"),
                StatusCode::OK,
                "snapshot bytes",
            ),
            (
                format!("&execution_id={execution}&max_size=1"),
                StatusCode::PAYLOAD_TOO_LARGE,
                "body_too_large",
            ),
            (
                "&execution_id=99999".into(),
                StatusCode::NOT_FOUND,
                "snapshot_body_not_found",
            ),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("{base}{query}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), status);
            assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            if status == StatusCode::OK {
                assert_eq!(body.as_ref(), expected.as_bytes());
            } else {
                let error: serde_json::Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(error["error"]["code"], expected);
            }
        }
    }

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
    async fn share_is_idempotent_revocable_and_process_local() {
        let directory = tempdir().unwrap();
        let runtime = ProxyCrab::open(directory.path(), Arc::new(LogBuffer::default())).unwrap();
        let manager: Arc<dyn ProxyCrabManager> = MitmManager::new(runtime);
        let session = manager
            .create_session(CreateSessionRequest {
                name: Some("shared".into()),
                description: None,
                ..Default::default()
            })
            .await
            .unwrap();
        let service = SessionShareService::default();
        assert_eq!(
            service.status(&manager, session.id).await.unwrap(),
            SessionShareState {
                persistent: false,
                redaction_script: None,
                session_id: session.id,
                enabled: false,
                token: None,
            }
        );

        let first = service.enable(&manager, session.id).await.unwrap();
        let second = service.enable(&manager, session.id).await.unwrap();
        assert!(first.enabled);
        assert_eq!(first, second);
        let token = first.token.as_deref().unwrap();
        assert_eq!(service.authenticate(token).unwrap().session_id, session.id);
        assert_eq!(service.status(&manager, session.id).await.unwrap(), first);

        assert_eq!(
            service.disable(&manager, session.id).await.unwrap(),
            SessionShareState {
                persistent: false,
                redaction_script: None,
                session_id: session.id,
                enabled: false,
                token: None,
            }
        );
        assert!(matches!(
            service.authenticate(token),
            Err(ShareAuthError::Invalid)
        ));
        assert!(
            !SessionShareService::default()
                .status(&manager, session.id)
                .await
                .unwrap()
                .enabled
        );

        let error = service.enable(&manager, u64::MAX).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::NotFound);
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
                ..Default::default()
            })
            .await
            .unwrap();
        let other = manager
            .create_session(CreateSessionRequest {
                name: Some("other".into()),
                description: None,
                ..Default::default()
            })
            .await
            .unwrap();
        manager
            .create_column_script(ScriptRequest {
                name: "secret-column".into(),
                content: "-- #display_name: 鉴权token\nreturn 'secret source'".into(),
            })
            .await
            .unwrap();
        let shares = SessionShareService::new();
        let created = shares.enable(&manager, shared.id).await.unwrap();
        let app = router(manager.clone(), shares);
        let token = url::form_urlencoded::byte_serialize(created.token.unwrap().as_bytes())
            .collect::<String>();

        for request in [
            Request::builder()
                .uri("/share-api/bootstrap")
                .header("authorization", "Bearer pcrab_share_wrong-transport")
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
        assert_eq!(body["data"][0]["display_name"], "鉴权token");
        assert!(!body.to_string().contains("secret source"));

        let original_filter = manager.session_view(Some(shared.id)).await.unwrap().filter;
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/share-api/logs/ids?token={token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"session_id":{},"filter":{{"option":{{"kind":"column","column":{{"kind":"uri"}},"regex":false}},"input":"needle"}}}}"#,
                        other.id
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["data"]["matched_ids"], json!([]));
        assert_eq!(body["data"]["in_progress_ids"], json!([]));
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
