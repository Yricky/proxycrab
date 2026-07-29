use std::{path::Path, sync::Arc};

use async_trait::async_trait;
use proxy_crab_mitm::{
    ProxyCrab,
    lua::{evaluate_column, evaluate_filter},
    model::{
        AppConfig, CaptureOutcome, Column, HeaderValues, InterceptorKind, ProxyStatus, Script,
        ScriptKind, SessionMetadata, SystemLogEntry, WorkspacePaths,
    },
};

use crate::dto::{
    CertificateResponse, ColumnInput, ColumnView, CreateSessionRequest, FilterLogsRequest,
    HeaderItem, InterceptorCreateRequest, InterceptorDetail, InterceptorList,
    InterceptorUpdateRequest, LogDetail, LogRow, LogsPayload, LogsQuery, ManagerError,
    ManagerResult, RequestDetail, ResponseDetail, ScriptRequest, SetInterceptorOrderRequest,
    SystemLogsQuery, UpdateScriptRequest, UpdateSessionRequest, VisibleColumn,
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
    async fn create_session(&self, request: CreateSessionRequest)
    -> ManagerResult<SessionMetadata>;
    async fn update_session(
        &self,
        id: u64,
        request: UpdateSessionRequest,
    ) -> ManagerResult<SessionMetadata>;
    async fn delete_session(&self, id: u64) -> ManagerResult<()>;
    async fn activate_session(&self, id: u64) -> ManagerResult<SessionMetadata>;
    async fn logs(&self, query: LogsQuery) -> ManagerResult<LogsPayload>;
    async fn log(&self, session_id: Option<u64>, id: u64) -> ManagerResult<LogDetail>;
    async fn filter_logs(&self, request: FilterLogsRequest) -> ManagerResult<LogsPayload>;
    async fn column_scripts(&self) -> ManagerResult<Vec<Script>>;
    async fn create_column_script(&self, request: ScriptRequest) -> ManagerResult<()>;
    async fn column_script(&self, name: String) -> ManagerResult<Script>;
    async fn update_column_script(
        &self,
        name: String,
        request: UpdateScriptRequest,
    ) -> ManagerResult<()>;
    async fn delete_column_script(&self, name: String) -> ManagerResult<()>;
    async fn columns(&self) -> ManagerResult<Vec<VisibleColumn>>;
    async fn append_column(&self, input: ColumnInput) -> ManagerResult<Vec<VisibleColumn>>;
    async fn replace_column(
        &self,
        index: usize,
        input: ColumnInput,
    ) -> ManagerResult<Vec<VisibleColumn>>;
    async fn delete_column(&self, index: usize) -> ManagerResult<Vec<VisibleColumn>>;
    async fn interceptors(&self, kind: InterceptorKind) -> ManagerResult<InterceptorList>;
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
    async fn set_interceptor_enabled(
        &self,
        kind: InterceptorKind,
        name: String,
        enabled: bool,
    ) -> ManagerResult<()>;
    async fn set_interceptor_order(
        &self,
        request: SetInterceptorOrderRequest,
    ) -> ManagerResult<Vec<String>>;
    async fn filter_history(&self) -> ManagerResult<Vec<String>>;
    async fn add_filter_history(&self, script: String) -> ManagerResult<Vec<String>>;
    async fn remove_filter_history(&self, script: Option<String>) -> ManagerResult<Vec<String>>;
    async fn certificate(&self) -> ManagerResult<CertificateResponse>;
    async fn regenerate_certificate(&self) -> ManagerResult<CertificateResponse>;
    async fn system_logs(&self, query: SystemLogsQuery) -> ManagerResult<Vec<SystemLogEntry>>;
    async fn clear_system_logs(&self) -> ManagerResult<()>;
}

pub struct MitmManager {
    runtime: Arc<ProxyCrab>,
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
            .or_else(|| self.runtime.active_session().map(|session| session.id))
            .ok_or_else(|| ManagerError::conflict("no active session"))
    }

    fn logs_payload(
        &self,
        items: Vec<proxy_crab_mitm::model::CaptureSummary>,
    ) -> ManagerResult<LogsPayload> {
        let columns = self.runtime.columns();
        let mut views = vec![ColumnView {
            key: "id".into(),
            name: "id".into(),
            kind: "id".into(),
            width: None,
            script_name: None,
        }];
        views.extend(columns.iter().map(column_view));
        let rows = items
            .into_iter()
            .map(|item| {
                let mut cells = vec![item.id.to_string()];
                for column in &columns {
                    cells.push(self.render_cell(column, &item));
                }
                LogRow { id: item.id, cells }
            })
            .collect();
        Ok(LogsPayload {
            columns: views,
            rows,
        })
    }

    fn render_cell(
        &self,
        column: &Column,
        item: &proxy_crab_mitm::model::CaptureSummary,
    ) -> String {
        match column {
            Column::Method { .. } => item.request.method.clone(),
            Column::Uri { .. } => item.request.uri.clone(),
            Column::Code { .. } => item
                .response
                .as_ref()
                .map(|response| response.status.to_string())
                .unwrap_or_else(|| "...".into()),
            Column::Source { .. } => item.source.clone(),
            Column::Stage { .. } => item.stage.clone(),
            Column::Script { script_name, .. } => self
                .runtime
                .script(ScriptKind::Column, script_name)
                .and_then(|script| evaluate_column(&script.content, item))
                .unwrap_or_else(|error| error.to_string()),
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
        if let Some(id) = config.active_session_id
            && !self
                .runtime
                .sessions()
                .iter()
                .any(|session| session.id == id)
        {
            return Err(ManagerError::not_found(format!("session {id} not found")));
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
            if error.to_string().contains("active session") {
                ManagerError::new("active_session_delete_forbidden", error.to_string())
            } else if error.to_string().contains("requests in progress") {
                ManagerError::new("session_in_use", error.to_string())
            } else {
                map_error(error)
            }
        })
    }

    async fn activate_session(&self, id: u64) -> ManagerResult<SessionMetadata> {
        self.runtime.activate_session(id).map_err(map_error)
    }

    async fn logs(&self, query: LogsQuery) -> ManagerResult<LogsPayload> {
        let session_id = self.session_id(query.session_id)?;
        let items = self
            .runtime
            .list_captures(
                session_id,
                query.limit.unwrap_or(100).min(1000),
                query.after_id,
            )
            .map_err(map_error)?;
        self.logs_payload(items)
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
                status_text: response.status.to_string(),
                version: response.version,
                headers: flatten_headers(&response.headers),
                body: detail.response_body,
            }),
            req_modifications: detail.request_modifications,
            resp_modifications: detail.response_modifications,
        })
    }

    async fn filter_logs(&self, request: FilterLogsRequest) -> ManagerResult<LogsPayload> {
        let session_id = self.session_id(request.session_id)?;
        let limit = request.limit.unwrap_or(100).min(1000);
        if limit == 0 {
            return self.logs_payload(Vec::new());
        }
        if request.script.trim().is_empty() {
            let items = self
                .runtime
                .list_captures(session_id, limit, None)
                .map_err(map_error)?;
            return self.logs_payload(items);
        }
        let runtime = self.runtime.clone();
        let script = request.script;
        let matched = tokio::task::spawn_blocking(move || {
            let mut matched = Vec::with_capacity(limit);
            let mut before_id = None;
            loop {
                let page = runtime
                    .list_captures_before(session_id, 256, before_id)
                    .map_err(map_error)?;
                if page.is_empty() {
                    break;
                }
                before_id = page.last().map(|item| item.id);
                for item in page {
                    if evaluate_filter(&script, &item)
                        .map_err(|error| ManagerError::bad_request(error.to_string()))?
                    {
                        matched.push(item);
                        if matched.len() == limit {
                            return Ok::<_, ManagerError>(matched);
                        }
                    }
                }
            }
            Ok::<_, ManagerError>(matched)
        })
        .await
        .map_err(|error| ManagerError::internal(format!("filter task failed: {error}")))??;
        self.logs_payload(matched)
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
        let current = self
            .runtime
            .script(ScriptKind::Column, &name)
            .map_err(map_error)?;
        self.runtime
            .update_script(
                ScriptKind::Column,
                &name,
                Script {
                    name: request.name.unwrap_or(current.name),
                    content: request.content.unwrap_or(current.content),
                },
            )
            .map_err(map_error)
    }

    async fn delete_column_script(&self, name: String) -> ManagerResult<()> {
        self.runtime
            .delete_script(ScriptKind::Column, &name)
            .map_err(map_error)
    }

    async fn columns(&self) -> ManagerResult<Vec<VisibleColumn>> {
        Ok(visible_columns(&self.runtime.columns()))
    }

    async fn append_column(&self, input: ColumnInput) -> ManagerResult<Vec<VisibleColumn>> {
        let mut columns = self.runtime.columns();
        columns.push(parse_column(input)?);
        self.runtime.replace_columns(columns).map_err(map_error)?;
        self.columns().await
    }

    async fn replace_column(
        &self,
        index: usize,
        input: ColumnInput,
    ) -> ManagerResult<Vec<VisibleColumn>> {
        let mut columns = self.runtime.columns();
        if index >= columns.len() {
            return Err(ManagerError::not_found("column index not found"));
        }
        columns[index] = parse_column(input)?;
        self.runtime.replace_columns(columns).map_err(map_error)?;
        self.columns().await
    }

    async fn delete_column(&self, index: usize) -> ManagerResult<Vec<VisibleColumn>> {
        let mut columns = self.runtime.columns();
        if index >= columns.len() {
            return Err(ManagerError::not_found("column index not found"));
        }
        columns.remove(index);
        self.runtime.replace_columns(columns).map_err(map_error)?;
        self.columns().await
    }

    async fn interceptors(&self, kind: InterceptorKind) -> ManagerResult<InterceptorList> {
        let items = self.runtime.interceptors(kind).map_err(map_error)?;
        let active_order = items
            .iter()
            .filter_map(|item| item.order.map(|order| (order, item.name.clone())))
            .collect::<Vec<_>>();
        let mut active_order = active_order;
        active_order.sort_by_key(|(order, _)| *order);
        Ok(InterceptorList {
            kind,
            active_order: active_order.into_iter().map(|(_, name)| name).collect(),
            items,
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
        if request.enabled {
            self.runtime
                .set_interceptor_enabled(request.kind, &request.name, true)
                .map_err(map_error)?;
        }
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
        let enabled = match kind {
            InterceptorKind::Request => self.runtime.config().active_request_interceptors,
            InterceptorKind::Response => self.runtime.config().active_response_interceptors,
        }
        .contains(&name);
        Ok(InterceptorDetail {
            kind,
            name: script.name,
            content: script.content,
            enabled,
        })
    }

    async fn update_interceptor(
        &self,
        kind: InterceptorKind,
        name: String,
        request: InterceptorUpdateRequest,
    ) -> ManagerResult<()> {
        let script_kind = interceptor_script_kind(kind);
        let current = self.runtime.script(script_kind, &name).map_err(map_error)?;
        let new_name = request.name.unwrap_or(current.name);
        self.runtime
            .update_script(
                script_kind,
                &name,
                Script {
                    name: new_name.clone(),
                    content: request.content.unwrap_or(current.content),
                },
            )
            .map_err(map_error)?;
        if let Some(enabled) = request.enabled {
            self.runtime
                .set_interceptor_enabled(kind, &new_name, enabled)
                .map_err(map_error)?;
        }
        Ok(())
    }

    async fn delete_interceptor(&self, kind: InterceptorKind, name: String) -> ManagerResult<()> {
        self.runtime
            .delete_script(interceptor_script_kind(kind), &name)
            .map_err(map_error)
    }

    async fn set_interceptor_enabled(
        &self,
        kind: InterceptorKind,
        name: String,
        enabled: bool,
    ) -> ManagerResult<()> {
        self.runtime
            .set_interceptor_enabled(kind, &name, enabled)
            .map_err(map_error)
    }

    async fn set_interceptor_order(
        &self,
        request: SetInterceptorOrderRequest,
    ) -> ManagerResult<Vec<String>> {
        self.runtime
            .set_interceptor_order(request.kind, request.order)
            .map_err(map_error)
    }

    async fn filter_history(&self) -> ManagerResult<Vec<String>> {
        Ok(self.runtime.filter_history())
    }

    async fn add_filter_history(&self, script: String) -> ManagerResult<Vec<String>> {
        self.runtime.add_filter_history(script).map_err(map_error)
    }

    async fn remove_filter_history(&self, script: Option<String>) -> ManagerResult<Vec<String>> {
        self.runtime
            .remove_filter_history(script.as_deref())
            .map_err(map_error)
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
}

fn parse_column(input: ColumnInput) -> ManagerResult<Column> {
    let width = input.width.unwrap_or(match input.kind.as_str() {
        "method" => 80.0,
        "uri" => 300.0,
        "code" => 50.0,
        "source" => 130.0,
        "stage" => 70.0,
        "script" => 100.0,
        _ => 100.0,
    });
    if !width.is_finite() || width <= 0.0 {
        return Err(ManagerError::bad_request("column width must be positive"));
    }
    Ok(match input.kind.as_str() {
        "method" => Column::Method { width },
        "uri" => Column::Uri { width },
        "code" => Column::Code { width },
        "source" => Column::Source { width },
        "stage" => Column::Stage { width },
        "script" => Column::Script {
            width,
            script_name: input
                .script_name
                .ok_or_else(|| ManagerError::bad_request("script_name is required"))?,
        },
        _ => return Err(ManagerError::bad_request("unknown column kind")),
    })
}

fn visible_columns(columns: &[Column]) -> Vec<VisibleColumn> {
    columns
        .iter()
        .enumerate()
        .map(|(index, column)| {
            let view = column_view(column);
            VisibleColumn {
                index,
                key: view.key,
                name: view.name,
                kind: view.kind,
                width: view.width.unwrap_or_default(),
                script_name: view.script_name,
            }
        })
        .collect()
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
        || message.contains("script")
        || message.contains("Lua")
    {
        ManagerError::bad_request(message)
    } else {
        ManagerError::internal(message)
    }
}
