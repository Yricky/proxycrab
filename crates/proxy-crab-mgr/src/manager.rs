use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

use async_trait::async_trait;
use proxy_crab_mitm::{
    ProxyCrab, ReplayBody, ReplayError, ReplayRequest,
    asset::{Asset, AssetError, AssetUpload},
    lua::{CaptureBodyAccess, ColumnEvaluator, FilterEvaluator},
    model::{
        AppConfig, BreakpointListFilter, BreakpointSummary, CaptureDetail, CaptureOutcome,
        CaptureSummary, Column, FilterColumn, FilterOption, HeaderValues, InterceptorKind,
        InterceptorScriptContent, InterceptorSnapshot, NewSession, ProxyStatus, Script, ScriptKind,
        SessionFilter, SessionInterceptor, SessionInterceptors, SessionMetadata, SystemLogEntry,
        TemporaryExecutionResult,
    },
    storage::{BodySide, BodySource},
};
use regex::Regex;

use crate::{
    agents::AgentsStore,
    clash_subscription::{
        ClashConfig, ClashSubscription, ClashSubscriptionState, EnableClashSubscriptionRequest,
        SaveClashConfigRequest,
    },
    dto::{
        ActiveSession, AgentsPresetState, BreakpointDetailPayload, BreakpointQuery, BypassPage,
        BypassQuery, CertificateResponse, CreateAgentsPresetRequest, CreateSessionRequest,
        DebugFilterScriptRequest, DeleteCount, ErrorCode, ExecuteTemporaryScriptRequest,
        ExtendBreakpointRequest, HeaderItem, InterceptorCreateRequest, InterceptorDetail,
        InterceptorLibraryList, InterceptorUpdateRequest, LogDetail, LogIdsPayload, LogIdsRequest,
        LogViewException, LogViewRow, LogViewsPayload, LogViewsRequest, ManagerError,
        ManagerResult, ReplaceSessionInterceptorsRequest, ReplaceSessionViewRequest,
        ReplayBodyPayload, ReplayRequestPayload, ReplayResult, RequestDetail, ResponseDetail,
        RoutingSelection, ScriptRequest, SessionInterceptorItem, SessionInterceptorsPayload,
        SessionViewPayload, SessionsPayload, SystemLogsQuery, UpdateAgentsPresetRequest,
        UpdateScriptRequest, UpdateSessionRequest,
    },
};
use tokio::sync::{Semaphore, watch};

pub(crate) mod redaction;
use redaction::{ReadPolicy, ReadRedactor};

const MAX_BLOCKING_MANAGEMENT_TASKS: usize = 8;

#[async_trait]
pub trait ProxyCrabManager: Send + Sync {
    async fn export_session_config(
        &self,
        id: u64,
        config: crate::bundle::ConfigShare,
    ) -> ManagerResult<Vec<u8>>;
    async fn import_bundle(
        &self,
        bytes: Vec<u8>,
        accepted: Vec<String>,
    ) -> ManagerResult<crate::bundle::ImportResult>;
    async fn resolve_bundle(
        &self,
        id: String,
        resolutions: Vec<crate::bundle::Resolution>,
    ) -> ManagerResult<crate::bundle::ImportResult>;

    fn reader(&self, script: Option<&str>, api: &str) -> ManagerResult<Arc<dyn ProxyCrabManager>>;
    async fn redaction_scripts(&self) -> ManagerResult<Vec<Script>>;
    async fn redaction_script(&self, name: String) -> ManagerResult<Script>;
    async fn create_redaction_script(&self, request: ScriptRequest) -> ManagerResult<()>;
    async fn update_redaction_script(
        &self,
        name: String,
        request: UpdateScriptRequest,
    ) -> ManagerResult<()>;
    async fn delete_redaction_script(&self, name: String) -> ManagerResult<()>;
    fn subscribe_proxy_status_changes(&self) -> watch::Receiver<u64>;
    async fn asset(&self, id: String) -> ManagerResult<Asset>;
    async fn begin_asset_upload(
        &self,
        id: String,
        content_type: String,
    ) -> ManagerResult<AssetUpload>;
    async fn config(&self) -> ManagerResult<AppConfig>;
    async fn replace_config(&self, config: AppConfig) -> ManagerResult<AppConfig>;
    async fn agents_presets(&self) -> ManagerResult<AgentsPresetState>;
    async fn agents_markdown(&self) -> ManagerResult<String>;
    async fn create_agents_preset(
        &self,
        request: CreateAgentsPresetRequest,
    ) -> ManagerResult<AgentsPresetState>;
    async fn update_agents_preset(
        &self,
        id: String,
        request: UpdateAgentsPresetRequest,
    ) -> ManagerResult<AgentsPresetState>;
    async fn activate_agents_preset(&self, id: String) -> ManagerResult<AgentsPresetState>;
    async fn delete_agents_preset(&self, id: String) -> ManagerResult<AgentsPresetState>;
    async fn reimport_default_agents_presets(&self) -> ManagerResult<AgentsPresetState>;
    async fn proxy_status(&self) -> ManagerResult<ProxyStatus>;
    async fn start_proxy(&self) -> ManagerResult<ProxyStatus>;
    async fn stop_proxy(&self) -> ManagerResult<ProxyStatus>;
    async fn sessions(&self) -> ManagerResult<SessionsPayload>;
    async fn archived_sessions(&self) -> ManagerResult<Vec<SessionMetadata>>;
    async fn replace_active_session(&self, active: ActiveSession) -> ManagerResult<ActiveSession>;
    async fn create_session(&self, request: CreateSessionRequest)
    -> ManagerResult<SessionMetadata>;
    async fn update_session(
        &self,
        id: u64,
        request: UpdateSessionRequest,
    ) -> ManagerResult<SessionMetadata>;
    async fn archive_session(&self, id: u64) -> ManagerResult<SessionMetadata>;
    async fn restore_session(&self, id: u64) -> ManagerResult<SessionMetadata>;
    async fn delete_archived_session(&self, id: u64) -> ManagerResult<()>;
    async fn log_ids(&self, request: LogIdsRequest) -> ManagerResult<LogIdsPayload>;
    async fn log_views(&self, request: LogViewsRequest) -> ManagerResult<LogViewsPayload>;
    async fn log(&self, session_id: Option<u64>, id: u64) -> ManagerResult<LogDetail>;
    async fn websocket_messages(
        &self,
        session_id: Option<u64>,
        id: u64,
        side: BodySide,
        after_id: u32,
        limit: usize,
    ) -> ManagerResult<proxy_crab_mitm::websocket::WebSocketPage>;
    async fn websocket_message_body(
        &self,
        session_id: Option<u64>,
        id: u64,
        side: BodySide,
        message_id: u32,
        max_size: u64,
    ) -> ManagerResult<BodySource>;
    async fn replay(
        &self,
        session_id: u64,
        request: ReplayRequestPayload,
    ) -> ManagerResult<ReplayResult>;
    async fn assets(&self) -> ManagerResult<Vec<proxy_crab_mitm::asset::AssetMetadata>>;
    async fn log_body_source(
        &self,
        session_id: Option<u64>,
        id: u64,
        side: BodySide,
    ) -> ManagerResult<BodySource>;
    async fn interceptor_snapshot_body_source(
        &self,
        session_id: Option<u64>,
        capture_id: u64,
        execution_id: u64,
        side: BodySide,
    ) -> ManagerResult<BodySource>;
    async fn interceptor_script_content(
        &self,
        session_id: Option<u64>,
        capture_id: u64,
        execution_id: u64,
    ) -> ManagerResult<InterceptorScriptContent>;
    async fn interceptor_snapshot(
        &self,
        session_id: Option<u64>,
        capture_id: u64,
        execution_id: u64,
    ) -> ManagerResult<InterceptorSnapshot>;
    async fn breakpoints(&self, query: BreakpointQuery) -> ManagerResult<Vec<BreakpointSummary>>;
    async fn breakpoint(&self, id: u64) -> ManagerResult<BreakpointDetailPayload>;
    async fn breakpoint_body_source(&self, id: u64, side: BodySide) -> ManagerResult<BodySource>;
    async fn extend_breakpoint(
        &self,
        id: u64,
        request: ExtendBreakpointRequest,
    ) -> ManagerResult<BreakpointSummary>;
    async fn release_breakpoint(&self, id: u64) -> ManagerResult<()>;
    async fn execute_breakpoint_script(
        &self,
        id: u64,
        request: ExecuteTemporaryScriptRequest,
    ) -> ManagerResult<TemporaryExecutionResult>;
    async fn session_view(&self, session_id: Option<u64>) -> ManagerResult<SessionViewPayload>;
    async fn replace_session_filter(
        &self,
        session_id: u64,
        filter: SessionFilter,
    ) -> ManagerResult<SessionViewPayload>;
    async fn replace_session_view(
        &self,
        session_id: Option<u64>,
        request: ReplaceSessionViewRequest,
    ) -> ManagerResult<SessionViewPayload>;
    async fn column_scripts(&self) -> ManagerResult<Vec<Script>>;
    async fn create_column_script(&self, request: ScriptRequest) -> ManagerResult<()>;
    async fn column_script(&self, name: String) -> ManagerResult<Script>;
    async fn update_column_script(
        &self,
        name: String,
        request: UpdateScriptRequest,
    ) -> ManagerResult<()>;
    async fn delete_column_script(&self, name: String) -> ManagerResult<()>;
    async fn filter_scripts(&self) -> ManagerResult<Vec<Script>>;
    async fn create_filter_script(&self, request: ScriptRequest) -> ManagerResult<()>;
    async fn filter_script(&self, name: String) -> ManagerResult<Script>;
    async fn update_filter_script(
        &self,
        name: String,
        request: UpdateScriptRequest,
    ) -> ManagerResult<()>;
    async fn delete_filter_script(&self, name: String) -> ManagerResult<()>;
    async fn debug_filter_script(
        &self,
        name: String,
        request: DebugFilterScriptRequest,
    ) -> ManagerResult<bool>;
    async fn routing_scripts(&self) -> ManagerResult<Vec<Script>>;
    async fn create_routing_script(&self, request: ScriptRequest) -> ManagerResult<()>;
    async fn routing_script(&self, name: String) -> ManagerResult<Script>;
    async fn update_routing_script(
        &self,
        name: String,
        request: UpdateScriptRequest,
    ) -> ManagerResult<()>;
    async fn delete_routing_script(&self, name: String) -> ManagerResult<()>;
    async fn routing_selection(&self) -> ManagerResult<RoutingSelection>;
    async fn replace_routing_selection(
        &self,
        selection: RoutingSelection,
    ) -> ManagerResult<RoutingSelection>;
    async fn interceptors(&self, kind: InterceptorKind) -> ManagerResult<InterceptorLibraryList>;
    async fn create_interceptor(&self, request: InterceptorCreateRequest) -> ManagerResult<()>;
    async fn interceptor(
        &self,
        kind: InterceptorKind,
        name: String,
    ) -> ManagerResult<InterceptorDetail>;
    async fn update_interceptor(
        &self,
        kind: InterceptorKind,
        name: String,
        request: InterceptorUpdateRequest,
    ) -> ManagerResult<()>;
    async fn delete_interceptor(&self, kind: InterceptorKind, name: String) -> ManagerResult<()>;
    async fn session_interceptors(
        &self,
        session_id: Option<u64>,
    ) -> ManagerResult<SessionInterceptorsPayload>;
    async fn replace_session_interceptors(
        &self,
        session_id: Option<u64>,
        request: ReplaceSessionInterceptorsRequest,
    ) -> ManagerResult<SessionInterceptorsPayload>;
    async fn certificate(&self) -> ManagerResult<CertificateResponse>;
    async fn clash_subscription(&self) -> ManagerResult<ClashSubscriptionState>;
    async fn enable_clash_subscription(
        &self,
        request: EnableClashSubscriptionRequest,
    ) -> ManagerResult<ClashSubscriptionState>;
    async fn disable_clash_subscription(&self) -> ManagerResult<ClashSubscriptionState>;
    async fn clash_subscription_yaml(&self) -> ManagerResult<String>;
    async fn clash_config(&self) -> ManagerResult<ClashConfig>;
    async fn save_clash_config(
        &self,
        request: SaveClashConfigRequest,
    ) -> ManagerResult<ClashConfig>;
    async fn reset_clash_config(&self) -> ManagerResult<ClashConfig>;
    async fn regenerate_certificate(&self) -> ManagerResult<CertificateResponse>;
    async fn system_logs(&self, query: SystemLogsQuery) -> ManagerResult<Vec<SystemLogEntry>>;
    async fn clear_system_logs(&self) -> ManagerResult<()>;
    async fn bypass_entries(&self, query: BypassQuery) -> ManagerResult<BypassPage>;
    async fn delete_bypass_entry(&self, id: u64) -> ManagerResult<()>;
    async fn delete_bypass_entries(&self, ids: Vec<u64>) -> ManagerResult<DeleteCount>;
    async fn clear_bypass_entries(&self) -> ManagerResult<DeleteCount>;
}

#[derive(Clone)]
pub struct MitmManager {
    imports: Arc<crate::bundle::BundleImports>,
    forced_redaction: Option<Script>,
    read_policy: Option<ReadPolicy>,
    runtime: Arc<ProxyCrab>,
    agents: Arc<AgentsStore>,
    clash_subscription: Arc<ClashSubscription>,
    blocking_tasks: Arc<Semaphore>,
}

enum PreparedFilter {
    All,
    Column {
        column: FilterColumn,
        input: String,
        regex: Option<Regex>,
        script: Option<PreparedColumnScript>,
    },
    Script {
        script: PreparedFilterScript,
        input: String,
    },
}

struct PreparedColumnScript {
    evaluator: Result<ColumnEvaluator, String>,
}

struct PreparedFilterScript {
    evaluator: Result<FilterEvaluator, String>,
}

impl MitmManager {
    pub fn new(runtime: Arc<ProxyCrab>) -> Arc<Self> {
        let agents = Arc::new(AgentsStore::new(runtime.workspace().root()));
        if let Err(error) = agents.initialize() {
            tracing::error!("failed to initialize AGENTS.md presets: {error}");
        }
        let clash_subscription = Arc::new(ClashSubscription::new(runtime.workspace().root()));
        Arc::new(Self {
            imports: Arc::new(crate::bundle::BundleImports::default()),
            forced_redaction: None,
            read_policy: None,
            runtime,
            agents,
            clash_subscription,
            blocking_tasks: Arc::new(Semaphore::new(MAX_BLOCKING_MANAGEMENT_TASKS)),
        })
    }

    /// Pin a validated, process-local policy that overrides identity and share bindings.
    pub fn with_forced_redaction(&self, script: Script) -> ManagerResult<Arc<Self>> {
        proxy_crab_mitm::lua::redaction::RedactionEvaluator::new(&script.content, &script.name)
            .map_err(|error| ManagerError::bad_request(error.to_string()))?;
        let mut manager = self.clone();
        manager.forced_redaction = Some(script);
        Ok(Arc::new(manager))
    }

    fn ensure_redaction_management_enabled(&self) -> ManagerResult<()> {
        if self.forced_redaction.is_some() {
            return Err(ManagerError::new(
                ErrorCode::RedactionManagementDisabled,
                "脱敏接口管理功能已禁用",
            ));
        }
        Ok(())
    }

    pub fn runtime(&self) -> &Arc<ProxyCrab> {
        &self.runtime
    }

    async fn run_blocking<T, F>(&self, name: &'static str, task: F) -> ManagerResult<T>
    where
        T: Send + 'static,
        F: FnOnce() -> ManagerResult<T> + Send + 'static,
    {
        let permit = self
            .blocking_tasks
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| ManagerError::internal("management task limiter closed"))?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            task()
        })
        .await
        .map_err(|error| ManagerError::internal(format!("{name} task failed: {error}")))?
    }

    fn session_id(&self, requested: Option<u64>) -> ManagerResult<u64> {
        requested
            .or_else(|| self.runtime.active_session_id())
            .ok_or_else(|| ManagerError::conflict("no active Session"))
    }

    fn render_builtin_cell(column: &Column, item: &CaptureSummary) -> Option<String> {
        match column {
            Column::Method { .. } => Some(item.request.method.clone()),
            Column::Uri { .. } => Some(item.request.uri.clone()),
            Column::Code { .. } => item
                .response
                .as_ref()
                .map(|response| response.status.to_string())
                .or_else(|| Some("...".into())),
            Column::Source { .. } => Some(item.source.clone()),
            Column::Stage { .. } => Some(item.stage.clone()),
            Column::CreatedAt { .. } => Some(item.created_at.to_string()),
            Column::UpdatedAt { .. } => Some(item.updated_at.to_string()),
            Column::Script { .. } => None,
        }
    }

    fn capture_body_access(runtime: &Arc<ProxyCrab>, item: &CaptureSummary) -> CaptureBodyAccess {
        let runtime = runtime.clone();
        let session_id = item.session_id;
        let capture_id = item.id;
        CaptureBodyAccess::new(move |side| {
            runtime.capture_body_source(session_id, capture_id, side)
        })
    }

    fn prepare_filter(
        runtime: &ProxyCrab,
        filter: &SessionFilter,
    ) -> ManagerResult<PreparedFilter> {
        if filter.input.is_empty() {
            return Ok(PreparedFilter::All);
        }
        match &filter.option {
            None => Ok(PreparedFilter::All),
            Some(FilterOption::Column { column, regex }) => {
                let script = match column {
                    FilterColumn::Script { script_name } => {
                        let script = runtime
                            .script(ScriptKind::Column, script_name)
                            .map_err(map_error)?;
                        Some(PreparedColumnScript {
                            evaluator: ColumnEvaluator::new(&script.content, &script.name)
                                .map_err(|error| error.to_string()),
                        })
                    }
                    _ => None,
                };
                Ok(PreparedFilter::Column {
                    column: column.clone(),
                    input: filter.input.clone(),
                    regex: regex
                        .then(|| Regex::new(&filter.input))
                        .transpose()
                        .map_err(|error| ManagerError::bad_request(error.to_string()))?,
                    script,
                })
            }
            Some(FilterOption::Script { script_name }) => {
                let script = runtime
                    .script(ScriptKind::Filter, script_name)
                    .map_err(map_error)?;
                Ok(PreparedFilter::Script {
                    script: PreparedFilterScript {
                        evaluator: FilterEvaluator::new(&script.content, &script.name)
                            .map_err(|error| error.to_string()),
                    },
                    input: filter.input.clone(),
                })
            }
        }
    }

    fn matches_filter<F>(filter: &PreparedFilter, item: &CaptureSummary, body_access: F) -> bool
    where
        F: FnOnce() -> CaptureBodyAccess,
    {
        match filter {
            PreparedFilter::All => true,
            PreparedFilter::Script { script, input } => {
                let Ok(evaluator) = &script.evaluator else {
                    return false;
                };
                evaluator
                    .evaluate_with_bodies(input, item, body_access())
                    .unwrap_or(false)
            }
            PreparedFilter::Column {
                column,
                input,
                regex,
                script,
            } => {
                let value = match column {
                    FilterColumn::Method => item.request.method.clone(),
                    FilterColumn::Uri => item.request.uri.clone(),
                    FilterColumn::Code => item
                        .response
                        .as_ref()
                        .map(|response| response.status.to_string())
                        .unwrap_or_else(|| "...".into()),
                    FilterColumn::Source => item.source.clone(),
                    FilterColumn::Stage => item.stage.clone(),
                    FilterColumn::Script { .. } => {
                        let Some(script) = script else {
                            return false;
                        };
                        let Ok(evaluator) = &script.evaluator else {
                            return false;
                        };
                        let Ok(value) = evaluator.evaluate_with_bodies(item, body_access()) else {
                            return false;
                        };
                        value
                    }
                };
                regex
                    .as_ref()
                    .map_or_else(|| value.contains(input), |regex| regex.is_match(&value))
            }
        }
    }

    fn classify_log_id<F>(
        filter: &PreparedFilter,
        item: &CaptureSummary,
        active_ids: &BTreeSet<u64>,
        body_access: F,
        matched_ids: &mut Vec<u64>,
        in_progress_ids: &mut Vec<u64>,
        redactor: Option<&ReadRedactor>,
        runtime: &Arc<ProxyCrab>,
    ) -> ManagerResult<()>
    where
        F: FnOnce() -> CaptureBodyAccess,
    {
        let mut sanitized = None;
        let mut sanitized_bodies = None;
        let mut sanitized_view = None;
        if let Some(redactor) = redactor {
            match filter {
                PreparedFilter::Column {
                    column: FilterColumn::Uri,
                    ..
                } => {
                    let mut value = item.clone();
                    value.request.uri = redactor.uri(&item.request.uri)?;
                    sanitized = Some(value);
                }
                PreparedFilter::Script { .. }
                | PreparedFilter::Column {
                    column: FilterColumn::Script { .. },
                    ..
                } => {
                    let view = redactor.view(runtime, item)?;
                    sanitized_bodies = Some(view.body_access());
                    sanitized = Some(view.summary());
                    sanitized_view = Some(view);
                }
                _ => {}
            }
        }
        let matched = Self::matches_filter(filter, sanitized.as_ref().unwrap_or(item), || {
            sanitized_bodies.unwrap_or_else(body_access)
        });
        if let Some(view) = sanitized_view {
            view.ensure_success().map_err(|_| redaction::failed())?;
        }
        if matched {
            matched_ids.push(item.id);
        }
        if item.outcome == CaptureOutcome::InProgress && active_ids.contains(&item.id) {
            in_progress_ids.push(item.id);
        }
        Ok(())
    }
}

#[async_trait]
impl ProxyCrabManager for MitmManager {
    async fn export_session_config(
        &self,
        id: u64,
        config: crate::bundle::ConfigShare,
    ) -> ManagerResult<Vec<u8>> {
        let runtime = self.runtime.clone();
        self.run_blocking("export configuration", move || {
            crate::bundle::export(&runtime, id, &config)
                .map_err(|e| ManagerError::bad_request(format!("{e:#}")))
        })
        .await
    }
    async fn import_bundle(
        &self,
        bytes: Vec<u8>,
        accepted: Vec<String>,
    ) -> ManagerResult<crate::bundle::ImportResult> {
        let runtime = self.runtime.clone();
        let imports = self.imports.clone();
        self.run_blocking("import bundle", move || {
            imports
                .import(&runtime, bytes, accepted)
                .map_err(|e| ManagerError::bad_request(format!("{e:#}")))
        })
        .await
    }
    async fn resolve_bundle(
        &self,
        id: String,
        resolutions: Vec<crate::bundle::Resolution>,
    ) -> ManagerResult<crate::bundle::ImportResult> {
        let runtime = self.runtime.clone();
        let imports = self.imports.clone();
        self.run_blocking("resolve bundle", move || {
            imports
                .resolve(&runtime, id, resolutions)
                .map_err(|e| ManagerError::bad_request(format!("{e:#}")))
        })
        .await
    }

    fn reader(&self, script: Option<&str>, api: &str) -> ManagerResult<Arc<dyn ProxyCrabManager>> {
        let mut reader = self.clone();
        let script = match &self.forced_redaction {
            Some(forced) => Some(forced.clone()),
            None => script
                .map(|name| {
                    self.runtime
                        .script(ScriptKind::Redaction, name)
                        .map_err(|_| redaction::failed())
                })
                .transpose()?,
        };
        reader.read_policy = script.map(|script| ReadPolicy {
            script,
            api: api.into(),
        });
        Ok(Arc::new(reader))
    }
    async fn redaction_scripts(&self) -> ManagerResult<Vec<Script>> {
        self.ensure_redaction_management_enabled()?;
        self.runtime
            .scripts(ScriptKind::Redaction)
            .map_err(map_error)
    }
    async fn redaction_script(&self, name: String) -> ManagerResult<Script> {
        self.ensure_redaction_management_enabled()?;
        self.runtime
            .script(ScriptKind::Redaction, &name)
            .map_err(map_error)
    }
    async fn create_redaction_script(&self, request: ScriptRequest) -> ManagerResult<()> {
        self.ensure_redaction_management_enabled()?;
        self.runtime
            .create_script(
                ScriptKind::Redaction,
                Script {
                    name: request.name,
                    content: request.content,
                },
            )
            .map_err(map_error)
    }
    async fn update_redaction_script(
        &self,
        name: String,
        request: UpdateScriptRequest,
    ) -> ManagerResult<()> {
        self.ensure_redaction_management_enabled()?;
        self.runtime
            .update_script(ScriptKind::Redaction, &name, request.content)
            .map_err(map_error)
    }
    async fn delete_redaction_script(&self, name: String) -> ManagerResult<()> {
        self.ensure_redaction_management_enabled()?;
        self.runtime
            .delete_script(ScriptKind::Redaction, &name)
            .map_err(map_error)
    }

    fn subscribe_proxy_status_changes(&self) -> watch::Receiver<u64> {
        self.runtime.subscribe_proxy_status_changes()
    }

    async fn asset(&self, id: String) -> ManagerResult<Asset> {
        self.runtime
            .asset(&id)
            .map_err(map_asset_error)?
            .ok_or_else(|| {
                ManagerError::new(ErrorCode::AssetNotFound, format!("asset {id} not found"))
            })
    }

    async fn begin_asset_upload(
        &self,
        id: String,
        content_type: String,
    ) -> ManagerResult<AssetUpload> {
        self.runtime
            .begin_asset_upload(&id, content_type)
            .await
            .map_err(map_asset_error)
    }

    async fn config(&self) -> ManagerResult<AppConfig> {
        Ok(self.runtime.config())
    }

    async fn replace_config(&self, config: AppConfig) -> ManagerResult<AppConfig> {
        self.runtime.replace_config(config).await.map_err(map_error)
    }

    async fn agents_presets(&self) -> ManagerResult<AgentsPresetState> {
        let agents = self.agents.clone();
        self.run_blocking("AGENTS.md preset", move || {
            agents.state().map_err(map_error)
        })
        .await
    }

    async fn agents_markdown(&self) -> ManagerResult<String> {
        let agents = self.agents.clone();
        self.run_blocking("AGENTS.md", move || {
            agents.active_markdown().map_err(map_error)
        })
        .await
    }

    async fn create_agents_preset(
        &self,
        request: CreateAgentsPresetRequest,
    ) -> ManagerResult<AgentsPresetState> {
        let agents = self.agents.clone();
        self.run_blocking("AGENTS.md preset", move || {
            agents.create(request.name).map_err(map_error)
        })
        .await
    }

    async fn update_agents_preset(
        &self,
        id: String,
        request: UpdateAgentsPresetRequest,
    ) -> ManagerResult<AgentsPresetState> {
        let agents = self.agents.clone();
        self.run_blocking("AGENTS.md preset", move || {
            agents
                .update(&id, request.name, request.content)
                .map_err(map_error)
        })
        .await
    }

    async fn activate_agents_preset(&self, id: String) -> ManagerResult<AgentsPresetState> {
        let agents = self.agents.clone();
        self.run_blocking("AGENTS.md preset", move || {
            agents.activate(&id).map_err(map_error)
        })
        .await
    }

    async fn delete_agents_preset(&self, id: String) -> ManagerResult<AgentsPresetState> {
        let agents = self.agents.clone();
        self.run_blocking("AGENTS.md preset", move || {
            agents.delete(&id).map_err(map_error)
        })
        .await
    }

    async fn reimport_default_agents_presets(&self) -> ManagerResult<AgentsPresetState> {
        let agents = self.agents.clone();
        self.run_blocking("AGENTS.md preset", move || {
            agents.reimport_defaults().map_err(map_error)
        })
        .await
    }

    async fn proxy_status(&self) -> ManagerResult<ProxyStatus> {
        Ok(self.runtime.proxy_status())
    }

    async fn start_proxy(&self) -> ManagerResult<ProxyStatus> {
        self.runtime.start_proxy().await.map_err(map_error)
    }

    async fn stop_proxy(&self) -> ManagerResult<ProxyStatus> {
        self.runtime.stop_proxy().await.map_err(map_error)
    }

    async fn sessions(&self) -> ManagerResult<SessionsPayload> {
        Ok(SessionsPayload {
            sessions: self.runtime.sessions(),
            active_session_id: self.runtime.active_session_id(),
        })
    }

    async fn archived_sessions(&self) -> ManagerResult<Vec<SessionMetadata>> {
        Ok(self.runtime.archived_sessions())
    }

    async fn replace_active_session(&self, active: ActiveSession) -> ManagerResult<ActiveSession> {
        self.runtime
            .replace_active_session(active.session_id)
            .await
            .map_err(map_error)?;
        Ok(active)
    }

    async fn create_session(
        &self,
        request: CreateSessionRequest,
    ) -> ManagerResult<SessionMetadata> {
        self.runtime
            .create_session(NewSession {
                name: request.name,
                description: request.description,
                view: request.view,
                interceptors: request.interceptors,
            })
            .map_err(map_error)
    }

    async fn update_session(
        &self,
        id: u64,
        request: UpdateSessionRequest,
    ) -> ManagerResult<SessionMetadata> {
        self.runtime
            .update_session(id, request.name, request.description)
            .map_err(map_error)
    }

    async fn archive_session(&self, id: u64) -> ManagerResult<SessionMetadata> {
        self.runtime.archive_session(id).await.map_err(map_error)
    }

    async fn restore_session(&self, id: u64) -> ManagerResult<SessionMetadata> {
        self.runtime.restore_session(id).await.map_err(map_error)
    }

    async fn delete_archived_session(&self, id: u64) -> ManagerResult<()> {
        self.runtime
            .delete_archived_session(id)
            .await
            .map_err(map_error)
    }

    async fn log_ids(&self, request: LogIdsRequest) -> ManagerResult<LogIdsPayload> {
        const DEFAULT_LIMIT: usize = 10_000;
        const MAX_IDS: usize = 10_000;
        const SCAN_PAGE_SIZE: usize = 512;

        let session_id = self.session_id(request.session_id)?;
        let limit = request.limit.unwrap_or(DEFAULT_LIMIT).min(DEFAULT_LIMIT);
        if request.ids.is_some() && (request.min_id.is_some() || request.max_id.is_some()) {
            return Err(ManagerError::bad_request(
                "ids cannot be combined with min_id or max_id",
            ));
        }
        if request.ids.as_ref().is_some_and(|ids| ids.len() > MAX_IDS) {
            return Err(ManagerError::bad_request(format!(
                "at most {MAX_IDS} log IDs may be requested"
            )));
        }
        let ascending = request.min_id.is_some() && request.max_id.is_none();
        let filter = request.filter.unwrap_or_default();
        let active_ids = match self.runtime.proxy_status() {
            ProxyStatus::Running { active_netlog, .. } => active_netlog
                .get(&session_id)
                .into_iter()
                .flatten()
                .copied()
                .collect(),
            _ => BTreeSet::new(),
        };
        let runtime = self.runtime.clone();
        let requested_ids = request.ids;
        let min_id = request.min_id;
        let max_id = request.max_id;
        let policy = self.read_policy.clone();
        self.run_blocking("log ID", move || {
            let prepared = Self::prepare_filter(&runtime, &filter)?;
            let needs_redaction = matches!(
                &prepared,
                PreparedFilter::Script { .. }
                    | PreparedFilter::Column {
                        column: FilterColumn::Uri | FilterColumn::Script { .. },
                        ..
                    }
            );
            let redactor = if needs_redaction {
                ReadRedactor::prepare(&policy)?
            } else {
                None
            };
            let mut matched_ids = Vec::with_capacity(limit);
            let mut in_progress_ids = Vec::new();
            if let Some(ids) = requested_ids {
                let unique_ids = ids.into_iter().collect::<BTreeSet<_>>();
                let mut summaries = HashMap::new();
                for ids in unique_ids
                    .iter()
                    .copied()
                    .collect::<Vec<_>>()
                    .chunks(SCAN_PAGE_SIZE)
                {
                    for item in runtime.captures(session_id, ids).map_err(map_error)? {
                        summaries.insert(item.id, item);
                    }
                }
                for id in unique_ids.into_iter().rev() {
                    if let Some(item) = summaries.get(&id) {
                        Self::classify_log_id(
                            &prepared,
                            item,
                            &active_ids,
                            || Self::capture_body_access(&runtime, item),
                            &mut matched_ids,
                            &mut in_progress_ids,
                            redactor.as_ref(),
                            &runtime,
                        )?;
                    }
                }
            } else if limit == 0 {
                // A zero limit is useful for validating a filter without scanning logs.
            } else if matches!(prepared, PreparedFilter::All) {
                for item in runtime
                    .list_captures_range(session_id, limit, min_id, max_id, ascending)
                    .map_err(map_error)?
                {
                    Self::classify_log_id(
                        &prepared,
                        &item,
                        &active_ids,
                        || Self::capture_body_access(&runtime, &item),
                        &mut matched_ids,
                        &mut in_progress_ids,
                        redactor.as_ref(),
                        &runtime,
                    )?;
                }
            } else {
                let mut min_id = min_id;
                let mut max_id = max_id;
                'scan: while matched_ids.len() < limit {
                    let page = runtime
                        .list_captures_range(session_id, SCAN_PAGE_SIZE, min_id, max_id, ascending)
                        .map_err(map_error)?;
                    if page.is_empty() {
                        break;
                    }
                    let page_len = page.len();
                    let cursor = page.last().map(|item| item.id);
                    for item in page {
                        Self::classify_log_id(
                            &prepared,
                            &item,
                            &active_ids,
                            || Self::capture_body_access(&runtime, &item),
                            &mut matched_ids,
                            &mut in_progress_ids,
                            redactor.as_ref(),
                            &runtime,
                        )?;
                        if matched_ids.len() == limit {
                            break 'scan;
                        }
                    }
                    if page_len < SCAN_PAGE_SIZE {
                        break;
                    }
                    if ascending {
                        min_id = cursor;
                    } else {
                        max_id = cursor;
                    }
                }
            }
            Ok(LogIdsPayload {
                matched_ids,
                in_progress_ids,
            })
        })
        .await
    }

    async fn log_views(&self, request: LogViewsRequest) -> ManagerResult<LogViewsPayload> {
        const MAX_LOGS: usize = 200;

        if request.logs.len() > MAX_LOGS {
            return Err(ManagerError::bad_request(format!(
                "at most {MAX_LOGS} logs may be requested"
            )));
        }
        let session_id = self.session_id(request.session_id)?;
        let runtime = self.runtime.clone();
        let policy = self.read_policy.clone();
        self.run_blocking("log view", move || {
            let redactor = ReadRedactor::prepare(&policy)?;
            let columns = match request.view {
                Some(view) => view.columns,
                None => runtime.session_view(session_id).map_err(map_error)?.columns,
            };
            if columns
                .iter()
                .any(|column| !column.width().is_finite() || column.width() <= 0.0)
            {
                return Err(ManagerError::bad_request("column width must be positive"));
            }
            let mut requested = HashMap::new();
            for item in request.logs {
                requested.insert(item.id, item.updated_at);
            }
            let ids = requested.keys().copied().collect::<Vec<_>>();
            let summaries = runtime.captures(session_id, &ids).map_err(map_error)?;
            let summaries = summaries
                .into_iter()
                .map(|item| (item.id, item))
                .collect::<HashMap<_, _>>();
            let mut scripts = HashMap::<String, Result<ColumnEvaluator, String>>::new();
            for column in &columns {
                if let Column::Script { script_name, .. } = column {
                    scripts.entry(script_name.clone()).or_insert_with(|| {
                        runtime
                            .script(ScriptKind::Column, script_name)
                            .and_then(|script| ColumnEvaluator::new(&script.content, &script.name))
                            .map_err(|error| error.to_string())
                    });
                }
            }

            let mut rows = Vec::new();
            let mut exceptions = Vec::new();
            for (id, client_updated_at) in requested {
                let Some(item) = summaries.get(&id) else {
                    exceptions.push(LogViewException {
                        id,
                        column_index: None,
                        code: "log_not_found".into(),
                        message: format!("log {id} not found"),
                    });
                    continue;
                };
                if redactor.is_none()
                    && client_updated_at.is_some_and(|updated_at| item.updated_at <= updated_at)
                {
                    continue;
                }

                let view = redactor
                    .as_ref()
                    .map(|r| r.view(&runtime, item))
                    .transpose()?;
                let output = view.as_ref().map(|v| v.summary());
                let item = output.as_ref().unwrap_or(item);
                let bodies = (!scripts.is_empty()).then(|| {
                    view.as_ref()
                        .map(|v| v.body_access())
                        .unwrap_or_else(|| Self::capture_body_access(&runtime, item))
                });
                let mut cells = Vec::with_capacity(columns.len());
                for (column_index, column) in columns.iter().enumerate() {
                    if let Some(value) = Self::render_builtin_cell(column, item) {
                        cells.push(value);
                        continue;
                    }
                    let Column::Script { script_name, .. } = column else {
                        unreachable!("all built-in columns were rendered above");
                    };
                    let result = match scripts
                        .get(script_name)
                        .expect("every script column is preloaded")
                    {
                        Ok(evaluator) => evaluator
                            .evaluate_with_bodies(
                                item,
                                bodies
                                    .as_ref()
                                    .expect("script columns require body access")
                                    .clone(),
                            )
                            .map_err(|error| error.to_string()),
                        Err(message) => Err(message.clone()),
                    };
                    match result {
                        Ok(value) => cells.push(value),
                        Err(message) => {
                            cells.push(String::new());
                            exceptions.push(LogViewException {
                                id,
                                column_index: Some(column_index),
                                code: "column_script_error".into(),
                                message,
                            });
                        }
                    }
                }
                if let Some(view) = &view {
                    view.ensure_success().map_err(|_| redaction::failed())?;
                }
                rows.push(LogViewRow {
                    id,
                    created_at: item.created_at,
                    updated_at: item.updated_at,
                    outcome: outcome_name(item.outcome).into(),
                    cells,
                });
            }
            Ok(LogViewsPayload {
                columns,
                rows,
                exceptions,
            })
        })
        .await
    }

    async fn replay(
        &self,
        session_id: u64,
        request: ReplayRequestPayload,
    ) -> ManagerResult<ReplayResult> {
        let body = match request.body {
            None => None,
            Some(ReplayBodyPayload::Text { text, charset }) => match charset.as_deref() {
                None | Some("utf8") => Some(ReplayBody::Text { text }),
                Some(other) => {
                    return Err(ManagerError::bad_request(format!(
                        "unsupported charset: {other}"
                    )));
                }
            },
            Some(ReplayBodyPayload::BodyRef {
                session_id,
                log_id,
                side,
            }) => {
                let side = match side.as_str() {
                    "request" => BodySide::Request,
                    "response" => BodySide::Response,
                    other => {
                        return Err(ManagerError::bad_request(format!(
                            "invalid body side: {other}"
                        )));
                    }
                };
                Some(ReplayBody::BodyRef {
                    session_id,
                    log_id,
                    side,
                })
            }
            Some(ReplayBodyPayload::Asset { asset_id }) => Some(ReplayBody::Asset { asset_id }),
        };
        let log_id = self
            .runtime
            .replay(
                session_id,
                ReplayRequest {
                    method: request.method,
                    url: request.url,
                    headers: request.headers,
                    body,
                },
            )
            .await
            .map_err(|error| match error {
                ReplayError::SessionNotFound => {
                    ManagerError::not_found(format!("session {session_id} not found or archived"))
                }
                ReplayError::ProxyNotRunning => {
                    ManagerError::new(ErrorCode::ProxyNotRunning, "proxy is not running")
                }
                ReplayError::BodyNotFound => {
                    ManagerError::not_found("referenced capture body not found")
                }
                ReplayError::AssetNotFound => ManagerError::not_found("referenced asset not found"),
                ReplayError::Invalid(message) => ManagerError::bad_request(message),
                ReplayError::Internal(error) => {
                    ManagerError::new(ErrorCode::ReplayFailed, error.to_string())
                }
            })?;
        Ok(ReplayResult { log_id })
    }

    async fn assets(&self) -> ManagerResult<Vec<proxy_crab_mitm::asset::AssetMetadata>> {
        self.runtime
            .assets()
            .map_err(|error| ManagerError::new(ErrorCode::AssetStoreFailed, error.to_string()))
    }

    async fn log(&self, session_id: Option<u64>, id: u64) -> ManagerResult<LogDetail> {
        let session_id = self.session_id(session_id)?;
        let runtime = self.runtime.clone();
        let policy = self.read_policy.clone();
        self.run_blocking("log read", move || {
            let redactor = ReadRedactor::prepare(&policy)?;
            let detail = runtime
                .capture(session_id, id)
                .map_err(map_error)?
                .ok_or_else(|| ManagerError::not_found(format!("log {id} not found")))?;
            let detail = match redactor {
                Some(ref r) => r.detail(&runtime, detail)?,
                None => detail,
            };
            let mut result = log_detail_from_capture(detail);
            result.websocket = runtime.websocket_info(session_id, id).map_err(map_error)?;
            if policy.is_some() {
                if let Some(info) = &mut result.websocket {
                    info.close_reason = None;
                    // Negotiated headers must follow the same policy as the handshake.
                    let handshake = runtime
                        .capture(session_id, info.handshake_log_id)
                        .map_err(map_error)?
                        .ok_or_else(|| ManagerError::not_found("handshake not found"))?;
                    let safe = redactor
                        .as_ref()
                        .expect("policy redactor")
                        .detail(&runtime, handshake)?
                        .summary;
                    info.subprotocol = safe
                        .response
                        .as_ref()
                        .and_then(|r| r.headers.get("sec-websocket-protocol"))
                        .and_then(|v| v.first())
                        .cloned();
                    info.extensions = safe
                        .response
                        .as_ref()
                        .and_then(|r| r.headers.get("sec-websocket-extensions"))
                        .map(|v| v.join(","))
                        .unwrap_or_default();
                }
            }
            Ok(result)
        })
        .await
    }

    async fn websocket_messages(
        &self,
        session_id: Option<u64>,
        id: u64,
        side: BodySide,
        after_id: u32,
        limit: usize,
    ) -> ManagerResult<proxy_crab_mitm::websocket::WebSocketPage> {
        if !(1..=2000).contains(&limit) {
            return Err(ManagerError::bad_request(
                "limit must be between 1 and 2000",
            ));
        }
        let sid = self.session_id(session_id)?;
        let runtime = self.runtime.clone();
        let redact = self.read_policy.is_some();
        self.run_blocking("websocket message index", move || {
            let mut page = runtime
                .websocket_messages(sid, id, side, after_id, limit)
                .map_err(map_error)?
                .ok_or_else(|| ManagerError::not_found("WebSocket connection not found"))?;
            if redact {
                for message in &mut page.messages {
                    if message.error.is_some() {
                        message.error = Some("WebSocket delivery failed".into());
                    }
                    for error in &mut message.errors {
                        error.error = "WebSocket callback failed".into();
                    }
                }
            }
            Ok(page)
        })
        .await
    }

    async fn websocket_message_body(
        &self,
        session_id: Option<u64>,
        id: u64,
        side: BodySide,
        message_id: u32,
        max_size: u64,
    ) -> ManagerResult<BodySource> {
        if message_id == 0 || max_size == 0 {
            return Err(ManagerError::bad_request(
                "message_id and max_size must be positive",
            ));
        }
        let sid = self.session_id(session_id)?;
        let runtime = self.runtime.clone();
        let policy = self.read_policy.clone();
        self.run_blocking("websocket message body", move || {
            let page = runtime
                .websocket_messages(sid, id, side, message_id - 1, 1)
                .map_err(map_error)?
                .ok_or_else(|| ManagerError::not_found("WebSocket connection not found"))?;
            let meta = page
                .messages
                .into_iter()
                .find(|m| m.id == message_id)
                .ok_or_else(|| ManagerError::not_found("message not found"))?;
            if !meta.content_available {
                return Err(ManagerError::new(
                    ErrorCode::MessageContentUnavailable,
                    "message content was omitted during capture",
                ));
            }
            if meta.size > max_size {
                return Err(ManagerError::body_too_large(meta.size, max_size));
            }
            let mut bytes = runtime
                .websocket_message_body(sid, id, side, message_id)
                .map_err(map_error)?
                .ok_or_else(|| ManagerError::not_found("message not found"))?;
            if let Some(policy) = policy {
                let summary = runtime
                    .captures(sid, &[id])
                    .map_err(map_error)?
                    .pop()
                    .ok_or_else(|| ManagerError::not_found("log not found"))?;
                let evaluator = proxy_crab_mitm::lua::redaction::RedactionEvaluator::new(
                    &policy.script.content,
                    &policy.script.name,
                )
                .map_err(|_| redaction::failed())?;
                let mut context = serde_json::to_value(&meta).map_err(|_| redaction::failed())?;
                context["session_id"] = sid.into();
                context["log_id"] = id.into();
                bytes = evaluator
                    .websocket_message(&policy.api, &summary.request.uri, side, &context, bytes)
                    .map_err(|_| redaction::failed())?
                    .ok_or_else(|| {
                        ManagerError::new(
                            ErrorCode::MessageContentHidden,
                            "message content is hidden by policy",
                        )
                    })?;
            }
            if bytes.len() as u64 > max_size {
                return Err(ManagerError::body_too_large(bytes.len() as u64, max_size));
            }
            Ok(BodySource {
                stored_size: bytes.len() as u64,
                data: proxy_crab_mitm::storage::BodySourceData::Bytes(bytes),
                path: None,
                content_type: Some(
                    if serde_json::to_value(&meta).map_err(|_| redaction::failed())?["type"]
                        == "text"
                    {
                        "text/plain; charset=utf-8"
                    } else {
                        "application/octet-stream"
                    }
                    .into(),
                ),
                content_encodings: Vec::new(),
            })
        })
        .await
    }

    async fn log_body_source(
        &self,
        session_id: Option<u64>,
        id: u64,
        side: BodySide,
    ) -> ManagerResult<BodySource> {
        let session_id = self.session_id(session_id)?;
        let runtime = self.runtime.clone();
        let policy = self.read_policy.clone();
        self.run_blocking("log body source", move || {
            if runtime
                .captures(session_id, &[id])
                .map_err(map_error)?
                .first()
                .is_some_and(|s| s.request.version.starts_with("WebSocket/"))
            {
                return Err(ManagerError::bad_request(
                    "WebSocket containers are internal; read individual messages",
                ));
            }
            let redactor = ReadRedactor::prepare(&policy)?;
            if !runtime.has_capture(session_id, id).map_err(map_error)? {
                return Err(ManagerError::new(
                    ErrorCode::LogNotFound,
                    format!("log {id} not found"),
                ));
            }
            if let Some(redactor) = redactor {
                let summary = runtime
                    .captures(session_id, &[id])
                    .map_err(map_error)?
                    .pop()
                    .ok_or_else(|| ManagerError::not_found("log not found"))?;
                return redactor
                    .view(&runtime, &summary)?
                    .body_source(side)
                    .map_err(|_| redaction::failed())?
                    .ok_or_else(|| ManagerError::new(ErrorCode::BodyNotFound, "body not found"));
            }
            runtime
                .capture_body_source(session_id, id, side)
                .map_err(|error| ManagerError::new(ErrorCode::BodyReadFailed, error.to_string()))?
                .ok_or_else(|| {
                    ManagerError::new(ErrorCode::BodyNotFound, format!("{side:?} body not found"))
                })
        })
        .await
    }

    async fn interceptor_snapshot_body_source(
        &self,
        session_id: Option<u64>,
        capture_id: u64,
        execution_id: u64,
        side: BodySide,
    ) -> ManagerResult<BodySource> {
        let session_id = self.session_id(session_id)?;
        let runtime = self.runtime.clone();
        let policy = self.read_policy.clone();
        self.run_blocking("interceptor snapshot body source", move || {
            let redactor = ReadRedactor::prepare(&policy)?;
            if let Some(redactor) = redactor {
                let (_, view) =
                    redactor.snapshot(&runtime, session_id, capture_id, execution_id)?;
                return view
                    .body_source(side)
                    .map_err(|_| redaction::failed())?
                    .ok_or_else(|| {
                        ManagerError::new(
                            ErrorCode::SnapshotBodyNotFound,
                            "snapshot body not found",
                        )
                    });
            }
            runtime
                .interceptor_snapshot_body_source(session_id, capture_id, execution_id, side)
                .map_err(|error| ManagerError::new(ErrorCode::BodyReadFailed, error.to_string()))?
                .ok_or_else(|| {
                    ManagerError::new(
                        ErrorCode::SnapshotBodyNotFound,
                        format!("interceptor execution {execution_id} snapshot body not found"),
                    )
                })
        })
        .await
    }

    async fn interceptor_script_content(
        &self,
        session_id: Option<u64>,
        capture_id: u64,
        execution_id: u64,
    ) -> ManagerResult<InterceptorScriptContent> {
        let session_id = self.session_id(session_id)?;
        let runtime = self.runtime.clone();
        self.run_blocking("interceptor script content", move || {
            runtime
                .interceptor_script_content(session_id, capture_id, execution_id)
                .map_err(map_error)?
                .ok_or_else(|| {
                    ManagerError::new(
                        ErrorCode::ExecutionNotFound,
                        format!("interceptor execution {execution_id} not found"),
                    )
                })
        })
        .await
    }

    async fn interceptor_snapshot(
        &self,
        session_id: Option<u64>,
        capture_id: u64,
        execution_id: u64,
    ) -> ManagerResult<InterceptorSnapshot> {
        let session_id = self.session_id(session_id)?;
        let runtime = self.runtime.clone();
        let policy = self.read_policy.clone();
        self.run_blocking("interceptor snapshot", move || {
            let redactor = ReadRedactor::prepare(&policy)?;
            if let Some(redactor) = redactor {
                let (snapshot, view) =
                    redactor.snapshot(&runtime, session_id, capture_id, execution_id)?;
                return redaction::snapshot_output(snapshot, &view);
            }
            runtime
                .interceptor_snapshot(session_id, capture_id, execution_id)
                .map_err(map_error)?
                .ok_or_else(|| {
                    ManagerError::new(
                        ErrorCode::SnapshotNotFound,
                        format!("interceptor execution {execution_id} has no snapshot"),
                    )
                })
        })
        .await
    }

    async fn breakpoints(&self, query: BreakpointQuery) -> ManagerResult<Vec<BreakpointSummary>> {
        let session_id = self.session_id(query.session_id)?;
        let runtime = self.runtime.clone();
        let policy = self.read_policy.clone();
        self.run_blocking("breakpoint list", move || {
            let redactor = ReadRedactor::prepare(&policy)?;
            let mut rows = runtime.breakpoints(&BreakpointListFilter {
                session_id,
                phase: query.phase,
                interceptor_name: query.interceptor_name,
            });
            if let Some(redactor) = redactor {
                for row in &mut rows {
                    row.uri = redactor.uri(&row.uri)?;
                }
            }
            Ok(rows)
        })
        .await
    }

    async fn breakpoint(&self, id: u64) -> ManagerResult<BreakpointDetailPayload> {
        let runtime = self.runtime.clone();
        let policy = self.read_policy.clone();
        self.run_blocking("breakpoint read", move || {
            let redactor = ReadRedactor::prepare(&policy)?;
            let mut detail = runtime.breakpoint(id).map_err(map_error)?;
            if let Some(redactor) = redactor {
                let live_runtime = runtime.clone();
                let view = redactor.with_sources(detail.capture.summary.clone(), move |side| {
                    live_runtime.breakpoint_body_source(id, side)
                })?;
                detail.capture.request_body = redaction::payload(
                    view.body_source(BodySide::Request)
                        .map_err(|_| redaction::failed())?,
                )?;
                detail.capture.response_body = redaction::payload(
                    view.body_source(BodySide::Response)
                        .map_err(|_| redaction::failed())?,
                )?;
                detail.capture.summary = view.summary();
                detail.breakpoint.uri = detail.capture.summary.request.uri.clone();
            }
            Ok(BreakpointDetailPayload {
                breakpoint: detail.breakpoint,
                log: log_detail_from_capture(detail.capture),
            })
        })
        .await
    }

    async fn breakpoint_body_source(&self, id: u64, side: BodySide) -> ManagerResult<BodySource> {
        let runtime = self.runtime.clone();
        let policy = self.read_policy.clone();
        self.run_blocking("breakpoint body source", move || {
            let redactor = ReadRedactor::prepare(&policy)?;
            if let Some(redactor) = redactor {
                let detail = runtime.breakpoint(id).map_err(map_error)?;
                let live_runtime = runtime.clone();
                return redactor
                    .with_sources(detail.capture.summary, move |side| {
                        live_runtime.breakpoint_body_source(id, side)
                    })?
                    .body_source(side)
                    .map_err(|_| redaction::failed())?
                    .ok_or_else(|| ManagerError::new(ErrorCode::BodyNotFound, "body not found"));
            }
            runtime
                .breakpoint_body_source(id, side)
                .map_err(map_error)?
                .ok_or_else(|| {
                    ManagerError::new(ErrorCode::BodyNotFound, format!("{side:?} body not found"))
                })
        })
        .await
    }

    async fn extend_breakpoint(
        &self,
        id: u64,
        request: ExtendBreakpointRequest,
    ) -> ManagerResult<BreakpointSummary> {
        let runtime = self.runtime.clone();
        let policy = self.read_policy.clone();
        self.run_blocking("breakpoint extend", move || {
            let redactor = ReadRedactor::prepare(&policy)?;
            let mut result = runtime
                .extend_breakpoint(id, request.timeout_ms)
                .map_err(map_error)?;
            if let Some(redactor) = redactor {
                result.uri = redactor.uri(&result.uri)?;
            }
            Ok(result)
        })
        .await
    }

    async fn release_breakpoint(&self, id: u64) -> ManagerResult<()> {
        self.runtime.release_breakpoint(id).map_err(map_error)
    }

    async fn execute_breakpoint_script(
        &self,
        id: u64,
        request: ExecuteTemporaryScriptRequest,
    ) -> ManagerResult<TemporaryExecutionResult> {
        let runtime = self.runtime.clone();
        let policy = self.read_policy.clone();
        self.run_blocking("breakpoint script", move || {
            let redactor = ReadRedactor::prepare(&policy)?;
            let mut result = runtime
                .execute_breakpoint_script(id, &request.content)
                .map_err(map_error)?;
            if let Some(redactor) = redactor {
                result.breakpoint.uri = redactor.uri(&result.breakpoint.uri)?;
            }
            Ok(result)
        })
        .await
    }

    async fn session_view(&self, session_id: Option<u64>) -> ManagerResult<SessionViewPayload> {
        let session_id = self.session_id(session_id)?;
        let view = self.runtime.session_view(session_id).map_err(map_error)?;
        Ok(SessionViewPayload {
            redaction_revision: self
                .read_policy
                .as_ref()
                .map(|p| proxy_crab_mitm::model::script_content_hash(&p.script.content)),
            session_id,
            columns: view.columns,
            filter: view.filter,
        })
    }

    async fn replace_session_filter(
        &self,
        session_id: u64,
        filter: SessionFilter,
    ) -> ManagerResult<SessionViewPayload> {
        let view = self
            .runtime
            .replace_session_filter(session_id, filter)
            .map_err(map_error)?;
        Ok(SessionViewPayload {
            redaction_revision: self
                .read_policy
                .as_ref()
                .map(|p| proxy_crab_mitm::model::script_content_hash(&p.script.content)),
            session_id,
            columns: view.columns,
            filter: view.filter,
        })
    }

    async fn replace_session_view(
        &self,
        session_id: Option<u64>,
        request: ReplaceSessionViewRequest,
    ) -> ManagerResult<SessionViewPayload> {
        let session_id = self.session_id(session_id)?;
        let view = self
            .runtime
            .replace_session_columns(session_id, request.columns)
            .map_err(map_error)?;
        Ok(SessionViewPayload {
            redaction_revision: self
                .read_policy
                .as_ref()
                .map(|p| proxy_crab_mitm::model::script_content_hash(&p.script.content)),
            session_id,
            columns: view.columns,
            filter: view.filter,
        })
    }

    async fn column_scripts(&self) -> ManagerResult<Vec<Script>> {
        self.runtime.scripts(ScriptKind::Column).map_err(map_error)
    }

    async fn create_column_script(&self, request: ScriptRequest) -> ManagerResult<()> {
        self.runtime
            .create_script(
                ScriptKind::Column,
                Script {
                    name: request.name,
                    content: request.content,
                },
            )
            .map_err(map_error)
    }

    async fn column_script(&self, name: String) -> ManagerResult<Script> {
        self.runtime
            .script(ScriptKind::Column, &name)
            .map_err(map_error)
    }

    async fn update_column_script(
        &self,
        name: String,
        request: UpdateScriptRequest,
    ) -> ManagerResult<()> {
        self.runtime
            .update_script(ScriptKind::Column, &name, request.content)
            .map_err(map_error)
    }

    async fn delete_column_script(&self, name: String) -> ManagerResult<()> {
        self.runtime
            .delete_script(ScriptKind::Column, &name)
            .map_err(map_error)
    }

    async fn filter_scripts(&self) -> ManagerResult<Vec<Script>> {
        self.runtime.scripts(ScriptKind::Filter).map_err(map_error)
    }

    async fn create_filter_script(&self, request: ScriptRequest) -> ManagerResult<()> {
        self.runtime
            .create_script(
                ScriptKind::Filter,
                Script {
                    name: request.name,
                    content: request.content,
                },
            )
            .map_err(map_error)
    }

    async fn filter_script(&self, name: String) -> ManagerResult<Script> {
        self.runtime
            .script(ScriptKind::Filter, &name)
            .map_err(map_error)
    }

    async fn update_filter_script(
        &self,
        name: String,
        request: UpdateScriptRequest,
    ) -> ManagerResult<()> {
        self.runtime
            .update_script(ScriptKind::Filter, &name, request.content)
            .map_err(map_error)
    }

    async fn delete_filter_script(&self, name: String) -> ManagerResult<()> {
        self.runtime
            .delete_script(ScriptKind::Filter, &name)
            .map_err(map_error)
    }

    async fn debug_filter_script(
        &self,
        name: String,
        request: DebugFilterScriptRequest,
    ) -> ManagerResult<bool> {
        let session_id = self.session_id(request.session_id)?;
        let runtime = self.runtime.clone();
        let policy = self.read_policy.clone();
        self.run_blocking("filter script", move || {
            let redactor = ReadRedactor::prepare(&policy)?;
            let script = runtime
                .script(ScriptKind::Filter, &name)
                .map_err(map_error)?;
            let detail = runtime
                .capture(session_id, request.log_id)
                .map_err(map_error)?
                .ok_or_else(|| {
                    ManagerError::not_found(format!("log {} not found", request.log_id))
                })?;
            let evaluator = FilterEvaluator::new(&script.content, &script.name)
                .map_err(|error| ManagerError::bad_request(error.to_string()))?;
            let view = redactor
                .as_ref()
                .map(|r| r.view(&runtime, &detail.summary))
                .transpose()?;
            let summary = view.as_ref().map(|v| v.summary()).unwrap_or(detail.summary);
            let bodies = view
                .as_ref()
                .map(|v| v.body_access())
                .unwrap_or_else(|| Self::capture_body_access(&runtime, &summary));
            let result = evaluator.evaluate_with_bodies(&request.input, &summary, bodies);
            if let Some(view) = &view {
                view.ensure_success().map_err(|_| redaction::failed())?;
            }
            result.map_err(|error| ManagerError::bad_request(error.to_string()))
        })
        .await
    }

    async fn routing_scripts(&self) -> ManagerResult<Vec<Script>> {
        self.runtime.scripts(ScriptKind::Routing).map_err(map_error)
    }

    async fn create_routing_script(&self, request: ScriptRequest) -> ManagerResult<()> {
        self.runtime
            .create_script(
                ScriptKind::Routing,
                Script {
                    name: request.name,
                    content: request.content,
                },
            )
            .map_err(map_error)
    }

    async fn routing_script(&self, name: String) -> ManagerResult<Script> {
        self.runtime
            .script(ScriptKind::Routing, &name)
            .map_err(map_error)
    }

    async fn update_routing_script(
        &self,
        name: String,
        request: UpdateScriptRequest,
    ) -> ManagerResult<()> {
        self.runtime
            .update_routing_script(&name, request.content)
            .await
            .map_err(map_error)
    }

    async fn delete_routing_script(&self, name: String) -> ManagerResult<()> {
        self.runtime
            .delete_routing_script(&name)
            .await
            .map_err(map_error)
    }

    async fn routing_selection(&self) -> ManagerResult<RoutingSelection> {
        Ok(RoutingSelection {
            name: self.runtime.config().routing_script_name,
        })
    }

    async fn replace_routing_selection(
        &self,
        selection: RoutingSelection,
    ) -> ManagerResult<RoutingSelection> {
        let name = self
            .runtime
            .replace_routing_selection(selection.name)
            .await
            .map_err(map_error)?;
        Ok(RoutingSelection { name })
    }

    async fn interceptors(&self, kind: InterceptorKind) -> ManagerResult<InterceptorLibraryList> {
        Ok(InterceptorLibraryList {
            kind,
            items: self.runtime.interceptor_library(kind).map_err(map_error)?,
        })
    }

    async fn create_interceptor(&self, request: InterceptorCreateRequest) -> ManagerResult<()> {
        let kind = interceptor_script_kind(request.kind);
        self.runtime
            .create_script(
                kind,
                Script {
                    name: request.name.clone(),
                    content: request.content,
                },
            )
            .map_err(map_error)?;
        Ok(())
    }

    async fn interceptor(
        &self,
        kind: InterceptorKind,
        name: String,
    ) -> ManagerResult<InterceptorDetail> {
        let script = self
            .runtime
            .script(interceptor_script_kind(kind), &name)
            .map_err(map_error)?;
        Ok(InterceptorDetail {
            kind,
            display_name: proxy_crab_mitm::model::script_display_name(&script.content)
                .map(str::to_owned),
            name: script.name,
            content: script.content,
        })
    }

    async fn update_interceptor(
        &self,
        kind: InterceptorKind,
        name: String,
        request: InterceptorUpdateRequest,
    ) -> ManagerResult<()> {
        let script_kind = interceptor_script_kind(kind);
        self.runtime
            .update_script(script_kind, &name, request.content)
            .map_err(map_error)?;
        Ok(())
    }

    async fn delete_interceptor(&self, kind: InterceptorKind, name: String) -> ManagerResult<()> {
        self.runtime
            .delete_script(interceptor_script_kind(kind), &name)
            .map_err(map_error)
    }

    async fn session_interceptors(
        &self,
        session_id: Option<u64>,
    ) -> ManagerResult<SessionInterceptorsPayload> {
        let session_id = self.session_id(session_id)?;
        let value = self
            .runtime
            .resolved_session_interceptors(session_id)
            .map_err(map_error)?;
        Ok(SessionInterceptorsPayload {
            session_id,
            request: value
                .request
                .into_iter()
                .map(|item| SessionInterceptorItem {
                    display_name: item.display_name,
                    name: item.name,
                    enabled: item.enabled,
                    valid: item.valid,
                })
                .collect(),
            response: value
                .response
                .into_iter()
                .map(|item| SessionInterceptorItem {
                    display_name: item.display_name,
                    name: item.name,
                    enabled: item.enabled,
                    valid: item.valid,
                })
                .collect(),
        })
    }

    async fn replace_session_interceptors(
        &self,
        session_id: Option<u64>,
        request: ReplaceSessionInterceptorsRequest,
    ) -> ManagerResult<SessionInterceptorsPayload> {
        let session_id = self.session_id(session_id)?;
        self.runtime
            .replace_session_interceptors(
                session_id,
                SessionInterceptors {
                    request: request
                        .request
                        .into_iter()
                        .map(|item| SessionInterceptor {
                            name: item.name,
                            enabled: item.enabled,
                        })
                        .collect(),
                    response: request
                        .response
                        .into_iter()
                        .map(|item| SessionInterceptor {
                            name: item.name,
                            enabled: item.enabled,
                        })
                        .collect(),
                },
            )
            .map_err(map_error)?;
        self.session_interceptors(Some(session_id)).await
    }

    async fn clash_subscription(&self) -> ManagerResult<ClashSubscriptionState> {
        Ok(self.clash_subscription.state())
    }

    async fn enable_clash_subscription(
        &self,
        request: EnableClashSubscriptionRequest,
    ) -> ManagerResult<ClashSubscriptionState> {
        self.clash_subscription.enable(&request.server)
    }

    async fn disable_clash_subscription(&self) -> ManagerResult<ClashSubscriptionState> {
        Ok(self.clash_subscription.disable())
    }

    async fn clash_subscription_yaml(&self) -> ManagerResult<String> {
        self.clash_subscription
            .yaml(self.runtime.config().proxy_port)
    }

    async fn clash_config(&self) -> ManagerResult<ClashConfig> {
        self.clash_subscription.config()
    }

    async fn save_clash_config(
        &self,
        request: SaveClashConfigRequest,
    ) -> ManagerResult<ClashConfig> {
        self.clash_subscription.save_config(&request.yaml)
    }

    async fn reset_clash_config(&self) -> ManagerResult<ClashConfig> {
        self.clash_subscription.reset_config()
    }

    async fn certificate(&self) -> ManagerResult<CertificateResponse> {
        Ok(CertificateResponse {
            pem: self.runtime.certificate_pem(),
        })
    }

    async fn regenerate_certificate(&self) -> ManagerResult<CertificateResponse> {
        self.runtime
            .regenerate_ca()
            .await
            .map(|pem| CertificateResponse { pem })
            .map_err(map_error)
    }

    async fn system_logs(&self, query: SystemLogsQuery) -> ManagerResult<Vec<SystemLogEntry>> {
        Ok(self
            .runtime
            .system_logs(query.after_seq, query.limit.unwrap_or(1000).min(10_000)))
    }

    async fn clear_system_logs(&self) -> ManagerResult<()> {
        self.runtime.clear_system_logs();
        Ok(())
    }

    async fn bypass_entries(&self, query: BypassQuery) -> ManagerResult<BypassPage> {
        let limit = query.limit.unwrap_or(200).clamp(1, 1000);
        let runtime = self.runtime.clone();
        let policy = self.read_policy.clone();
        self.run_blocking("bypass read", move || {
            let redactor = ReadRedactor::prepare(&policy)?;
            let mut rows = runtime
                .bypass_entries(limit + 1, query.before_id)
                .map_err(map_error)?;
            let has_more = rows.len() > limit;
            rows.truncate(limit);
            if let Some(redactor) = redactor {
                for row in &mut rows {
                    row.uri = redactor.uri(&row.uri)?;
                }
            }
            Ok(BypassPage { rows, has_more })
        })
        .await
    }

    async fn delete_bypass_entry(&self, id: u64) -> ManagerResult<()> {
        let runtime = self.runtime.clone();
        self.run_blocking("bypass delete", move || {
            runtime.delete_bypass_entry(id).map_err(map_error)
        })
        .await
    }

    async fn delete_bypass_entries(&self, ids: Vec<u64>) -> ManagerResult<DeleteCount> {
        let runtime = self.runtime.clone();
        self.run_blocking("bypass delete", move || {
            runtime
                .delete_bypass_entries(&ids)
                .map(|deleted| DeleteCount { deleted })
                .map_err(map_error)
        })
        .await
    }

    async fn clear_bypass_entries(&self) -> ManagerResult<DeleteCount> {
        let runtime = self.runtime.clone();
        self.run_blocking("bypass clear", move || {
            runtime
                .clear_bypass_entries()
                .map(|deleted| DeleteCount { deleted })
                .map_err(map_error)
        })
        .await
    }
}

fn flatten_headers(headers: &HeaderValues) -> Vec<HeaderItem> {
    headers
        .iter()
        .flat_map(|(name, values)| {
            values.iter().map(|value| HeaderItem {
                name: name.clone(),
                value: value.clone(),
            })
        })
        .collect()
}

fn log_detail_from_capture(detail: CaptureDetail) -> LogDetail {
    let id = detail.summary.id;
    let session_id = detail.summary.session_id;
    LogDetail {
        websocket: None,
        id,
        session_id,
        created_at: detail.summary.created_at,
        updated_at: detail.summary.updated_at,
        source_type: "ip".into(),
        source_addr: Some(detail.summary.source),
        stage: detail.summary.stage,
        outcome: outcome_name(detail.summary.outcome).into(),
        error: detail.summary.error,
        request: RequestDetail {
            method: detail.summary.request.method,
            uri: detail.summary.request.uri,
            version: detail.summary.request.version,
            headers: flatten_headers(&detail.summary.request.headers),
            tags: detail.summary.request.tags,
            body: detail.request_body,
        },
        response: detail.summary.response.map(|response| ResponseDetail {
            status: response.status,
            status_text: status_text(response.status),
            version: response.version,
            headers: flatten_headers(&response.headers),
            body: detail.response_body,
        }),
        request_interceptors: detail.request_interceptors,
        response_interceptors: detail.response_interceptors,
    }
}

fn status_text(status: u16) -> String {
    axum::http::StatusCode::from_u16(status)
        .ok()
        .and_then(|status| status.canonical_reason())
        .unwrap_or_default()
        .to_owned()
}

fn outcome_name(outcome: CaptureOutcome) -> &'static str {
    match outcome {
        CaptureOutcome::InProgress => "in_progress",
        CaptureOutcome::Success => "success",
        CaptureOutcome::Failed => "failed",
        CaptureOutcome::Tunneled => "tunneled",
    }
}

fn interceptor_script_kind(kind: InterceptorKind) -> ScriptKind {
    match kind {
        InterceptorKind::Request => ScriptKind::RequestInterceptor,
        InterceptorKind::Response => ScriptKind::ResponseInterceptor,
    }
}

fn map_error(error: anyhow::Error) -> ManagerError {
    use proxy_crab_mitm::error::Error as MitmError;
    let Some(mitm) = error.downcast_ref::<MitmError>() else {
        return ManagerError::internal(error.to_string());
    };
    let message = mitm.to_string();
    match mitm {
        MitmError::SessionNotFound(_)
        | MitmError::ArchivedSessionNotFound(_)
        | MitmError::ScriptNotFound(_)
        | MitmError::CaptureNotFound(_)
        | MitmError::BreakpointNotFound(_)
        | MitmError::BypassEntryNotFound(_) => ManagerError::not_found(message),
        MitmError::SessionAlreadyExists(_)
        | MitmError::ArchivedSessionAlreadyExists(_)
        | MitmError::ScriptAlreadyExists(_)
        | MitmError::ProxyMustBeStopped
        | MitmError::BypassEntryInProgress(_) => ManagerError::conflict(message),
        MitmError::ActiveSessionArchive(_) | MitmError::SessionRequestsInProgress(_) => {
            ManagerError::new(ErrorCode::SessionInUse, message)
        }
        MitmError::ProxyAlreadyRunning => ManagerError::new(ErrorCode::ProxyRunning, message),
        MitmError::InvalidScriptName
        | MitmError::InvalidScript(_)
        | MitmError::InvalidArgument(_) => ManagerError::bad_request(message),
    }
}

pub fn map_asset_error(error: AssetError) -> ManagerError {
    match error {
        AssetError::InvalidId(message) => ManagerError::new(
            ErrorCode::InvalidAssetId,
            format!("invalid asset id: {message}"),
        ),
        AssetError::NotFound(id) => {
            ManagerError::new(ErrorCode::AssetNotFound, format!("asset {id} not found"))
        }
        AssetError::AlreadyExists(id) => ManagerError::new(
            ErrorCode::AssetAlreadyExists,
            format!("asset {id} already exists"),
        ),
        AssetError::PathConflict(id) => ManagerError::new(
            ErrorCode::AssetPathConflict,
            format!("asset path conflicts with an existing file or directory: {id}"),
        ),
        AssetError::Storage(error) => {
            ManagerError::new(ErrorCode::AssetStoreFailed, error.to_string())
        }
        AssetError::InvalidMetadata(error) => {
            ManagerError::new(ErrorCode::AssetStoreFailed, error.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, sync::Arc};

    use proxy_crab_mitm::{
        ProxyCrab,
        log_buffer::LogBuffer,
        lua::CaptureBodyAccess,
        model::{
            BodySourceType, CaptureOutcome, CaptureSummary, Column, FilterColumn, FilterOption,
            HeaderValues, InterceptorKind, NewSession, RequestData, ResponseData, SessionFilter,
        },
        storage::{BodySide, CaptureStore},
    };
    use tempfile::tempdir;

    use crate::dto::{
        CreateAgentsPresetRequest, DebugFilterScriptRequest, ErrorCode, InterceptorCreateRequest,
        LogIdsRequest, LogViewItem, LogViewsRequest, ReplaceSessionInterceptorsRequest,
        ReplaceSessionViewRequest, ScriptRequest, SessionInterceptorInput, SessionViewInput,
        UpdateAgentsPresetRequest, UpdateScriptRequest,
    };

    use super::{
        MAX_BLOCKING_MANAGEMENT_TASKS, MitmManager, PreparedFilter, ProxyCrabManager, status_text,
    };

    #[tokio::test]
    async fn websocket_reads_apply_policy_and_never_expose_containers() {
        let dir = tempdir().unwrap();
        let runtime = ProxyCrab::open(dir.path(), Arc::new(LogBuffer::default())).unwrap();
        let session = runtime.create_session(NewSession::default()).unwrap();
        let store = CaptureStore::open(session.id, runtime.workspace().root()).unwrap();
        let handshake = store
            .begin("test", &request("handshake"), "request")
            .unwrap();
        let mut req = request("socket");
        req.version = "WebSocket/13".into();
        req.uri = "wss://example.com/ws?secret=1".into();
        let id = store.begin("test", &req, "websocket").unwrap();
        let db = rusqlite::Connection::open(
            runtime
                .workspace()
                .root()
                .join("sessions")
                .join(session.id.to_string())
                .join("captures.db"),
        )
        .unwrap();
        db.execute("INSERT INTO websocket_captures(capture_id,handshake_id,extensions,opened_at,close_reason) VALUES (?1,?2,'',1,'sensitive close')", rusqlite::params![id as i64,handshake as i64]).unwrap();
        let mut bytes = b"PCWS\x01\0\0\0".to_vec();
        for (message_id, payload, available) in [
            (1u32, b"secret".as_slice(), true),
            (2, b"".as_slice(), false),
        ] {
            let metadata = serde_json::json!({"id":message_id,"type":"text","started_at":1,"completed_at":2,"size":if available {6} else {16777217},"content_available":available,"omitted_reason":if available {None} else {Some("size_limit_exceeded")},"error":"private delivery","errors":[{"execution_id":1,"name":"test","invocation_id":1,"error":"private callback payload"}]});
            let meta = serde_json::to_vec(&metadata).unwrap();
            bytes.extend(message_id.to_le_bytes());
            bytes.extend(((4 + meta.len() + payload.len()) as u32).to_le_bytes());
            bytes.extend((meta.len() as u32).to_le_bytes());
            bytes.extend(meta);
            bytes.extend(payload);
        }
        store.save_body(id, BodySide::Request, &bytes).unwrap();
        let manager = MitmManager::new(runtime.clone());
        let page = manager
            .websocket_messages(Some(session.id), id, BodySide::Request, 0, 1)
            .await
            .unwrap();
        assert_eq!(page.messages.len(), 1);
        assert!(page.has_more);
        assert_eq!(page.next_after_id, 1);
        let page = manager
            .websocket_messages(Some(session.id), id, BodySide::Request, 2, 1)
            .await
            .unwrap();
        assert!(page.messages.is_empty());
        assert_eq!(page.next_after_id, 2);
        assert!(
            manager
                .websocket_messages(Some(session.id), id, BodySide::Request, 0, 2001)
                .await
                .is_err()
        );
        assert_eq!(
            manager
                .log_body_source(Some(session.id), id, BodySide::Request)
                .await
                .unwrap_err()
                .code,
            ErrorCode::BadRequest
        );
        assert!(matches!(
            manager
                .log(Some(session.id), id)
                .await
                .unwrap()
                .request
                .body,
            proxy_crab_mitm::model::BodyPayload::Empty
        ));
        assert_eq!(
            manager
                .websocket_message_body(Some(session.id), id, BodySide::Request, 1, 2)
                .await
                .unwrap_err()
                .code,
            ErrorCode::BodyTooLarge
        );
        assert_eq!(
            manager
                .websocket_message_body(Some(session.id), id, BodySide::Request, 2, 16777216)
                .await
                .unwrap_err()
                .code,
            ErrorCode::MessageContentUnavailable
        );
        for (name, script, expected) in [
            ("old", "", Some(ErrorCode::RedactionFailed)),
            (
                "fail",
                "-- @proxycrab websocket-messages\nerror('private')",
                Some(ErrorCode::RedactionFailed),
            ),
            (
                "hide",
                "-- @proxycrab websocket-messages\nif context.kind == 'websocket_message' then req.body:hide() end",
                Some(ErrorCode::MessageContentHidden),
            ),
            (
                "mask",
                "-- @proxycrab websocket-messages\nif context.kind == 'websocket_message' then assert(resp == nil); req.body:replace_with_string('safe') elseif req then req.uri = 'wss://example.com/safe' end",
                None,
            ),
        ] {
            manager
                .create_redaction_script(ScriptRequest {
                    name: name.into(),
                    content: script.into(),
                })
                .await
                .unwrap();
            let reader = manager
                .reader(
                    Some(name),
                    "/api/logs/{id}/websocket/messages/{message_id}/body",
                )
                .unwrap();
            let safe_page = reader
                .websocket_messages(Some(session.id), id, BodySide::Request, 0, 1)
                .await
                .unwrap();
            assert_eq!(
                safe_page.messages[0].error.as_deref(),
                Some("WebSocket delivery failed")
            );
            assert_eq!(
                safe_page.messages[0].errors[0].error,
                "WebSocket callback failed"
            );
            let result = reader
                .websocket_message_body(Some(session.id), id, BodySide::Request, 1, 16_777_216)
                .await;
            if let Some(code) = expected {
                assert_eq!(result.unwrap_err().code, code);
            } else {
                let source = result.unwrap();
                assert!(
                    matches!(source.data, proxy_crab_mitm::storage::BodySourceData::Bytes(ref b) if b == b"safe")
                );
                let detail = reader.log(Some(session.id), id).await.unwrap();
                assert_eq!(detail.request.uri, "wss://example.com/safe");
                assert!(detail.websocket.unwrap().close_reason.is_none());
            }
        }
        let source = manager
            .websocket_message_body(Some(session.id), id, BodySide::Request, 1, 16_777_216)
            .await
            .unwrap();
        assert!(
            matches!(source.data, proxy_crab_mitm::storage::BodySourceData::Bytes(ref b) if b == b"secret")
        );

        use axum::{
            body::{Body, to_bytes},
            http::Request,
        };
        use tower::ServiceExt;
        struct BoundPolicy;
        #[async_trait::async_trait]
        impl crate::permission::PermissionManager for BoundPolicy {
            async fn check_permission(
                &self,
                _: crate::permission::PermissionAction,
            ) -> Option<crate::permission::PermissionDenied> {
                None
            }
            async fn redaction_script(
                &self,
                _: &crate::permission::ManagementCredential,
            ) -> Result<Option<String>, crate::permission::PermissionDenied> {
                Ok(Some("mask".into()))
            }
        }
        let app = crate::http::router(manager.clone(), Arc::new(BoundPolicy));
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/logs/{id}/websocket/messages/1/body?session_id={}&side=request",
                        session.id
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(
            to_bytes(response.into_body(), 100).await.unwrap().as_ref(),
            b"safe"
        );
        for (path, status) in [
            (
                format!("/api/logs/{id}/body?session_id={}&side=request", session.id),
                400,
            ),
            (
                format!(
                    "/api/logs/{id}/websocket/messages?session_id={}&side=invalid",
                    session.id
                ),
                400,
            ),
            (
                format!(
                    "/api/logs/{id}/websocket/messages?session_id={}&side=request&limit=2001",
                    session.id
                ),
                400,
            ),
        ] {
            assert_eq!(
                app.clone()
                    .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                    .await
                    .unwrap()
                    .status(),
                status
            );
        }
        let shared_manager: Arc<dyn ProxyCrabManager> = manager;
        let shares = crate::session_share::SessionShareService::new();
        let token = shares
            .enable_with_script(&shared_manager, session.id, Some("mask".into()))
            .await
            .unwrap()
            .token
            .unwrap();
        let shared_app = crate::session_share::router(shared_manager, shares);
        let response = shared_app.oneshot(Request::builder().uri(format!("/share-api/logs/{id}/websocket/messages/1/body?session_id=999&side=request&token={token}")).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(
            to_bytes(response.into_body(), 100).await.unwrap().as_ref(),
            b"safe"
        );
    }

    #[test]
    fn response_status_text_uses_the_canonical_reason() {
        assert_eq!(status_text(200), "OK");
        assert_eq!(status_text(404), "Not Found");
        assert_eq!(status_text(999), "");
    }

    #[test]
    fn log_id_in_progress_requires_current_activity_membership() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let item = CaptureSummary {
            id: 7,
            session_id: 3,
            source: "127.0.0.1".into(),
            request: request("active"),
            response: None,
            outcome: CaptureOutcome::InProgress,
            stage: "request".into(),
            error: None,
            created_at: 1,
            updated_at: 1,
        };
        let mut matched = Vec::new();
        let mut in_progress = Vec::new();
        MitmManager::classify_log_id(
            &PreparedFilter::All,
            &item,
            &BTreeSet::new(),
            CaptureBodyAccess::unavailable,
            &mut matched,
            &mut in_progress,
            None,
            &runtime,
        )
        .unwrap();
        assert_eq!(matched, vec![7]);
        assert!(in_progress.is_empty());

        MitmManager::classify_log_id(
            &PreparedFilter::All,
            &item,
            &BTreeSet::from([7]),
            CaptureBodyAccess::unavailable,
            &mut Vec::new(),
            &mut in_progress,
            None,
            &runtime,
        )
        .unwrap();
        assert_eq!(in_progress, vec![7]);
    }

    #[test]
    fn blocking_management_tasks_are_limited_to_eight() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let manager = MitmManager::new(runtime);

        assert_eq!(MAX_BLOCKING_MANAGEMENT_TASKS, 8);
        let permits = (0..8)
            .map(|_| manager.blocking_tasks.clone().try_acquire_owned().unwrap())
            .collect::<Vec<_>>();
        assert!(manager.blocking_tasks.clone().try_acquire_owned().is_err());
        drop(permits);
        assert_eq!(manager.blocking_tasks.available_permits(), 8);
    }

    #[tokio::test]
    async fn agent_presets_are_managed_through_the_manager() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let manager = MitmManager::new(runtime);

        let state = manager
            .create_agents_preset(CreateAgentsPresetRequest {
                name: "自定义".into(),
            })
            .await
            .unwrap();
        let id = state
            .presets
            .iter()
            .find(|preset| preset.name == "自定义")
            .unwrap()
            .id
            .clone();
        manager
            .update_agents_preset(
                id.clone(),
                UpdateAgentsPresetRequest {
                    name: None,
                    content: Some("# Custom".into()),
                },
            )
            .await
            .unwrap();
        manager.activate_agents_preset(id).await.unwrap();

        assert_eq!(manager.agents_markdown().await.unwrap(), "# Custom");
    }

    #[tokio::test]
    async fn log_ids_can_filter_without_persisting_session_state() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let session = runtime.create_session(NewSession::default()).unwrap();
        let store = CaptureStore::open(session.id, runtime.workspace().root()).unwrap();
        let first = store
            .begin("127.0.0.1", &request("first"), "request")
            .unwrap();
        store
            .begin("127.0.0.1", &request("second"), "request")
            .unwrap();
        let manager = MitmManager::new(runtime);
        let original = manager.session_view(Some(session.id)).await.unwrap().filter;

        let result = manager
            .log_ids(LogIdsRequest {
                session_id: Some(session.id),
                filter: Some(SessionFilter {
                    option: Some(FilterOption::Column {
                        column: FilterColumn::Uri,
                        regex: false,
                    }),
                    input: "/first".into(),
                }),
                ids: None,
                min_id: None,
                max_id: None,
                limit: None,
            })
            .await
            .unwrap();

        assert_eq!(result.matched_ids, vec![first]);
        assert!(result.in_progress_ids.is_empty());
        assert_eq!(
            manager.session_view(Some(session.id)).await.unwrap().filter,
            original
        );

        let explicit = manager
            .log_ids(LogIdsRequest {
                session_id: Some(session.id),
                filter: Some(SessionFilter {
                    option: Some(FilterOption::Column {
                        column: FilterColumn::Uri,
                        regex: false,
                    }),
                    input: "/first".into(),
                }),
                ids: Some(vec![first]),
                min_id: None,
                max_id: None,
                limit: None,
            })
            .await
            .unwrap();
        assert_eq!(explicit.matched_ids, vec![first]);
        assert!(explicit.in_progress_ids.is_empty());

        store
            .mark_in_progress_range_as_shutdown(first, first)
            .unwrap();
        let completed = manager
            .log_ids(LogIdsRequest {
                session_id: Some(session.id),
                filter: Some(SessionFilter {
                    option: Some(FilterOption::Column {
                        column: FilterColumn::Uri,
                        regex: false,
                    }),
                    input: "/first".into(),
                }),
                ids: Some(vec![first]),
                min_id: None,
                max_id: None,
                limit: None,
            })
            .await
            .unwrap();
        assert_eq!(completed.matched_ids, vec![first]);
        assert!(completed.in_progress_ids.is_empty());

        let invalid = manager
            .log_ids(LogIdsRequest {
                session_id: Some(session.id),
                filter: None,
                ids: Some(vec![first]),
                min_id: Some(0),
                max_id: None,
                limit: None,
            })
            .await
            .unwrap_err();
        assert_eq!(invalid.code, ErrorCode::BadRequest);
    }

    #[tokio::test]
    async fn replacing_session_filter_preserves_columns() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let session = runtime.create_session(NewSession::default()).unwrap();
        let manager = MitmManager::new(runtime);
        let before = manager.session_view(Some(session.id)).await.unwrap();
        let filter = SessionFilter {
            option: Some(FilterOption::Column {
                column: FilterColumn::Uri,
                regex: false,
            }),
            input: "needle".into(),
        };

        let after = manager
            .replace_session_filter(session.id, filter.clone())
            .await
            .unwrap();

        assert_eq!(after.columns, before.columns);
        assert_eq!(after.filter, filter);
    }

    fn request(path: &str) -> RequestData {
        RequestData {
            method: "GET".into(),
            uri: format!("http://example.com/{path}"),
            version: "HTTP/1.1".into(),
            headers: HeaderValues::new(),
            tags: Default::default(),
        }
    }

    #[tokio::test]
    async fn log_ids_apply_exclusive_bounds_and_batch_views_report_cell_errors() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let session = runtime.create_session(NewSession::default()).unwrap();
        let store = CaptureStore::open(session.id, runtime.workspace().root()).unwrap();
        let first = store
            .begin("127.0.0.1", &request("first"), "request")
            .unwrap();
        let second = store
            .begin("127.0.0.1", &request("second"), "request")
            .unwrap();
        let third = store
            .begin("127.0.0.1", &request("third"), "request")
            .unwrap();
        let manager = MitmManager::new(runtime);

        let ids = manager
            .log_ids(LogIdsRequest {
                session_id: Some(session.id),
                filter: None,
                ids: None,
                min_id: Some(first),
                max_id: Some(third),
                limit: None,
            })
            .await
            .unwrap();
        assert_eq!(ids.matched_ids, vec![second]);
        assert!(ids.in_progress_ids.is_empty());

        manager
            .create_column_script(ScriptRequest {
                name: "broken".into(),
                content: "error(\"boom\")".into(),
            })
            .await
            .unwrap();
        manager
            .replace_session_view(
                Some(session.id),
                ReplaceSessionViewRequest {
                    columns: vec![
                        Column::Method { width: 50.0 },
                        Column::CreatedAt { width: 200.0 },
                        Column::UpdatedAt { width: 200.0 },
                        Column::Script {
                            width: 100.0,
                            script_name: "broken".into(),
                        },
                    ],
                },
            )
            .await
            .unwrap();
        let payload = manager
            .log_views(LogViewsRequest {
                session_id: Some(session.id),
                logs: vec![
                    LogViewItem {
                        id: second,
                        updated_at: None,
                    },
                    LogViewItem {
                        id: third + 100,
                        updated_at: None,
                    },
                ],
                view: None,
            })
            .await
            .unwrap();

        assert_eq!(payload.columns.len(), 4);
        assert_eq!(
            payload.rows[0].cells,
            vec![
                "GET".to_string(),
                payload.rows[0].created_at.to_string(),
                payload.rows[0].updated_at.to_string(),
                String::new(),
            ]
        );
        assert!(payload.rows[0].created_at > 0);
        assert_eq!(payload.rows[0].outcome, "in_progress");
        assert_eq!(payload.exceptions.len(), 2);
        assert!(payload.exceptions.iter().any(|error| {
            error.id == second
                && error.column_index == Some(3)
                && error.code == "column_script_error"
        }));
        assert!(
            payload
                .exceptions
                .iter()
                .any(|error| error.code == "log_not_found")
        );

        let error = manager
            .log_views(LogViewsRequest {
                session_id: Some(session.id),
                logs: (0..201)
                    .map(|id| LogViewItem {
                        id,
                        updated_at: None,
                    })
                    .collect(),
                view: None,
            })
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::BadRequest);
    }

    #[tokio::test]
    async fn historical_bodies_are_available_to_filter_and_column_scripts() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let session = runtime.create_session(NewSession::default()).unwrap();
        let store = CaptureStore::open(session.id, runtime.workspace().root()).unwrap();
        let request = RequestData {
            headers: HeaderValues::from([("content-type".into(), vec!["application/json".into()])]),
            ..request("body")
        };
        let response = ResponseData {
            status: 200,
            version: "HTTP/1.1".into(),
            headers: HeaderValues::from([("content-type".into(), vec!["text/plain".into()])]),
        };
        let completed = store.begin("127.0.0.1", &request, "request").unwrap();
        store
            .save_body(completed, BodySide::Request, br#"{"order_id":"42"}"#)
            .unwrap();
        store
            .save_body(completed, BodySide::Response, b"original response")
            .unwrap();
        store
            .complete(
                completed,
                &response,
                &BodySourceType::String {
                    content: "completed response".into(),
                },
            )
            .unwrap();

        let in_progress = store.begin("127.0.0.1", &request, "request").unwrap();
        store
            .save_body(in_progress, BodySide::Request, br#"{"order_id":"42"}"#)
            .unwrap();
        store
            .update_response(in_progress, &response, &BodySourceType::Original)
            .unwrap();
        store
            .save_body(in_progress, BodySide::Response, b"partial response")
            .unwrap();

        let manager = MitmManager::new(runtime);
        manager
            .create_filter_script(ScriptRequest {
                name: "request-order".into(),
                content: "local body = entry.req.body:as_json(); \
                          return body ~= nil and body.order_id == ..."
                    .into(),
            })
            .await
            .unwrap();
        let body_filter = SessionFilter {
            option: Some(FilterOption::Script {
                script_name: "request-order".into(),
            }),
            input: "42".into(),
        };
        assert_eq!(
            manager
                .log_ids(LogIdsRequest {
                    session_id: Some(session.id),
                    filter: Some(body_filter),
                    ids: Some(vec![completed, in_progress]),
                    min_id: None,
                    max_id: None,
                    limit: None,
                })
                .await
                .unwrap()
                .matched_ids,
            vec![completed]
        );
        assert!(
            manager
                .debug_filter_script(
                    "request-order".into(),
                    DebugFilterScriptRequest {
                        session_id: Some(session.id),
                        log_id: completed,
                        input: "42".into(),
                    },
                )
                .await
                .unwrap()
        );

        manager
            .create_column_script(ScriptRequest {
                name: "response-body".into(),
                content: "return entry.resp.body:as_string() or 'unavailable'".into(),
            })
            .await
            .unwrap();
        let view = vec![Column::Script {
            width: 100.0,
            script_name: "response-body".into(),
        }];
        let payload = manager
            .log_views(LogViewsRequest {
                session_id: Some(session.id),
                logs: vec![
                    LogViewItem {
                        id: completed,
                        updated_at: None,
                    },
                    LogViewItem {
                        id: in_progress,
                        updated_at: None,
                    },
                ],
                view: Some(SessionViewInput {
                    columns: view.clone(),
                }),
            })
            .await
            .unwrap();
        assert_eq!(
            payload
                .rows
                .iter()
                .find(|row| row.id == completed)
                .unwrap()
                .cells,
            vec!["completed response"]
        );
        assert_eq!(
            payload
                .rows
                .iter()
                .find(|row| row.id == in_progress)
                .unwrap()
                .cells,
            vec!["unavailable"]
        );
        assert_eq!(
            manager
                .log_ids(LogIdsRequest {
                    session_id: Some(session.id),
                    filter: Some(SessionFilter {
                        option: Some(FilterOption::Column {
                            column: FilterColumn::Script {
                                script_name: "response-body".into(),
                            },
                            regex: false,
                        }),
                        input: "completed".into(),
                    }),
                    ids: Some(vec![completed, in_progress]),
                    min_id: None,
                    max_id: None,
                    limit: None,
                })
                .await
                .unwrap()
                .matched_ids,
            vec![completed]
        );
    }

    #[tokio::test]
    async fn structured_filters_are_read_only_and_script_errors_are_non_matches() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let session = runtime.create_session(NewSession::default()).unwrap();
        let store = CaptureStore::open(session.id, runtime.workspace().root()).unwrap();
        let first = store
            .begin("127.0.0.1", &request("First"), "request")
            .unwrap();
        let second = store
            .begin("127.0.0.1", &request("second"), "request")
            .unwrap();
        let manager = MitmManager::new(runtime);
        let original = manager.session_view(Some(session.id)).await.unwrap().filter;

        let regex_filter = SessionFilter {
            option: Some(FilterOption::Column {
                column: FilterColumn::Uri,
                regex: true,
            }),
            input: r"(?i)^http://example\.com/first$".into(),
        };
        let ids = manager
            .log_ids(LogIdsRequest {
                session_id: Some(session.id),
                filter: Some(regex_filter.clone()),
                ids: None,
                min_id: None,
                max_id: None,
                limit: None,
            })
            .await
            .unwrap();
        assert_eq!(ids.matched_ids, vec![first]);
        assert_eq!(
            manager.session_view(Some(session.id)).await.unwrap().filter,
            original
        );
        assert_eq!(
            manager
                .log_ids(LogIdsRequest {
                    session_id: Some(session.id),
                    filter: None,
                    ids: None,
                    min_id: None,
                    max_id: None,
                    limit: None,
                })
                .await
                .unwrap()
                .matched_ids,
            vec![second, first]
        );

        let exact = SessionFilter {
            option: Some(FilterOption::Column {
                column: FilterColumn::Uri,
                regex: false,
            }),
            input: "EXAMPLE.COM/FIRST".into(),
        };
        assert!(
            manager
                .log_ids(LogIdsRequest {
                    session_id: Some(session.id),
                    filter: Some(exact),
                    ids: None,
                    min_id: None,
                    max_id: None,
                    limit: None,
                })
                .await
                .unwrap()
                .matched_ids
                .is_empty()
        );

        let invalid = manager
            .log_ids(LogIdsRequest {
                session_id: Some(session.id),
                filter: Some(SessionFilter {
                    option: Some(FilterOption::Column {
                        column: FilterColumn::Uri,
                        regex: true,
                    }),
                    input: "(".into(),
                }),
                ids: None,
                min_id: None,
                max_id: None,
                limit: None,
            })
            .await
            .unwrap_err();
        assert_eq!(invalid.code, ErrorCode::BadRequest);

        manager
            .create_column_script(ScriptRequest {
                name: "path".into(),
                content: "return entry.req.uri.path".into(),
            })
            .await
            .unwrap();
        let custom = SessionFilter {
            option: Some(FilterOption::Column {
                column: FilterColumn::Script {
                    script_name: "path".into(),
                },
                regex: true,
            }),
            input: "^/First$".into(),
        };
        assert_eq!(
            manager
                .log_ids(LogIdsRequest {
                    session_id: Some(session.id),
                    filter: Some(custom),
                    ids: None,
                    min_id: None,
                    max_id: None,
                    limit: None,
                })
                .await
                .unwrap()
                .matched_ids,
            vec![first]
        );

        manager
            .create_column_script(ScriptRequest {
                name: "broken-column".into(),
                content: "error('boom')".into(),
            })
            .await
            .unwrap();
        assert!(
            manager
                .log_ids(LogIdsRequest {
                    session_id: Some(session.id),
                    filter: Some(SessionFilter {
                        option: Some(FilterOption::Column {
                            column: FilterColumn::Script {
                                script_name: "broken-column".into(),
                            },
                            regex: false,
                        }),
                        input: "anything".into(),
                    }),
                    ids: None,
                    min_id: None,
                    max_id: None,
                    limit: None,
                })
                .await
                .unwrap()
                .matched_ids
                .is_empty()
        );

        manager
            .create_filter_script(ScriptRequest {
                name: "path-is".into(),
                content: "local input = ...; return entry.req.uri.path == '/' .. input".into(),
            })
            .await
            .unwrap();
        let scripted = SessionFilter {
            option: Some(FilterOption::Script {
                script_name: "path-is".into(),
            }),
            input: "second".into(),
        };
        assert_eq!(
            manager
                .log_ids(LogIdsRequest {
                    session_id: Some(session.id),
                    filter: Some(scripted),
                    ids: None,
                    min_id: None,
                    max_id: None,
                    limit: None,
                })
                .await
                .unwrap()
                .matched_ids,
            vec![second]
        );
        assert!(
            manager
                .debug_filter_script(
                    "path-is".into(),
                    DebugFilterScriptRequest {
                        session_id: Some(session.id),
                        log_id: second,
                        input: "second".into(),
                    },
                )
                .await
                .unwrap()
        );

        manager
            .create_filter_script(ScriptRequest {
                name: "broken-filter".into(),
                content: "error('boom')".into(),
            })
            .await
            .unwrap();
        assert!(
            manager
                .log_ids(LogIdsRequest {
                    session_id: Some(session.id),
                    filter: Some(SessionFilter {
                        option: Some(FilterOption::Script {
                            script_name: "broken-filter".into(),
                        }),
                        input: "anything".into(),
                    }),
                    ids: None,
                    min_id: None,
                    max_id: None,
                    limit: None,
                })
                .await
                .unwrap()
                .matched_ids
                .is_empty()
        );
        assert!(
            manager
                .debug_filter_script(
                    "broken-filter".into(),
                    DebugFilterScriptRequest {
                        session_id: Some(session.id),
                        log_id: first,
                        input: "anything".into(),
                    },
                )
                .await
                .is_err()
        );

        let empty = SessionFilter {
            option: Some(FilterOption::Script {
                script_name: "path-is".into(),
            }),
            input: String::new(),
        };
        let all = manager
            .log_ids(LogIdsRequest {
                session_id: Some(session.id),
                filter: Some(empty.clone()),
                ids: None,
                min_id: None,
                max_id: None,
                limit: None,
            })
            .await
            .unwrap();
        assert_eq!(all.matched_ids, vec![second, first]);
        assert_eq!(
            manager.session_view(Some(session.id)).await.unwrap().filter,
            original
        );

        let spaced = SessionFilter {
            option: Some(FilterOption::Column {
                column: FilterColumn::Uri,
                regex: false,
            }),
            input: " /First ".into(),
        };
        assert!(
            manager
                .log_ids(LogIdsRequest {
                    session_id: Some(session.id),
                    filter: Some(spaced),
                    ids: None,
                    min_id: None,
                    max_id: None,
                    limit: None,
                })
                .await
                .unwrap()
                .matched_ids
                .is_empty()
        );
    }

    #[tokio::test]
    async fn session_views_validate_new_references_and_script_updates_keep_names() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let session = runtime.create_session(NewSession::default()).unwrap();
        let manager = MitmManager::new(runtime);

        let error = manager
            .replace_session_view(
                Some(session.id),
                ReplaceSessionViewRequest {
                    columns: vec![Column::Script {
                        width: 100.0,
                        script_name: "missing".into(),
                    }],
                },
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::NotFound);

        manager
            .create_column_script(ScriptRequest {
                name: "old".into(),
                content: "entry.req().method()".into(),
            })
            .await
            .unwrap();
        manager
            .replace_session_view(
                Some(session.id),
                ReplaceSessionViewRequest {
                    columns: vec![Column::Script {
                        width: 100.0,
                        script_name: "old".into(),
                    }],
                },
            )
            .await
            .unwrap();
        manager
            .update_column_script(
                "old".into(),
                UpdateScriptRequest {
                    content: "-- #display_name: 请求主机\nreturn entry.req().uri().host()".into(),
                },
            )
            .await
            .unwrap();

        let view = manager.session_view(Some(session.id)).await.unwrap();
        let script =
            serde_json::to_value(manager.column_script("old".into()).await.unwrap()).unwrap();
        assert_eq!(script["name"], "old");
        assert_eq!(script["display_name"], "请求主机");
        manager
            .update_column_script(
                "old".into(),
                UpdateScriptRequest {
                    content: "-- #display_name:\nreturn 'ok'".into(),
                },
            )
            .await
            .unwrap();
        let scripts = serde_json::to_value(manager.column_scripts().await.unwrap()).unwrap();
        assert!(scripts[0].get("display_name").is_none());
        assert!(matches!(
            &view.columns[0],
            Column::Script { script_name, .. } if script_name == "old"
        ));
        manager.delete_column_script("old".into()).await.unwrap();
        assert!(
            manager
                .session_view(Some(session.id))
                .await
                .unwrap()
                .columns
                .is_empty()
        );
    }

    #[tokio::test]
    async fn session_interceptors_validate_and_report_global_usage() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let first = runtime.create_session(NewSession::default()).unwrap();
        let second = runtime
            .create_session(NewSession {
                name: Some("second".into()),
                ..Default::default()
            })
            .unwrap();
        let manager = MitmManager::new(runtime);
        manager
            .create_interceptor(InterceptorCreateRequest {
                kind: InterceptorKind::Request,
                name: "header".into(),
                content: "-- #display_name: 请求头处理".into(),
            })
            .await
            .unwrap();

        manager
            .replace_session_interceptors(
                Some(first.id),
                ReplaceSessionInterceptorsRequest {
                    request: vec![SessionInterceptorInput {
                        name: "header".into(),
                        enabled: true,
                    }],
                    response: vec![],
                },
            )
            .await
            .unwrap();

        let first_chain = manager.session_interceptors(Some(first.id)).await.unwrap();
        assert!(first_chain.request[0].valid);
        assert_eq!(
            first_chain.request[0].display_name.as_deref(),
            Some("请求头处理")
        );
        let detail = manager
            .interceptor(InterceptorKind::Request, "header".into())
            .await
            .unwrap();
        assert_eq!(detail.display_name.as_deref(), Some("请求头处理"));
        assert!(
            manager
                .session_interceptors(Some(second.id))
                .await
                .unwrap()
                .request
                .is_empty()
        );
        let library = manager
            .interceptors(InterceptorKind::Request)
            .await
            .unwrap();
        assert_eq!(library.items[0].usage_count, 1);
        assert_eq!(library.items[0].name, "header");
        assert_eq!(library.items[0].display_name.as_deref(), Some("请求头处理"));

        let error = manager
            .replace_session_interceptors(
                Some(first.id),
                ReplaceSessionInterceptorsRequest {
                    request: vec![
                        SessionInterceptorInput {
                            name: "header".into(),
                            enabled: true,
                        },
                        SessionInterceptorInput {
                            name: "header".into(),
                            enabled: false,
                        },
                    ],
                    response: vec![],
                },
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::BadRequest);
    }
}
