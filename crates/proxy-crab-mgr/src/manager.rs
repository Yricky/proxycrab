use std::{collections::HashMap, path::Path, sync::Arc};

use async_trait::async_trait;
use proxy_crab_mitm::{
    ProxyCrab,
    lua::{evaluate_column_named, evaluate_filter_named},
    model::{
        AppConfig, CaptureOutcome, CaptureSummary, Column, FilterColumn, FilterOption,
        HeaderValues, InterceptorKind, ProxyStatus, Script, ScriptKind, SessionFilter,
        SessionInterceptor, SessionInterceptors, SessionMetadata, SessionView, SystemLogEntry,
        WorkspacePaths,
    },
};

use crate::dto::{
    ActiveSession, BypassPage, BypassQuery, CertificateResponse, ColumnView, CreateSessionRequest,
    DebugFilterScriptRequest, DeleteCount, HeaderItem, InterceptorCreateRequest, InterceptorDetail,
    InterceptorLibraryList, InterceptorUpdateRequest, LogDetail, LogIdsPayload, LogIdsRequest,
    LogViewException, LogViewRow, LogViewsPayload, LogViewsRequest, ManagerError, ManagerResult,
    ReplaceSessionInterceptorsRequest, ReplaceSessionViewRequest, RequestDetail, ResponseDetail,
    RoutingSelection, ScriptRequest, SessionInterceptorItem, SessionInterceptorsPayload,
    SessionViewPayload, SystemLogsQuery, UpdateScriptRequest, UpdateSessionRequest,
};

#[async_trait]
pub trait ProxyCrabManager: Send + Sync {
    async fn workspace(&self) -> ManagerResult<WorkspacePaths>;
    async fn set_workspace_for_next_start(&self, path: String) -> ManagerResult<WorkspacePaths>;
    async fn config(&self) -> ManagerResult<AppConfig>;
    async fn replace_config(&self, config: AppConfig) -> ManagerResult<AppConfig>;
    async fn proxy_status(&self) -> ManagerResult<ProxyStatus>;
    async fn start_proxy(&self) -> ManagerResult<ProxyStatus>;
    async fn stop_proxy(&self) -> ManagerResult<ProxyStatus>;
    async fn sessions(&self) -> ManagerResult<Vec<SessionMetadata>>;
    async fn active_session(&self) -> ManagerResult<ActiveSession>;
    async fn replace_active_session(&self, active: ActiveSession) -> ManagerResult<ActiveSession>;
    async fn create_session(&self, request: CreateSessionRequest)
    -> ManagerResult<SessionMetadata>;
    async fn update_session(
        &self,
        id: u64,
        request: UpdateSessionRequest,
    ) -> ManagerResult<SessionMetadata>;
    async fn delete_session(&self, id: u64) -> ManagerResult<()>;
    async fn log_ids(&self, request: LogIdsRequest) -> ManagerResult<LogIdsPayload>;
    async fn log_views(&self, request: LogViewsRequest) -> ManagerResult<LogViewsPayload>;
    async fn log(&self, session_id: Option<u64>, id: u64) -> ManagerResult<LogDetail>;
    async fn session_view(&self, session_id: Option<u64>) -> ManagerResult<SessionViewPayload>;
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
    async fn regenerate_certificate(&self) -> ManagerResult<CertificateResponse>;
    async fn system_logs(&self, query: SystemLogsQuery) -> ManagerResult<Vec<SystemLogEntry>>;
    async fn clear_system_logs(&self) -> ManagerResult<()>;
    async fn bypass_entries(&self, query: BypassQuery) -> ManagerResult<BypassPage>;
    async fn delete_bypass_entry(&self, id: u64) -> ManagerResult<()>;
    async fn delete_bypass_entries(&self, ids: Vec<u64>) -> ManagerResult<DeleteCount>;
    async fn clear_bypass_entries(&self) -> ManagerResult<DeleteCount>;
}

pub struct MitmManager {
    runtime: Arc<ProxyCrab>,
}

enum PreparedFilter {
    All,
    Column {
        column: FilterColumn,
        input: String,
        case_sensitive: bool,
        script: Option<PreparedScript>,
    },
    Script {
        script: PreparedScript,
        input: String,
    },
}

struct PreparedScript {
    name: String,
    source: String,
}

impl MitmManager {
    pub fn new(runtime: Arc<ProxyCrab>) -> Arc<Self> {
        Arc::new(Self { runtime })
    }

    pub fn runtime(&self) -> &Arc<ProxyCrab> {
        &self.runtime
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
            Column::Script { .. } => None,
        }
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
            Some(FilterOption::Column {
                column,
                case_sensitive,
            }) => {
                let script = match column {
                    FilterColumn::Script { script_name } => {
                        let script = runtime
                            .script(ScriptKind::Column, script_name)
                            .map_err(map_error)?;
                        Some(PreparedScript {
                            name: script.name,
                            source: script.content,
                        })
                    }
                    _ => None,
                };
                Ok(PreparedFilter::Column {
                    column: column.clone(),
                    input: filter.input.clone(),
                    case_sensitive: *case_sensitive,
                    script,
                })
            }
            Some(FilterOption::Script { script_name }) => {
                let script = runtime
                    .script(ScriptKind::Filter, script_name)
                    .map_err(map_error)?;
                Ok(PreparedFilter::Script {
                    script: PreparedScript {
                        name: script.name,
                        source: script.content,
                    },
                    input: filter.input.clone(),
                })
            }
        }
    }

    fn matches_filter(filter: &PreparedFilter, item: &CaptureSummary) -> bool {
        match filter {
            PreparedFilter::All => true,
            PreparedFilter::Script { script, input } => {
                evaluate_filter_named(&script.source, input, item, &script.name).unwrap_or(false)
            }
            PreparedFilter::Column {
                column,
                input,
                case_sensitive,
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
                        let Ok(value) = evaluate_column_named(&script.source, item, &script.name)
                        else {
                            return false;
                        };
                        value
                    }
                };
                if *case_sensitive {
                    value.contains(input)
                } else {
                    value.to_lowercase().contains(&input.to_lowercase())
                }
            }
        }
    }
}

#[async_trait]
impl ProxyCrabManager for MitmManager {
    async fn workspace(&self) -> ManagerResult<WorkspacePaths> {
        Ok(self.runtime.workspace_paths())
    }

    async fn set_workspace_for_next_start(&self, path: String) -> ManagerResult<WorkspacePaths> {
        self.runtime
            .set_workspace_for_next_start(Path::new(&path))
            .map_err(map_error)
    }

    async fn config(&self) -> ManagerResult<AppConfig> {
        Ok(self.runtime.config())
    }

    async fn replace_config(&self, config: AppConfig) -> ManagerResult<AppConfig> {
        if config.api_host != "127.0.0.1" && config.api_host != "::1" {
            return Err(ManagerError::bad_request(
                "management API host must be a loopback address",
            ));
        }
        self.runtime.replace_config(config).map_err(map_error)
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

    async fn sessions(&self) -> ManagerResult<Vec<SessionMetadata>> {
        Ok(self.runtime.sessions())
    }

    async fn active_session(&self) -> ManagerResult<ActiveSession> {
        Ok(ActiveSession {
            session_id: self.runtime.active_session_id(),
        })
    }

    async fn replace_active_session(&self, active: ActiveSession) -> ManagerResult<ActiveSession> {
        self.runtime
            .replace_active_session(active.session_id)
            .map_err(map_error)?;
        Ok(active)
    }

    async fn create_session(
        &self,
        request: CreateSessionRequest,
    ) -> ManagerResult<SessionMetadata> {
        self.runtime
            .create_session(request.name, request.description)
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

    async fn delete_session(&self, id: u64) -> ManagerResult<()> {
        self.runtime.delete_session(id).map_err(|error| {
            if error.to_string().contains("proxy is running") {
                ManagerError::new("proxy_running", error.to_string())
            } else if error.to_string().contains("requests in progress") {
                ManagerError::new("session_in_use", error.to_string())
            } else {
                map_error(error)
            }
        })
    }

    async fn log_ids(&self, request: LogIdsRequest) -> ManagerResult<LogIdsPayload> {
        const DEFAULT_LIMIT: usize = 10_000;
        const SCAN_PAGE_SIZE: usize = 512;

        let session_id = self.session_id(request.session_id)?;
        let limit = request.limit.unwrap_or(DEFAULT_LIMIT).min(DEFAULT_LIMIT);
        let ascending = request.min_id.is_some() && request.max_id.is_none();
        let requested_filter = request.filter;
        let runtime = self.runtime.clone();
        let filter = match &requested_filter {
            Some(filter) => filter.clone(),
            None => runtime.session_view(session_id).map_err(map_error)?.filter,
        };
        let prepared = Self::prepare_filter(&runtime, &filter)?;
        let min_id = request.min_id;
        let max_id = request.max_id;
        let ids = tokio::task::spawn_blocking(move || {
            if limit == 0 {
                return Ok(Vec::new());
            }
            if matches!(prepared, PreparedFilter::All) {
                return runtime
                    .list_captures_range(session_id, limit, min_id, max_id, ascending)
                    .map(|items| items.into_iter().map(|item| item.id).collect())
                    .map_err(map_error);
            }

            let mut ids = Vec::with_capacity(limit);
            let mut min_id = min_id;
            let mut max_id = max_id;
            loop {
                let page = runtime
                    .list_captures_range(session_id, SCAN_PAGE_SIZE, min_id, max_id, ascending)
                    .map_err(map_error)?;
                if page.is_empty() {
                    break;
                }
                let page_len = page.len();
                let cursor = page.last().map(|item| item.id);
                for item in page {
                    if Self::matches_filter(&prepared, &item) {
                        ids.push(item.id);
                        if ids.len() == limit {
                            return Ok(ids);
                        }
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
            Ok(ids)
        })
        .await
        .map_err(|error| ManagerError::internal(format!("log ID task failed: {error}")))??;
        let effective_filter = if let Some(filter) = requested_filter {
            let mut view = self.runtime.session_view(session_id).map_err(map_error)?;
            view.filter = filter;
            self.runtime
                .replace_session_view(session_id, view)
                .map_err(map_error)?
                .filter
        } else {
            filter
        };
        Ok(LogIdsPayload {
            ids,
            filter: effective_filter,
        })
    }

    async fn log_views(&self, request: LogViewsRequest) -> ManagerResult<LogViewsPayload> {
        const MAX_LOGS: usize = 200;

        if request.logs.len() > MAX_LOGS {
            return Err(ManagerError::bad_request(format!(
                "at most {MAX_LOGS} logs may be requested"
            )));
        }
        let session_id = self.session_id(request.session_id)?;
        let columns = match request.view {
            Some(view) => view.columns,
            None => {
                self.runtime
                    .session_view(session_id)
                    .map_err(map_error)?
                    .columns
            }
        };
        if columns
            .iter()
            .any(|column| !column.width().is_finite() || column.width() <= 0.0)
        {
            return Err(ManagerError::bad_request("column width must be positive"));
        }
        let column_views = columns.iter().map(column_view).collect::<Vec<_>>();
        let mut requested = HashMap::new();
        for item in request.logs {
            requested.insert(item.id, item.updated_at);
        }
        let ids = requested.keys().copied().collect::<Vec<_>>();
        let summaries = self.runtime.captures(session_id, &ids).map_err(map_error)?;
        let summaries = summaries
            .into_iter()
            .map(|item| (item.id, item))
            .collect::<HashMap<_, _>>();
        let mut scripts = HashMap::<String, Result<String, String>>::new();
        for column in &columns {
            if let Column::Script { script_name, .. } = column {
                scripts.entry(script_name.clone()).or_insert_with(|| {
                    self.runtime
                        .script(ScriptKind::Column, script_name)
                        .map(|script| script.content)
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
            if client_updated_at.is_some_and(|updated_at| item.updated_at <= updated_at) {
                continue;
            }

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
                    Ok(content) => evaluate_column_named(content, item, script_name)
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
            rows.push(LogViewRow {
                id,
                updated_at: item.updated_at,
                cells,
            });
        }
        Ok(LogViewsPayload {
            columns: column_views,
            rows,
            exceptions,
        })
    }

    async fn log(&self, session_id: Option<u64>, id: u64) -> ManagerResult<LogDetail> {
        let session_id = self.session_id(session_id)?;
        let runtime = self.runtime.clone();
        let detail = tokio::task::spawn_blocking(move || runtime.capture(session_id, id))
            .await
            .map_err(|error| ManagerError::internal(format!("log read task failed: {error}")))?
            .map_err(map_error)?
            .ok_or_else(|| ManagerError::not_found(format!("log {id} not found")))?;
        Ok(LogDetail {
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
        })
    }

    async fn session_view(&self, session_id: Option<u64>) -> ManagerResult<SessionViewPayload> {
        let session_id = self.session_id(session_id)?;
        let view = self.runtime.session_view(session_id).map_err(map_error)?;
        Ok(SessionViewPayload {
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
        let current = self.runtime.session_view(session_id).map_err(map_error)?;
        let view = self
            .runtime
            .replace_session_view(
                session_id,
                SessionView {
                    columns: request.columns,
                    filter: current.filter,
                },
            )
            .map_err(map_error)?;
        Ok(SessionViewPayload {
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
        let script = self
            .runtime
            .script(ScriptKind::Filter, &name)
            .map_err(map_error)?;
        let detail = self
            .runtime
            .capture(session_id, request.log_id)
            .map_err(map_error)?
            .ok_or_else(|| ManagerError::not_found(format!("log {} not found", request.log_id)))?;
        evaluate_filter_named(
            &script.content,
            &request.input,
            &detail.summary,
            &script.name,
        )
        .map_err(|error| ManagerError::bad_request(error.to_string()))
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
            .update_script(ScriptKind::Routing, &name, request.content)
            .map_err(map_error)
    }

    async fn delete_routing_script(&self, name: String) -> ManagerResult<()> {
        self.runtime
            .delete_script(ScriptKind::Routing, &name)
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
        if let Some(name) = &selection.name {
            self.runtime
                .script(ScriptKind::Routing, name)
                .map_err(map_error)?;
        }
        let name = selection.name;
        self.runtime
            .workspace()
            .update_config(|config| config.routing_script_name = name.clone())
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
                    name: item.name,
                    enabled: item.enabled,
                    valid: item.valid,
                })
                .collect(),
            response: value
                .response
                .into_iter()
                .map(|item| SessionInterceptorItem {
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
        let mut rows = self
            .runtime
            .bypass_entries(limit + 1, query.before_id)
            .map_err(map_error)?;
        let has_more = rows.len() > limit;
        rows.truncate(limit);
        Ok(BypassPage { rows, has_more })
    }

    async fn delete_bypass_entry(&self, id: u64) -> ManagerResult<()> {
        self.runtime.delete_bypass_entry(id).map_err(map_error)
    }

    async fn delete_bypass_entries(&self, ids: Vec<u64>) -> ManagerResult<DeleteCount> {
        self.runtime
            .delete_bypass_entries(&ids)
            .map(|deleted| DeleteCount { deleted })
            .map_err(map_error)
    }

    async fn clear_bypass_entries(&self) -> ManagerResult<DeleteCount> {
        self.runtime
            .clear_bypass_entries()
            .map(|deleted| DeleteCount { deleted })
            .map_err(map_error)
    }
}

fn column_view(column: &Column) -> ColumnView {
    let kind = match column {
        Column::Method { .. } => "method",
        Column::Uri { .. } => "uri",
        Column::Code { .. } => "code",
        Column::Source { .. } => "source",
        Column::Stage { .. } => "stage",
        Column::Script { .. } => "script",
    };
    let script_name = match column {
        Column::Script { script_name, .. } => Some(script_name.clone()),
        _ => None,
    };
    ColumnView {
        key: script_name
            .as_ref()
            .map(|name| format!("script:{name}"))
            .unwrap_or_else(|| kind.into()),
        name: script_name.clone().unwrap_or_else(|| kind.into()),
        kind: kind.into(),
        width: Some(column.width()),
        script_name,
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
    let message = error.to_string();
    if message.contains("not found") {
        ManagerError::not_found(message)
    } else if message.contains("already exists")
        || message.contains("already running")
        || message.contains("must be stopped")
    {
        ManagerError::conflict(message)
    } else if message.contains("invalid")
        || message.contains("must ")
        || message.contains("cannot ")
        || message.contains("duplicate")
        || message.contains("script")
        || message.contains("Lua")
    {
        ManagerError::bad_request(message)
    } else {
        ManagerError::internal(message)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use proxy_crab_mitm::{
        ProxyCrab,
        log_buffer::LogBuffer,
        model::{
            Column, FilterColumn, FilterOption, HeaderValues, InterceptorKind, RequestData,
            SessionFilter,
        },
        storage::CaptureStore,
    };
    use tempfile::tempdir;

    use crate::dto::{
        DebugFilterScriptRequest, InterceptorCreateRequest, LogIdsRequest, LogViewItem,
        LogViewsRequest, ReplaceSessionInterceptorsRequest, ReplaceSessionViewRequest,
        ScriptRequest, SessionInterceptorInput, UpdateScriptRequest,
    };

    use super::{MitmManager, ProxyCrabManager, status_text};

    #[test]
    fn response_status_text_uses_the_canonical_reason() {
        assert_eq!(status_text(200), "OK");
        assert_eq!(status_text(404), "Not Found");
        assert_eq!(status_text(999), "");
    }

    fn request(path: &str) -> RequestData {
        RequestData {
            method: "GET".into(),
            uri: format!("http://example.com/{path}"),
            version: "HTTP/1.1".into(),
            headers: HeaderValues::new(),
        }
    }

    #[tokio::test]
    async fn log_ids_apply_exclusive_bounds_and_batch_views_report_cell_errors() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let session = runtime.create_session(None, None).unwrap();
        let store =
            CaptureStore::open(session.id, &runtime.workspace().session_dir(session.id)).unwrap();
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
                min_id: Some(first),
                max_id: Some(third),
                limit: None,
            })
            .await
            .unwrap();
        assert_eq!(ids.ids, vec![second]);

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

        assert_eq!(payload.columns.len(), 2);
        assert_eq!(payload.rows[0].cells, vec!["GET", ""]);
        assert_eq!(payload.exceptions.len(), 2);
        assert!(payload.exceptions.iter().any(|error| {
            error.id == second
                && error.column_index == Some(1)
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
        assert_eq!(error.code, "bad_request");
    }

    #[tokio::test]
    async fn structured_filters_persist_and_script_errors_are_non_matches() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let session = runtime.create_session(None, None).unwrap();
        let store =
            CaptureStore::open(session.id, &runtime.workspace().session_dir(session.id)).unwrap();
        let first = store
            .begin("127.0.0.1", &request("First"), "request")
            .unwrap();
        let second = store
            .begin("127.0.0.1", &request("second"), "request")
            .unwrap();
        let manager = MitmManager::new(runtime);

        let insensitive = SessionFilter {
            option: Some(FilterOption::Column {
                column: FilterColumn::Uri,
                case_sensitive: false,
            }),
            input: "EXAMPLE.COM/FIRST".into(),
        };
        let ids = manager
            .log_ids(LogIdsRequest {
                session_id: Some(session.id),
                filter: Some(insensitive.clone()),
                min_id: None,
                max_id: None,
                limit: None,
            })
            .await
            .unwrap();
        assert_eq!(ids.ids, vec![first]);
        assert_eq!(
            manager.session_view(Some(session.id)).await.unwrap().filter,
            insensitive
        );
        assert_eq!(
            manager
                .log_ids(LogIdsRequest {
                    session_id: Some(session.id),
                    filter: None,
                    min_id: None,
                    max_id: None,
                    limit: None,
                })
                .await
                .unwrap()
                .ids,
            vec![first]
        );

        let exact = SessionFilter {
            option: Some(FilterOption::Column {
                column: FilterColumn::Uri,
                case_sensitive: true,
            }),
            input: "EXAMPLE.COM/FIRST".into(),
        };
        assert!(
            manager
                .log_ids(LogIdsRequest {
                    session_id: Some(session.id),
                    filter: Some(exact),
                    min_id: None,
                    max_id: None,
                    limit: None,
                })
                .await
                .unwrap()
                .ids
                .is_empty()
        );

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
                case_sensitive: true,
            }),
            input: "First".into(),
        };
        assert_eq!(
            manager
                .log_ids(LogIdsRequest {
                    session_id: Some(session.id),
                    filter: Some(custom),
                    min_id: None,
                    max_id: None,
                    limit: None,
                })
                .await
                .unwrap()
                .ids,
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
                            case_sensitive: false,
                        }),
                        input: "anything".into(),
                    }),
                    min_id: None,
                    max_id: None,
                    limit: None,
                })
                .await
                .unwrap()
                .ids
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
                    min_id: None,
                    max_id: None,
                    limit: None,
                })
                .await
                .unwrap()
                .ids,
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
                    min_id: None,
                    max_id: None,
                    limit: None,
                })
                .await
                .unwrap()
                .ids
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
                min_id: None,
                max_id: None,
                limit: None,
            })
            .await
            .unwrap();
        assert_eq!(all.ids, vec![second, first]);
        assert_eq!(
            manager.session_view(Some(session.id)).await.unwrap().filter,
            empty
        );

        let spaced = SessionFilter {
            option: Some(FilterOption::Column {
                column: FilterColumn::Uri,
                case_sensitive: true,
            }),
            input: " /First ".into(),
        };
        assert!(
            manager
                .log_ids(LogIdsRequest {
                    session_id: Some(session.id),
                    filter: Some(spaced),
                    min_id: None,
                    max_id: None,
                    limit: None,
                })
                .await
                .unwrap()
                .ids
                .is_empty()
        );
    }

    #[tokio::test]
    async fn session_views_validate_new_references_and_script_updates_keep_names() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let session = runtime.create_session(None, None).unwrap();
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
        assert_eq!(error.code, "not_found");

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
                    content: "return entry.req().uri().host()".into(),
                },
            )
            .await
            .unwrap();

        let view = manager.session_view(Some(session.id)).await.unwrap();
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
        let first = runtime.create_session(None, None).unwrap();
        let second = runtime.create_session(Some("second".into()), None).unwrap();
        let manager = MitmManager::new(runtime);
        manager
            .create_interceptor(InterceptorCreateRequest {
                kind: InterceptorKind::Request,
                name: "header".into(),
                content: String::new(),
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
        assert_eq!(error.code, "bad_request");
    }
}
