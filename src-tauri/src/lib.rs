mod skill_install;

use std::sync::{Arc, Mutex, RwLock};

use proxy_crab_mgr::{
    MitmManager, ProxyCrabManager,
    dto::{
        ActiveSession, BypassPage, BypassQuery, CertificateResponse, CreateSessionRequest,
        DebugFilterScriptRequest, DeleteCount, HttpApiChange, HttpApiResource, HttpServiceStatus,
        InterceptorCreateRequest, InterceptorDetail, InterceptorLibraryList,
        InterceptorUpdateRequest, LogDetail, LogIdsPayload, LogIdsRequest, LogViewsPayload,
        LogViewsRequest, ManagerError, ReplaceSessionInterceptorsRequest,
        ReplaceSessionViewRequest, RoutingSelection, ScriptRequest, SessionInterceptorsPayload,
        SessionViewPayload, SystemLogsQuery, UpdateScriptRequest, UpdateSessionRequest,
    },
    http::{HttpServerHandle, start_http_server},
};
use proxy_crab_mitm::{
    ProxyCrab,
    log_buffer::{BufferLayer, LogBuffer},
    model::{
        AppConfig, InterceptorKind, ProxyStatus, Script, SessionMetadata, SystemLogEntry,
        WorkspacePaths,
    },
};
use tauri::{Emitter, Manager, RunEvent, State};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::skill_install::SkillInstallInfo;

struct BackendState {
    manager: Arc<dyn ProxyCrabManager>,
    http: Mutex<Option<HttpServerHandle>>,
    http_error: RwLock<Option<String>>,
}

impl BackendState {
    fn manager(&self) -> Arc<dyn ProxyCrabManager> {
        self.manager.clone()
    }
}

#[tauri::command]
async fn get_workspace(state: State<'_, BackendState>) -> Result<WorkspacePaths, ManagerError> {
    state.manager().workspace().await
}

#[tauri::command]
async fn set_workspace_for_next_start(
    state: State<'_, BackendState>,
    path: String,
) -> Result<WorkspacePaths, ManagerError> {
    state.manager().set_workspace_for_next_start(path).await
}

#[tauri::command]
async fn get_config(state: State<'_, BackendState>) -> Result<AppConfig, ManagerError> {
    state.manager().config().await
}

#[tauri::command]
async fn replace_config(
    state: State<'_, BackendState>,
    config: AppConfig,
) -> Result<AppConfig, ManagerError> {
    state.manager().replace_config(config).await
}

#[tauri::command]
async fn get_proxy_status(state: State<'_, BackendState>) -> Result<ProxyStatus, ManagerError> {
    state.manager().proxy_status().await
}

#[tauri::command]
async fn start_proxy(state: State<'_, BackendState>) -> Result<ProxyStatus, ManagerError> {
    state.manager().start_proxy().await
}

#[tauri::command]
async fn stop_proxy(state: State<'_, BackendState>) -> Result<ProxyStatus, ManagerError> {
    state.manager().stop_proxy().await
}

#[tauri::command]
async fn list_sessions(
    state: State<'_, BackendState>,
) -> Result<Vec<SessionMetadata>, ManagerError> {
    state.manager().sessions().await
}

#[tauri::command]
async fn get_active_session(state: State<'_, BackendState>) -> Result<ActiveSession, ManagerError> {
    state.manager().active_session().await
}

#[tauri::command]
async fn replace_active_session(
    state: State<'_, BackendState>,
    active: ActiveSession,
) -> Result<ActiveSession, ManagerError> {
    state.manager().replace_active_session(active).await
}

#[tauri::command]
async fn create_session(
    state: State<'_, BackendState>,
    request: CreateSessionRequest,
) -> Result<SessionMetadata, ManagerError> {
    state.manager().create_session(request).await
}

#[tauri::command]
async fn update_session(
    state: State<'_, BackendState>,
    id: u64,
    request: UpdateSessionRequest,
) -> Result<SessionMetadata, ManagerError> {
    state.manager().update_session(id, request).await
}

#[tauri::command]
async fn delete_session(state: State<'_, BackendState>, id: u64) -> Result<(), ManagerError> {
    state.manager().delete_session(id).await
}

#[tauri::command]
async fn get_log_ids(
    state: State<'_, BackendState>,
    request: LogIdsRequest,
) -> Result<LogIdsPayload, ManagerError> {
    state.manager().log_ids(request).await
}

#[tauri::command]
async fn get_log_views(
    state: State<'_, BackendState>,
    request: LogViewsRequest,
) -> Result<LogViewsPayload, ManagerError> {
    state.manager().log_views(request).await
}

#[tauri::command]
async fn get_log(
    state: State<'_, BackendState>,
    session_id: Option<u64>,
    id: u64,
) -> Result<LogDetail, ManagerError> {
    state.manager().log(session_id, id).await
}

#[tauri::command]
async fn get_session_view(
    state: State<'_, BackendState>,
    session_id: Option<u64>,
) -> Result<SessionViewPayload, ManagerError> {
    state.manager().session_view(session_id).await
}

#[tauri::command]
async fn replace_session_view(
    state: State<'_, BackendState>,
    session_id: Option<u64>,
    request: ReplaceSessionViewRequest,
) -> Result<SessionViewPayload, ManagerError> {
    state
        .manager()
        .replace_session_view(session_id, request)
        .await
}

#[tauri::command]
async fn list_column_scripts(state: State<'_, BackendState>) -> Result<Vec<Script>, ManagerError> {
    state.manager().column_scripts().await
}

#[tauri::command]
async fn create_column_script(
    state: State<'_, BackendState>,
    request: ScriptRequest,
) -> Result<(), ManagerError> {
    state.manager().create_column_script(request).await
}

#[tauri::command]
async fn get_column_script(
    state: State<'_, BackendState>,
    name: String,
) -> Result<Script, ManagerError> {
    state.manager().column_script(name).await
}

#[tauri::command]
async fn update_column_script(
    state: State<'_, BackendState>,
    name: String,
    request: UpdateScriptRequest,
) -> Result<(), ManagerError> {
    state.manager().update_column_script(name, request).await
}

#[tauri::command]
async fn delete_column_script(
    state: State<'_, BackendState>,
    name: String,
) -> Result<(), ManagerError> {
    state.manager().delete_column_script(name).await
}

#[tauri::command]
async fn list_filter_scripts(state: State<'_, BackendState>) -> Result<Vec<Script>, ManagerError> {
    state.manager().filter_scripts().await
}

#[tauri::command]
async fn create_filter_script(
    state: State<'_, BackendState>,
    request: ScriptRequest,
) -> Result<(), ManagerError> {
    state.manager().create_filter_script(request).await
}

#[tauri::command]
async fn get_filter_script(
    state: State<'_, BackendState>,
    name: String,
) -> Result<Script, ManagerError> {
    state.manager().filter_script(name).await
}

#[tauri::command]
async fn update_filter_script(
    state: State<'_, BackendState>,
    name: String,
    request: UpdateScriptRequest,
) -> Result<(), ManagerError> {
    state.manager().update_filter_script(name, request).await
}

#[tauri::command]
async fn delete_filter_script(
    state: State<'_, BackendState>,
    name: String,
) -> Result<(), ManagerError> {
    state.manager().delete_filter_script(name).await
}

#[tauri::command]
async fn debug_filter_script(
    state: State<'_, BackendState>,
    name: String,
    request: DebugFilterScriptRequest,
) -> Result<bool, ManagerError> {
    state.manager().debug_filter_script(name, request).await
}

#[tauri::command]
async fn list_routing_scripts(state: State<'_, BackendState>) -> Result<Vec<Script>, ManagerError> {
    state.manager().routing_scripts().await
}

#[tauri::command]
async fn create_routing_script(
    state: State<'_, BackendState>,
    request: ScriptRequest,
) -> Result<(), ManagerError> {
    state.manager().create_routing_script(request).await
}

#[tauri::command]
async fn get_routing_script(
    state: State<'_, BackendState>,
    name: String,
) -> Result<Script, ManagerError> {
    state.manager().routing_script(name).await
}

#[tauri::command]
async fn update_routing_script(
    state: State<'_, BackendState>,
    name: String,
    request: UpdateScriptRequest,
) -> Result<(), ManagerError> {
    state.manager().update_routing_script(name, request).await
}

#[tauri::command]
async fn delete_routing_script(
    state: State<'_, BackendState>,
    name: String,
) -> Result<(), ManagerError> {
    state.manager().delete_routing_script(name).await
}

#[tauri::command]
async fn get_routing_selection(
    state: State<'_, BackendState>,
) -> Result<RoutingSelection, ManagerError> {
    state.manager().routing_selection().await
}

#[tauri::command]
async fn replace_routing_selection(
    state: State<'_, BackendState>,
    selection: RoutingSelection,
) -> Result<RoutingSelection, ManagerError> {
    state.manager().replace_routing_selection(selection).await
}

#[tauri::command]
async fn list_interceptors(
    state: State<'_, BackendState>,
    kind: InterceptorKind,
) -> Result<InterceptorLibraryList, ManagerError> {
    state.manager().interceptors(kind).await
}

#[tauri::command]
async fn create_interceptor(
    state: State<'_, BackendState>,
    request: InterceptorCreateRequest,
) -> Result<(), ManagerError> {
    state.manager().create_interceptor(request).await
}

#[tauri::command]
async fn get_interceptor(
    state: State<'_, BackendState>,
    kind: InterceptorKind,
    name: String,
) -> Result<InterceptorDetail, ManagerError> {
    state.manager().interceptor(kind, name).await
}

#[tauri::command]
async fn update_interceptor(
    state: State<'_, BackendState>,
    kind: InterceptorKind,
    name: String,
    request: InterceptorUpdateRequest,
) -> Result<(), ManagerError> {
    state
        .manager()
        .update_interceptor(kind, name, request)
        .await
}

#[tauri::command]
async fn delete_interceptor(
    state: State<'_, BackendState>,
    kind: InterceptorKind,
    name: String,
) -> Result<(), ManagerError> {
    state.manager().delete_interceptor(kind, name).await
}

#[tauri::command]
async fn get_session_interceptors(
    state: State<'_, BackendState>,
    session_id: Option<u64>,
) -> Result<SessionInterceptorsPayload, ManagerError> {
    state.manager().session_interceptors(session_id).await
}

#[tauri::command]
async fn replace_session_interceptors(
    state: State<'_, BackendState>,
    session_id: Option<u64>,
    request: ReplaceSessionInterceptorsRequest,
) -> Result<SessionInterceptorsPayload, ManagerError> {
    state
        .manager()
        .replace_session_interceptors(session_id, request)
        .await
}

#[tauri::command]
async fn get_certificate(
    state: State<'_, BackendState>,
) -> Result<CertificateResponse, ManagerError> {
    state.manager().certificate().await
}

#[tauri::command]
async fn regenerate_certificate(
    state: State<'_, BackendState>,
) -> Result<CertificateResponse, ManagerError> {
    state.manager().regenerate_certificate().await
}

#[tauri::command]
async fn get_system_logs(
    state: State<'_, BackendState>,
    query: SystemLogsQuery,
) -> Result<Vec<SystemLogEntry>, ManagerError> {
    state.manager().system_logs(query).await
}

#[tauri::command]
async fn clear_system_logs(state: State<'_, BackendState>) -> Result<(), ManagerError> {
    state.manager().clear_system_logs().await
}

#[tauri::command]
async fn get_bypass_entries(
    state: State<'_, BackendState>,
    query: BypassQuery,
) -> Result<BypassPage, ManagerError> {
    state.manager().bypass_entries(query).await
}

#[tauri::command]
async fn delete_bypass_entry(state: State<'_, BackendState>, id: u64) -> Result<(), ManagerError> {
    state.manager().delete_bypass_entry(id).await
}

#[tauri::command]
async fn delete_bypass_entries(
    state: State<'_, BackendState>,
    ids: Vec<u64>,
) -> Result<DeleteCount, ManagerError> {
    state.manager().delete_bypass_entries(ids).await
}

#[tauri::command]
async fn clear_bypass_entries(state: State<'_, BackendState>) -> Result<DeleteCount, ManagerError> {
    state.manager().clear_bypass_entries().await
}

#[tauri::command]
fn get_http_service_error(state: State<'_, BackendState>) -> Option<String> {
    state
        .http_error
        .read()
        .expect("HTTP status lock poisoned")
        .clone()
}

#[tauri::command]
fn get_proxycrab_skill_install_info(parent: String) -> Result<SkillInstallInfo, ManagerError> {
    skill_install::install_info(&parent)
}

#[tauri::command]
fn install_proxycrab_skill(
    app: tauri::AppHandle,
    parent: String,
    overwrite: bool,
) -> Result<SkillInstallInfo, ManagerError> {
    let bundled = app
        .path()
        .resource_dir()
        .map_err(|error| ManagerError::internal(format!("resolve resource directory: {error}")))?
        .join("skills")
        .join("proxycrab");
    #[cfg(debug_assertions)]
    let bundled = if bundled.is_dir() {
        bundled
    } else {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("skills")
            .join("proxycrab")
    };
    skill_install::install(&bundled, &parent, overwrite)
}

#[tauri::command]
async fn get_http_service_status(
    state: State<'_, BackendState>,
) -> Result<HttpServiceStatus, ManagerError> {
    let config = state.manager().config().await?;
    let running = state
        .http
        .lock()
        .expect("HTTP service lock poisoned")
        .as_ref()
        .is_some_and(HttpServerHandle::is_running);
    let mut error = state
        .http_error
        .read()
        .expect("HTTP status lock poisoned")
        .clone();
    if !running && error.is_none() {
        error = Some("management HTTP service is not running".into());
    }
    Ok(HttpServiceStatus {
        running,
        host: config.api_host,
        port: config.api_port,
        error,
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            let log_buffer = Arc::new(LogBuffer::default());
            let _ = tracing_subscriber::registry()
                .with(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
                )
                .with(tracing_subscriber::fmt::layer())
                .with(BufferLayer::new(log_buffer.clone()))
                .try_init();
            let runtime = ProxyCrab::open(app_data_dir, log_buffer)?;
            let manager: Arc<dyn ProxyCrabManager> = MitmManager::new(runtime);
            let (http, http_error) =
                match tauri::async_runtime::block_on(start_http_server(manager.clone())) {
                    Ok(handle) => (Some(handle), None),
                    Err(error) => {
                        tracing::error!("management HTTP service did not start: {error}");
                        (None, Some(error.to_string()))
                    }
                };
            if let Some(http) = http.as_ref() {
                let mut changes = http.subscribe_changes();
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        let change = match changes.recv().await {
                            Ok(change) => change,
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                HttpApiChange {
                                    resources: vec![HttpApiResource::All],
                                    session_id: None,
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        };
                        if let Err(error) = app_handle.emit("proxycrab://http-api-change", change) {
                            tracing::warn!(
                                "failed to emit HTTP API change to the frontend: {error}"
                            );
                        }
                    }
                });
            }
            app.manage(BackendState {
                manager,
                http: Mutex::new(http),
                http_error: RwLock::new(http_error),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_workspace,
            set_workspace_for_next_start,
            get_config,
            replace_config,
            get_proxy_status,
            start_proxy,
            stop_proxy,
            list_sessions,
            get_active_session,
            replace_active_session,
            create_session,
            update_session,
            delete_session,
            get_log_ids,
            get_log_views,
            get_log,
            get_session_view,
            replace_session_view,
            list_column_scripts,
            create_column_script,
            get_column_script,
            update_column_script,
            delete_column_script,
            list_filter_scripts,
            create_filter_script,
            get_filter_script,
            update_filter_script,
            delete_filter_script,
            debug_filter_script,
            list_routing_scripts,
            create_routing_script,
            get_routing_script,
            update_routing_script,
            delete_routing_script,
            get_routing_selection,
            replace_routing_selection,
            list_interceptors,
            create_interceptor,
            get_interceptor,
            update_interceptor,
            delete_interceptor,
            get_session_interceptors,
            replace_session_interceptors,
            get_certificate,
            regenerate_certificate,
            get_system_logs,
            clear_system_logs,
            get_bypass_entries,
            delete_bypass_entry,
            delete_bypass_entries,
            clear_bypass_entries,
            get_http_service_error,
            get_http_service_status,
            get_proxycrab_skill_install_info,
            install_proxycrab_skill,
        ]);

    let app = builder
        .build(tauri::generate_context!())
        .expect("error while building Tauri application");
    app.run(|app_handle, event| {
        if matches!(event, RunEvent::Exit) {
            let state = app_handle.state::<BackendState>();
            let manager = state.manager();
            let http = state
                .http
                .lock()
                .expect("HTTP service lock poisoned")
                .take();
            tauri::async_runtime::block_on(async move {
                let _ = manager.stop_proxy().await;
                if let Some(http) = http {
                    http.shutdown().await;
                }
            });
        }
    });
}
