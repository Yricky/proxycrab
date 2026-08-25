mod http_permissions;
mod share_ui;

use std::sync::{Arc, Mutex, RwLock};

use proxy_crab_mgr::{
    MitmManager, ProxyCrabManager,
    dto::{
        ActiveSession, AgentsPresetState, BreakpointDetailPayload, BreakpointQuery, BypassPage,
        BypassQuery, CertificateResponse, CreateAgentsPresetRequest, CreateSessionRequest,
        DebugFilterScriptRequest, DeleteCount, ExecuteTemporaryScriptRequest,
        ExtendBreakpointRequest, HttpApiChange, HttpApiResource, HttpServiceStatus,
        InterceptorCreateRequest, InterceptorDetail, InterceptorLibraryList,
        InterceptorUpdateRequest, LogDetail, LogIdsPayload, LogIdsRequest, LogViewsPayload,
        LogViewsRequest, ManagerError, ReplaceSessionInterceptorsRequest,
        ReplaceSessionViewRequest, RoutingSelection, ScriptRequest, SessionInterceptorsPayload,
        SessionViewPayload, SystemLogsQuery, UpdateAgentsPresetRequest, UpdateScriptRequest,
        UpdateSessionRequest,
    },
    http::{HttpServerHandle, start_http_server_with_routes},
    session_share::{CreateSessionShareRequest, CreatedSessionShare, SessionShareService},
    skill_install::{self, SkillInstallInfo},
};
use proxy_crab_mitm::{
    ProxyCrab,
    log_buffer::{BufferLayer, LogBuffer},
    model::{
        AppConfig, BreakpointSummary, InterceptorKind, ProxyStatus, Script, SessionMetadata,
        SystemLogEntry, TemporaryExecutionResult, WorkspacePaths,
    },
};
use tauri::{
    Emitter, Manager, RunEvent, State,
    menu::{Menu, MenuItem, Submenu},
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::http_permissions::{
    ApiActionView, CreatedApiKey, HttpPermissionService, IdentityPermissions, PendingApproval,
    PermissionEntry, PermissionIdentitySummary, ResolveApprovalRequest,
};

struct BackendState {
    manager: Arc<dyn ProxyCrabManager>,
    permissions: Option<Arc<HttpPermissionService>>,
    permission_error: Option<String>,
    http: Mutex<Option<HttpServerHandle>>,
    http_error: RwLock<Option<String>>,
    shares: Arc<SessionShareService>,
}

#[tauri::command]
async fn create_session_share(
    state: State<'_, BackendState>,
    request: CreateSessionShareRequest,
) -> Result<CreatedSessionShare, ManagerError> {
    state.shares.create(&state.manager(), request).await
}

impl BackendState {
    fn manager(&self) -> Arc<dyn ProxyCrabManager> {
        self.manager.clone()
    }

    fn permissions(&self) -> Result<Arc<HttpPermissionService>, ManagerError> {
        self.permissions.clone().ok_or_else(|| {
            ManagerError::internal(
                self.permission_error
                    .clone()
                    .unwrap_or_else(|| "management API permissions are unavailable".into()),
            )
        })
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
async fn get_agents_presets(
    state: State<'_, BackendState>,
) -> Result<AgentsPresetState, ManagerError> {
    state.manager().agents_presets().await
}

#[tauri::command]
async fn create_agents_preset(
    state: State<'_, BackendState>,
    request: CreateAgentsPresetRequest,
) -> Result<AgentsPresetState, ManagerError> {
    state.manager().create_agents_preset(request).await
}

#[tauri::command]
async fn update_agents_preset(
    state: State<'_, BackendState>,
    id: String,
    request: UpdateAgentsPresetRequest,
) -> Result<AgentsPresetState, ManagerError> {
    state.manager().update_agents_preset(id, request).await
}

#[tauri::command]
async fn activate_agents_preset(
    state: State<'_, BackendState>,
    id: String,
) -> Result<AgentsPresetState, ManagerError> {
    state.manager().activate_agents_preset(id).await
}

#[tauri::command]
async fn delete_agents_preset(
    state: State<'_, BackendState>,
    id: String,
) -> Result<AgentsPresetState, ManagerError> {
    state.manager().delete_agents_preset(id).await
}

#[tauri::command]
async fn reimport_default_agents_presets(
    state: State<'_, BackendState>,
) -> Result<AgentsPresetState, ManagerError> {
    state.manager().reimport_default_agents_presets().await
}

#[tauri::command]
async fn get_proxy_status(state: State<'_, BackendState>) -> Result<ProxyStatus, ManagerError> {
    state.manager().proxy_status().await
}

/// Enumerates the machine's IPv4 addresses (including loopback) for the
/// toolbar's display-only IP picker. The proxy itself always binds to the
/// configured `proxy_host`; this list is purely informational.
#[tauri::command]
fn list_local_ips() -> Vec<String> {
    let mut ips: Vec<String> = if_addrs::get_if_addrs()
        .map(|interfaces| {
            interfaces
                .into_iter()
                .map(|interface| interface.addr.ip())
                .filter(|ip| ip.is_ipv4())
                .map(|ip| ip.to_string())
                .collect()
        })
        .unwrap_or_default();
    ips.sort();
    ips.dedup();
    ips
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
async fn list_archived_sessions(
    state: State<'_, BackendState>,
) -> Result<Vec<SessionMetadata>, ManagerError> {
    state.manager().archived_sessions().await
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
async fn archive_session(
    state: State<'_, BackendState>,
    id: u64,
) -> Result<SessionMetadata, ManagerError> {
    state.manager().archive_session(id).await
}

#[tauri::command]
async fn restore_session(
    state: State<'_, BackendState>,
    id: u64,
) -> Result<SessionMetadata, ManagerError> {
    state.manager().restore_session(id).await
}

#[tauri::command]
async fn delete_archived_session(
    state: State<'_, BackendState>,
    id: u64,
) -> Result<(), ManagerError> {
    state.manager().delete_archived_session(id).await
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
fn validate_filter_regex(pattern: String) -> Option<String> {
    filter_regex_error(&pattern)
}

fn filter_regex_error(pattern: &str) -> Option<String> {
    regex::Regex::new(pattern)
        .err()
        .map(|error| error.to_string())
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
async fn list_breakpoints(
    state: State<'_, BackendState>,
    query: BreakpointQuery,
) -> Result<Vec<BreakpointSummary>, ManagerError> {
    state.manager().breakpoints(query).await
}

#[tauri::command]
async fn get_breakpoint(
    state: State<'_, BackendState>,
    id: u64,
) -> Result<BreakpointDetailPayload, ManagerError> {
    state.manager().breakpoint(id).await
}

#[tauri::command]
async fn extend_breakpoint(
    state: State<'_, BackendState>,
    id: u64,
    request: ExtendBreakpointRequest,
) -> Result<BreakpointSummary, ManagerError> {
    state.manager().extend_breakpoint(id, request).await
}

#[tauri::command]
async fn release_breakpoint(state: State<'_, BackendState>, id: u64) -> Result<(), ManagerError> {
    state.manager().release_breakpoint(id).await
}

#[tauri::command]
async fn execute_breakpoint_script(
    state: State<'_, BackendState>,
    id: u64,
    request: ExecuteTemporaryScriptRequest,
) -> Result<TemporaryExecutionResult, ManagerError> {
    state.manager().execute_breakpoint_script(id, request).await
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
        host: std::net::Ipv4Addr::UNSPECIFIED.to_string(),
        port: config.api_port,
        error,
    })
}

#[tauri::command]
fn get_http_permission_catalog(
    state: State<'_, BackendState>,
) -> Result<Vec<ApiActionView>, ManagerError> {
    Ok(state.permissions()?.catalog())
}

#[tauri::command]
fn list_http_permission_identities(
    state: State<'_, BackendState>,
) -> Result<Vec<PermissionIdentitySummary>, ManagerError> {
    state.permissions()?.identities()
}

#[tauri::command]
fn get_http_identity_permissions(
    state: State<'_, BackendState>,
    id: String,
) -> Result<IdentityPermissions, ManagerError> {
    state.permissions()?.identity_permissions(&id)
}

#[tauri::command]
fn replace_http_identity_permissions(
    state: State<'_, BackendState>,
    id: String,
    permissions: Vec<PermissionEntry>,
) -> Result<IdentityPermissions, ManagerError> {
    state.permissions()?.replace_permissions(&id, permissions)
}

#[tauri::command]
fn create_http_api_key(
    state: State<'_, BackendState>,
    name: String,
) -> Result<CreatedApiKey, ManagerError> {
    state.permissions()?.create_api_key(name)
}

#[tauri::command]
fn delete_http_api_key(state: State<'_, BackendState>, id: String) -> Result<(), ManagerError> {
    state.permissions()?.delete_api_key(&id)
}

#[tauri::command]
fn list_http_approvals(
    state: State<'_, BackendState>,
) -> Result<Vec<PendingApproval>, ManagerError> {
    state.permissions()?.approvals()
}

#[tauri::command]
fn resolve_http_approval(
    state: State<'_, BackendState>,
    id: u64,
    request: ResolveApprovalRequest,
) -> Result<(), ManagerError> {
    state.permissions()?.resolve_approval(id, request)
}

/// Wraps the generated IPC handler so every Tauri command invocation is echoed
/// directly to the terminal (stderr), bypassing tracing filters. The payload is
/// the JSON of the command's arguments, truncated for readability.
/// 仅在 dev 构建（`debug_assertions`）下输出日志；release 构建中为无操作包装。
fn logging_invoke_handler(
    handler: impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static,
) -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    move |invoke| {
        #[cfg(debug_assertions)]
        {
            let args = match invoke.message.payload() {
                tauri::ipc::InvokeBody::Json(value) => value.to_string(),
                tauri::ipc::InvokeBody::Raw(bytes) => format!("<raw:{} bytes>", bytes.len()),
            };
            let args = if args.chars().count() > 500 {
                let mut truncated: String = args.chars().take(500).collect();
                truncated.push_str("...<truncated>");
                truncated
            } else {
                args
            };
            eprintln!("[tauri-command] {} args={}", invoke.message.command(), args);
        }
        handler(invoke)
    }
}

const DEVTOOLS_MENU_ID: &str = "open-devtools";

/// 注册全局菜单中的「打开开发者工具」项。
/// 非 macOS 平台应用默认没有菜单栏，自建一个极简菜单；macOS 保留系统默认菜单
/// （应用名菜单 / 编辑 / 视图 等），仅追加一个「工具」子菜单，避免破坏系统惯例。
fn setup_global_menu(app: &tauri::App) -> tauri::Result<()> {
    let devtools = MenuItem::with_id(
        app,
        DEVTOOLS_MENU_ID,
        "打开开发者工具",
        true,
        Some("CmdOrCtrl+Alt+I"),
    )?;
    let tools = Submenu::with_items(app, "工具", true, &[&devtools])?;
    #[cfg(target_os = "macos")]
    {
        // macOS 默认菜单由 Tauri 在 build 阶段生成，此处直接追加子菜单
        if let Some(menu) = app.menu() {
            menu.append(&tools)?;
            return Ok(());
        }
        // 兜底：默认菜单不存在时回退为自建菜单
    }
    let menu = Menu::with_items(app, &[&tools])?;
    app.set_menu(menu)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            setup_global_menu(app)?;
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
            let workspace = runtime.workspace_paths().current_path;
            let manager: Arc<dyn ProxyCrabManager> = MitmManager::new(runtime);
            let shares = SessionShareService::new();
            let approval_handle = app.handle().clone();
            let permissions = HttpPermissionService::open(
                std::path::Path::new(&workspace),
                Arc::new(move |count| {
                    if let Err(error) = approval_handle.emit("proxycrab://approval-change", count) {
                        tracing::warn!("failed to emit approval change: {error}");
                    }
                }),
            );
            let permission_error = permissions.as_ref().err().map(ToString::to_string);
            let permissions = permissions.ok();
            let (http, http_error) = if let Some(permissions) = permissions.as_ref() {
                let share_manager = manager.clone();
                let share_service = shares.clone();
                match tauri::async_runtime::block_on(start_http_server_with_routes(
                    manager.clone(),
                    permissions.clone(),
                    move |_| {
                        share_ui::router().merge(proxy_crab_mgr::session_share::router(
                            share_manager,
                            share_service,
                        ))
                    },
                )) {
                    Ok(handle) => (Some(handle), None),
                    Err(error) => {
                        tracing::error!("management HTTP service did not start: {error}");
                        (None, Some(error.to_string()))
                    }
                }
            } else {
                (None, permission_error.clone())
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
                permissions,
                permission_error,
                http: Mutex::new(http),
                http_error: RwLock::new(http_error),
                shares,
            });
            Ok(())
        })
        .on_menu_event(|app, event| {
            if event.id() == DEVTOOLS_MENU_ID {
                if let Some(window) = app.get_webview_window("main") {
                    window.open_devtools();
                }
            }
        })
        .invoke_handler(logging_invoke_handler(tauri::generate_handler![
            get_workspace,
            set_workspace_for_next_start,
            get_config,
            replace_config,
            get_agents_presets,
            create_agents_preset,
            update_agents_preset,
            activate_agents_preset,
            delete_agents_preset,
            reimport_default_agents_presets,
            get_proxy_status,
            list_local_ips,
            start_proxy,
            stop_proxy,
            list_sessions,
            list_archived_sessions,
            get_active_session,
            replace_active_session,
            create_session,
            update_session,
            archive_session,
            restore_session,
            delete_archived_session,
            get_log_ids,
            get_log_views,
            validate_filter_regex,
            get_log,
            list_breakpoints,
            get_breakpoint,
            extend_breakpoint,
            release_breakpoint,
            execute_breakpoint_script,
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
            create_session_share,
            get_http_permission_catalog,
            list_http_permission_identities,
            get_http_identity_permissions,
            replace_http_identity_permissions,
            create_http_api_key,
            delete_http_api_key,
            list_http_approvals,
            resolve_http_approval,
            get_proxycrab_skill_install_info,
            install_proxycrab_skill,
        ]));

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

#[cfg(test)]
mod tests {
    #[test]
    fn filter_regex_validation_uses_rust_regex_syntax() {
        assert!(super::filter_regex_error(r"(?i)^get$").is_none());
        assert!(super::filter_regex_error("(").is_some());
    }

    #[test]
    fn list_local_ips_returns_sorted_unique_ipv4() {
        let ips = super::list_local_ips();
        for ip in &ips {
            assert!(
                ip.parse::<std::net::Ipv4Addr>().is_ok(),
                "entry is not an IPv4 address: {ip}"
            );
        }
        assert!(
            ips.windows(2).all(|pair| pair[0] < pair[1]),
            "IPs must be sorted and deduplicated: {ips:?}"
        );
    }
}
