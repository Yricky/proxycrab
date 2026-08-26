use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::Read,
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
};

use crate::{
    asset::{Asset, AssetError, AssetStore, AssetUpload},
    breakpoint::BreakpointRegistry,
    bypass::{BypassEntry, BypassStore},
    ca::CertificateAuthority,
    log_buffer::LogBuffer,
    lua::BodyReplacement,
    model::{
        AppConfig, BodyPayload, BreakpointDetail, BreakpointListFilter, BreakpointSummary,
        CaptureDetail, CaptureSummary, Column, FilterColumn, FilterOption, InterceptorKind,
        InterceptorLibraryItem, MAX_SESSION_INTERCEPTORS_PER_KIND, ProxyStatus,
        ResolvedSessionInterceptor, ResolvedSessionInterceptors, Script, ScriptKind, SessionFilter,
        SessionInterceptors, SessionMetadata, SessionView, SystemLogEntry,
        TemporaryExecutionResult,
    },
    proxy::{ProxyController, UpstreamClient},
    storage::{
        BODY_DETAIL_LIMIT, BodySide, BodySource, BodySourceData, CaptureStore, body_payload,
    },
    workspace::Workspace,
};
use anyhow::{Result, bail};

fn body_replacement_payload(
    replacement: &BodyReplacement,
    headers: &crate::model::HeaderValues,
) -> Result<BodyPayload> {
    match replacement {
        BodyReplacement::String(content) => {
            let size = content.len() as u64;
            if size > BODY_DETAIL_LIMIT {
                return Ok(BodyPayload::Large { size, path: None });
            }
            Ok(body_payload(content.as_bytes(), headers, size, None))
        }
        BodyReplacement::Asset(asset) => {
            let file = File::open(asset.path())?;
            let size = file.metadata()?.len();
            if size > BODY_DETAIL_LIMIT {
                return Ok(BodyPayload::Large { size, path: None });
            }
            let mut bytes = Vec::new();
            file.take(BODY_DETAIL_LIMIT + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > BODY_DETAIL_LIMIT {
                return Ok(BodyPayload::Large {
                    size: bytes.len() as u64,
                    path: None,
                });
            }
            Ok(body_payload(&bytes, headers, bytes.len() as u64, None))
        }
    }
}

pub struct ProxyCrab {
    workspace: Arc<Workspace>,
    assets: AssetStore,
    authority: RwLock<Arc<CertificateAuthority>>,
    bypass: BypassStore,
    stores: Mutex<HashMap<u64, CaptureStore>>,
    session_pins: Mutex<HashMap<u64, usize>>,
    view_updates: Mutex<()>,
    log_buffer: Arc<LogBuffer>,
    breakpoints: Arc<BreakpointRegistry>,
    upstream: RwLock<UpstreamClient>,
    proxy: ProxyController,
}

impl ProxyCrab {
    pub fn open(
        workspace_root: impl Into<PathBuf>,
        log_buffer: Arc<LogBuffer>,
    ) -> Result<Arc<Self>> {
        let workspace_root = workspace_root.into();
        std::fs::create_dir_all(&workspace_root)?;
        let workspace = Workspace::open(workspace_root)?;
        let assets = AssetStore::open(workspace.root())?;
        let authority = Arc::new(CertificateAuthority::load_or_generate(workspace.root())?);
        let bypass = BypassStore::open(workspace.root())?;
        Ok(Arc::new(Self {
            workspace,
            assets,
            authority: RwLock::new(authority),
            bypass,
            stores: Mutex::new(HashMap::new()),
            session_pins: Mutex::new(HashMap::new()),
            view_updates: Mutex::new(()),
            log_buffer,
            breakpoints: Arc::new(BreakpointRegistry::default()),
            upstream: RwLock::new(UpstreamClient::new()),
            proxy: ProxyController::new(),
        }))
    }

    pub fn workspace(&self) -> &Arc<Workspace> {
        &self.workspace
    }

    pub fn asset(&self, id: &str) -> Result<Option<Asset>, AssetError> {
        self.assets.get(id)
    }

    pub(crate) fn asset_store(&self) -> AssetStore {
        self.assets.clone()
    }

    pub async fn begin_asset_upload(
        &self,
        id: &str,
        content_type: String,
    ) -> Result<AssetUpload, AssetError> {
        self.assets.begin_upload(id, content_type).await
    }

    pub fn config(&self) -> AppConfig {
        self.workspace.config()
    }

    pub async fn replace_config(&self, config: AppConfig) -> Result<AppConfig> {
        if let Some(name) = &config.routing_script_name {
            self.workspace.get_script(ScriptKind::Routing, name)?;
        }
        self.proxy
            .mutate_configuration(self, || {
                let current = self.config();
                let changed = current.active_session_id != config.active_session_id
                    || current.routing_script_name != config.routing_script_name;
                Ok((self.workspace.replace_config(config)?, changed))
            })
            .await
    }

    pub fn sessions(&self) -> Vec<SessionMetadata> {
        self.workspace.sessions()
    }

    pub fn archived_sessions(&self) -> Vec<SessionMetadata> {
        self.workspace.archived_sessions()
    }

    pub fn create_session(
        &self,
        name: Option<String>,
        description: Option<String>,
    ) -> Result<SessionMetadata> {
        self.workspace.create_session(name, description)
    }

    pub fn update_session(
        &self,
        id: u64,
        name: Option<String>,
        description: Option<Option<String>>,
    ) -> Result<SessionMetadata> {
        self.workspace.update_session(id, name, description)
    }

    pub async fn archive_session(&self, id: u64) -> Result<SessionMetadata> {
        self.proxy
            .mutate_sessions(|| {
                if self.active_session_id() == Some(id) {
                    bail!("active session {id} cannot be archived");
                }
                let pins = self
                    .session_pins
                    .lock()
                    .expect("session pins lock poisoned");
                if pins.get(&id).copied().unwrap_or_default() != 0 {
                    bail!("session {id} has requests in progress");
                }
                self.stores
                    .lock()
                    .expect("capture stores lock poisoned")
                    .remove(&id);
                let result = self.workspace.archive_session(id);
                drop(pins);
                result
            })
            .await
    }

    pub async fn restore_session(&self, id: u64) -> Result<SessionMetadata> {
        self.proxy
            .mutate_sessions(|| self.workspace.restore_session(id))
            .await
    }

    pub async fn delete_archived_session(&self, id: u64) -> Result<()> {
        self.proxy
            .mutate_sessions(|| self.workspace.delete_archived_session(id))
            .await
    }

    pub fn active_session_id(&self) -> Option<u64> {
        self.workspace.active_session_id()
    }

    pub fn active_session(&self) -> Option<SessionMetadata> {
        let id = self.active_session_id()?;
        self.sessions().into_iter().find(|session| session.id == id)
    }

    pub async fn replace_active_session(&self, session_id: Option<u64>) -> Result<Option<u64>> {
        self.proxy
            .mutate_configuration(self, || {
                let changed = self.active_session_id() != session_id;
                Ok((self.workspace.replace_active_session(session_id)?, changed))
            })
            .await
    }

    pub async fn replace_routing_selection(&self, name: Option<String>) -> Result<Option<String>> {
        if let Some(name) = &name {
            self.script(ScriptKind::Routing, name)?;
        }
        self.proxy
            .mutate_configuration(self, || {
                let changed = self.config().routing_script_name != name;
                self.workspace
                    .update_config(|config| config.routing_script_name = name.clone())?;
                Ok((name, changed))
            })
            .await
    }

    pub async fn update_routing_script(&self, name: &str, content: String) -> Result<()> {
        crate::lua::validate_script(ScriptKind::Routing, &content)?;
        let current = self.script(ScriptKind::Routing, name)?;
        let selected = self.config().routing_script_name.as_deref() == Some(name);
        let changed = selected && current.content != content;
        self.proxy
            .mutate_configuration(self, || {
                self.workspace.save_script(
                    ScriptKind::Routing,
                    Script {
                        name: name.to_string(),
                        content,
                    },
                    true,
                )?;
                Ok(((), changed))
            })
            .await
    }

    pub async fn delete_routing_script(&self, name: &str) -> Result<()> {
        self.script(ScriptKind::Routing, name)?;
        let selected = self.config().routing_script_name.as_deref() == Some(name);
        self.proxy
            .mutate_configuration(self, || {
                self.delete_script(ScriptKind::Routing, name)?;
                Ok(((), selected))
            })
            .await
    }

    pub fn selected_routing_script(&self) -> Result<Option<Script>> {
        let Some(name) = self.config().routing_script_name else {
            return Ok(None);
        };
        match self.script(ScriptKind::Routing, &name) {
            Ok(script) => Ok(Some(script)),
            Err(error) => {
                tracing::warn!("selected routing script {name} is unavailable: {error}");
                self.workspace
                    .update_config(|config| config.routing_script_name = None)?;
                Ok(None)
            }
        }
    }

    fn capture_store_locked(&self, session_id: u64) -> Result<CaptureStore> {
        if let Some(store) = self
            .stores
            .lock()
            .expect("capture stores lock poisoned")
            .get(&session_id)
            .cloned()
        {
            return Ok(store);
        }
        if !self
            .workspace
            .sessions()
            .iter()
            .any(|session| session.id == session_id)
        {
            bail!("session {session_id} not found");
        }
        let store = CaptureStore::open(session_id, &self.workspace.session_dir(session_id))?;
        self.stores
            .lock()
            .expect("capture stores lock poisoned")
            .insert(session_id, store.clone());
        Ok(store)
    }

    pub fn pin_session(self: &Arc<Self>, session_id: u64) -> Result<SessionPin> {
        let mut pins = self
            .session_pins
            .lock()
            .expect("session pins lock poisoned");
        let store = self.capture_store_locked(session_id)?;
        *pins.entry(session_id).or_default() += 1;
        Ok(SessionPin {
            runtime: self.clone(),
            session_id,
            store,
        })
    }

    fn pin_capture_store(&self, session_id: u64) -> Result<CaptureStorePin<'_>> {
        let mut pins = self
            .session_pins
            .lock()
            .expect("session pins lock poisoned");
        let store = self.capture_store_locked(session_id)?;
        *pins.entry(session_id).or_default() += 1;
        Ok(CaptureStorePin {
            runtime: self,
            session_id,
            store,
        })
    }

    fn unpin_session(&self, session_id: u64) {
        let mut pins = self
            .session_pins
            .lock()
            .expect("session pins lock poisoned");
        if let Some(count) = pins.get_mut(&session_id) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                pins.remove(&session_id);
            }
        }
    }

    pub fn list_captures(
        &self,
        session_id: u64,
        limit: usize,
        after_id: Option<u64>,
    ) -> Result<Vec<CaptureSummary>> {
        self.pin_capture_store(session_id)?
            .store
            .list(limit, after_id)
    }

    pub fn capture(&self, session_id: u64, id: u64) -> Result<Option<CaptureDetail>> {
        self.pin_capture_store(session_id)?.store.get(id)
    }

    pub fn has_capture(&self, session_id: u64, id: u64) -> Result<bool> {
        self.pin_capture_store(session_id)?.store.contains(id)
    }

    pub fn capture_body_source(
        &self,
        session_id: u64,
        id: u64,
        side: BodySide,
    ) -> Result<Option<BodySource>> {
        self.pin_capture_store(session_id)?
            .store
            .body_source(id, side)
    }

    pub fn breakpoint_body_source(&self, id: u64, side: BodySide) -> Result<Option<BodySource>> {
        let live = self.breakpoints.detail(id)?;
        let mut source =
            self.capture_body_source(live.summary.session_id, live.summary.capture_id, side)?;
        let live_side = match live.summary.phase {
            InterceptorKind::Request => BodySide::Request,
            InterceptorKind::Response => BodySide::Response,
        };
        if side != live_side {
            return Ok(source);
        }
        let headers = live.context.state.headers();
        if let Some(replacement) = live.context.state.body() {
            source = Some(match replacement {
                crate::lua::BodyReplacement::String(content) => BodySource {
                    stored_size: content.len() as u64,
                    data: BodySourceData::Bytes(content.into_bytes()),
                    path: None,
                    content_type: first_header(&headers, "content-type").map(str::to_owned),
                    content_encodings: Vec::new(),
                },
                crate::lua::BodyReplacement::Asset(asset) => {
                    let metadata = std::fs::metadata(asset.path())?;
                    BodySource {
                        stored_size: metadata.len(),
                        data: BodySourceData::File(asset.path().to_path_buf()),
                        path: Some(asset.path().to_string_lossy().into_owned()),
                        content_type: first_header(&headers, "content-type").map(str::to_owned),
                        content_encodings: Vec::new(),
                    }
                }
            });
        } else if let Some(source) = &mut source {
            source.content_type = first_header(&headers, "content-type").map(str::to_owned);
            source.content_encodings = header_tokens(&headers, "content-encoding");
        }
        Ok(source)
    }

    pub fn breakpoints(&self, filter: &BreakpointListFilter) -> Vec<BreakpointSummary> {
        self.breakpoints.list(filter)
    }

    pub fn breakpoint(&self, id: u64) -> Result<BreakpointDetail> {
        let live = self.breakpoints.detail(id)?;
        let mut capture = self
            .capture(live.summary.session_id, live.summary.capture_id)?
            .ok_or_else(|| anyhow::anyhow!("capture {} not found", live.summary.capture_id))?;
        let headers = live.context.state.headers();
        let tags = live.context.state.tags();
        capture.summary.request.tags = tags;
        match live.summary.phase {
            InterceptorKind::Request => {
                capture.summary.request.method = live
                    .context
                    .state
                    .method()
                    .expect("request breakpoint state always has a method");
                capture.summary.request.uri = live
                    .context
                    .state
                    .uri()
                    .expect("request breakpoint state always has a URI");
                capture.summary.request.headers = headers.clone();
                if let Some(replacement) = live.context.state.body()
                    && let Ok(payload) = body_replacement_payload(&replacement, &headers)
                {
                    capture.request_body = payload;
                }
            }
            InterceptorKind::Response => {
                if let Some(response) = &mut capture.summary.response {
                    response.status = live
                        .context
                        .state
                        .status()
                        .expect("response breakpoint state always has a status");
                    response.headers = headers.clone();
                }
                if let Some(replacement) = live.context.state.body()
                    && let Ok(payload) = body_replacement_payload(&replacement, &headers)
                {
                    capture.response_body = payload;
                }
            }
        }
        Ok(BreakpointDetail {
            breakpoint: live.summary,
            capture,
        })
    }

    pub fn extend_breakpoint(&self, id: u64, timeout_ms: u64) -> Result<BreakpointSummary> {
        self.breakpoints.extend(id, timeout_ms)
    }

    pub fn release_breakpoint(&self, id: u64) -> Result<()> {
        self.breakpoints.release(id)
    }

    pub fn execute_breakpoint_script(
        &self,
        id: u64,
        content: &str,
    ) -> Result<TemporaryExecutionResult> {
        self.breakpoints.execute_temporary(id, content)
    }

    pub(crate) fn breakpoint_registry(&self) -> Arc<BreakpointRegistry> {
        self.breakpoints.clone()
    }

    pub(crate) fn release_all_breakpoints(&self) {
        self.breakpoints.release_all();
    }

    pub fn list_captures_before(
        &self,
        session_id: u64,
        limit: usize,
        before_id: Option<u64>,
    ) -> Result<Vec<CaptureSummary>> {
        self.pin_capture_store(session_id)?
            .store
            .list_before(limit, before_id)
    }

    pub fn list_captures_range(
        &self,
        session_id: u64,
        limit: usize,
        min_id: Option<u64>,
        max_id: Option<u64>,
        ascending: bool,
    ) -> Result<Vec<CaptureSummary>> {
        self.pin_capture_store(session_id)?
            .store
            .list_range(limit, min_id, max_id, ascending)
    }

    pub fn captures(&self, session_id: u64, ids: &[u64]) -> Result<Vec<CaptureSummary>> {
        self.pin_capture_store(session_id)?.store.get_many(ids)
    }

    pub fn scripts(&self, kind: ScriptKind) -> Result<Vec<Script>> {
        self.workspace.list_scripts(kind)
    }

    pub fn script(&self, kind: ScriptKind, name: &str) -> Result<Script> {
        self.workspace.get_script(kind, name)
    }

    pub fn create_script(&self, kind: ScriptKind, script: Script) -> Result<()> {
        crate::lua::validate_script(kind, &script.content)?;
        self.workspace.save_script(kind, script, false)
    }

    pub fn update_script(&self, kind: ScriptKind, name: &str, content: String) -> Result<()> {
        crate::lua::validate_script(kind, &content)?;
        self.workspace.get_script(kind, name)?;
        self.workspace.save_script(
            kind,
            Script {
                name: name.to_string(),
                content,
            },
            true,
        )
    }

    pub fn delete_script(&self, kind: ScriptKind, name: &str) -> Result<()> {
        self.workspace.get_script(kind, name)?;
        if matches!(kind, ScriptKind::Column | ScriptKind::Filter) {
            let previous = self.session_view_snapshots()?;
            self.remove_view_references(kind, name)?;
            if let Err(error) = self.workspace.delete_script(kind, name) {
                if let Err(rollback_error) = self.restore_session_views(&previous) {
                    tracing::error!(
                        "failed to roll back session filters after script delete failed: {rollback_error}"
                    );
                }
                return Err(error);
            }
            return Ok(());
        }
        if kind == ScriptKind::Routing {
            let selected = self.config().routing_script_name;
            if selected.as_deref() == Some(name) {
                self.workspace
                    .update_config(|config| config.routing_script_name = None)?;
            }
            if let Err(error) = self.workspace.delete_script(kind, name) {
                if selected.as_deref() == Some(name) {
                    let _ = self
                        .workspace
                        .update_config(|config| config.routing_script_name = selected.clone());
                }
                return Err(error);
            }
            return Ok(());
        }
        let previous = self.interceptor_reference_snapshots()?;
        self.remove_interceptor_references(kind, name)?;
        if let Err(error) = self.workspace.delete_script(kind, name) {
            if let Err(rollback_error) = self.restore_interceptor_references(&previous) {
                tracing::error!(
                    "failed to roll back session interceptors after script delete failed: {rollback_error}"
                );
            }
            return Err(error);
        }
        Ok(())
    }

    pub fn session_view(&self, session_id: u64) -> Result<SessionView> {
        let _guard = self.view_updates.lock().expect("view update lock poisoned");
        self.session_view_unlocked(session_id)
    }

    fn session_view_unlocked(&self, session_id: u64) -> Result<SessionView> {
        let mut view = self.workspace.session_view(session_id)?;
        if self.validate_session_filter(&view.filter).is_err() {
            view.filter = SessionFilter::default();
            self.workspace
                .replace_session_view(session_id, view.clone())?;
        }
        Ok(view)
    }

    pub fn replace_session_view(&self, session_id: u64, view: SessionView) -> Result<SessionView> {
        let _guard = self.view_updates.lock().expect("view update lock poisoned");
        self.replace_session_view_unlocked(session_id, view)
    }

    fn replace_session_view_unlocked(
        &self,
        session_id: u64,
        mut view: SessionView,
    ) -> Result<SessionView> {
        for column in &view.columns {
            if !column.width().is_finite() || column.width() <= 0.0 {
                bail!("column width must be positive");
            }
            if let Column::Script { script_name, .. } = column {
                self.workspace.get_script(ScriptKind::Column, script_name)?;
            }
        }
        if view.filter.option.is_none() {
            view.filter = SessionFilter::default();
        } else {
            self.validate_session_filter(&view.filter)?;
        }
        self.workspace.replace_session_view(session_id, view)
    }

    pub fn replace_session_filter(
        &self,
        session_id: u64,
        filter: SessionFilter,
    ) -> Result<SessionView> {
        let _guard = self.view_updates.lock().expect("view update lock poisoned");
        let mut view = self.session_view_unlocked(session_id)?;
        view.filter = filter;
        self.replace_session_view_unlocked(session_id, view)
    }

    pub fn replace_session_columns(
        &self,
        session_id: u64,
        columns: Vec<Column>,
    ) -> Result<SessionView> {
        let _guard = self.view_updates.lock().expect("view update lock poisoned");
        let mut view = self.session_view_unlocked(session_id)?;
        view.columns = columns;
        self.replace_session_view_unlocked(session_id, view)
    }

    pub fn session_interceptors(&self, session_id: u64) -> Result<SessionInterceptors> {
        self.workspace.session_interceptors(session_id)
    }

    pub fn resolved_session_interceptors(
        &self,
        session_id: u64,
    ) -> Result<ResolvedSessionInterceptors> {
        let value = self.session_interceptors(session_id)?;
        Ok(ResolvedSessionInterceptors {
            session_id,
            request: self
                .resolve_interceptor_entries(ScriptKind::RequestInterceptor, value.request),
            response: self
                .resolve_interceptor_entries(ScriptKind::ResponseInterceptor, value.response),
        })
    }

    pub fn replace_session_interceptors(
        &self,
        session_id: u64,
        value: SessionInterceptors,
    ) -> Result<SessionInterceptors> {
        validate_session_interceptors(&value)?;
        self.workspace
            .replace_session_interceptors(session_id, value)
    }

    pub fn interceptor_library(
        &self,
        kind: InterceptorKind,
    ) -> Result<Vec<InterceptorLibraryItem>> {
        let script_kind = interceptor_script_kind(kind);
        let mut counts = HashMap::<String, usize>::new();
        for session in self.sessions() {
            let chains = self.session_interceptors(session.id)?;
            let entries = match kind {
                InterceptorKind::Request => chains.request,
                InterceptorKind::Response => chains.response,
            };
            for entry in entries {
                *counts.entry(entry.name).or_default() += 1;
            }
        }
        Ok(self
            .scripts(script_kind)?
            .into_iter()
            .map(|script| InterceptorLibraryItem {
                usage_count: counts.get(&script.name).copied().unwrap_or_default(),
                name: script.name,
            })
            .collect())
    }

    fn resolve_interceptor_entries(
        &self,
        kind: ScriptKind,
        entries: Vec<crate::model::SessionInterceptor>,
    ) -> Vec<ResolvedSessionInterceptor> {
        entries
            .into_iter()
            .map(|entry| ResolvedSessionInterceptor {
                valid: self.script(kind, &entry.name).is_ok(),
                name: entry.name,
                enabled: entry.enabled,
            })
            .collect()
    }

    fn interceptor_reference_snapshots(&self) -> Result<Vec<(u64, SessionInterceptors)>> {
        self.sessions()
            .into_iter()
            .map(|session| {
                self.session_interceptors(session.id)
                    .map(|value| (session.id, value))
            })
            .collect()
    }

    fn restore_interceptor_references(
        &self,
        snapshots: &[(u64, SessionInterceptors)],
    ) -> Result<()> {
        for (session_id, value) in snapshots {
            self.workspace
                .replace_session_interceptors(*session_id, value.clone())?;
        }
        Ok(())
    }

    fn remove_interceptor_references(&self, kind: ScriptKind, name: &str) -> Result<()> {
        let snapshots = self.interceptor_reference_snapshots()?;
        let mut changed = Vec::new();
        for (session_id, previous) in &snapshots {
            let mut next = previous.clone();
            let entries = match kind {
                ScriptKind::RequestInterceptor => &mut next.request,
                ScriptKind::ResponseInterceptor => &mut next.response,
                ScriptKind::Column | ScriptKind::Filter | ScriptKind::Routing => return Ok(()),
            };
            entries.retain(|entry| entry.name != name);
            if next != *previous {
                if let Err(error) = self
                    .workspace
                    .replace_session_interceptors(*session_id, next)
                {
                    for (changed_id, changed_value) in changed {
                        let _ = self
                            .workspace
                            .replace_session_interceptors(changed_id, changed_value);
                    }
                    return Err(error);
                }
                changed.push((*session_id, previous.clone()));
            }
        }
        Ok(())
    }

    fn validate_session_filter(&self, filter: &SessionFilter) -> Result<()> {
        match &filter.option {
            None => Ok(()),
            Some(FilterOption::Column {
                column: FilterColumn::Script { script_name },
                ..
            }) => self
                .workspace
                .get_script(ScriptKind::Column, script_name)
                .map(|_| ()),
            Some(FilterOption::Script { script_name }) => self
                .workspace
                .get_script(ScriptKind::Filter, script_name)
                .map(|_| ()),
            Some(FilterOption::Column { .. }) => Ok(()),
        }
    }

    fn session_view_snapshots(&self) -> Result<Vec<(u64, SessionView)>> {
        self.sessions()
            .into_iter()
            .map(|session| {
                self.workspace
                    .session_view(session.id)
                    .map(|view| (session.id, view))
            })
            .collect()
    }

    fn restore_session_views(&self, snapshots: &[(u64, SessionView)]) -> Result<()> {
        for (session_id, view) in snapshots {
            self.workspace
                .replace_session_view(*session_id, view.clone())?;
        }
        Ok(())
    }

    fn remove_view_references(&self, kind: ScriptKind, name: &str) -> Result<()> {
        let snapshots = self.session_view_snapshots()?;
        let mut changed = Vec::new();
        for (session_id, previous) in &snapshots {
            let mut next = previous.clone();
            replace_filter_reference(&mut next.filter, kind, name);
            if kind == ScriptKind::Column {
                next.columns.retain(|column| {
                    !matches!(
                        column,
                        Column::Script { script_name, .. } if script_name == name
                    )
                });
            }
            if next != *previous {
                if let Err(error) = self.workspace.replace_session_view(*session_id, next) {
                    for (changed_id, changed_view) in changed {
                        let _ = self
                            .workspace
                            .replace_session_view(changed_id, changed_view);
                    }
                    return Err(error);
                }
                changed.push((*session_id, previous.clone()));
            }
        }
        Ok(())
    }

    pub fn certificate_pem(&self) -> String {
        self.authority
            .read()
            .expect("CA lock poisoned")
            .certificate_pem()
            .to_string()
    }

    pub fn authority(&self) -> Arc<CertificateAuthority> {
        self.authority.read().expect("CA lock poisoned").clone()
    }

    pub(crate) fn regenerate_ca_unlocked(&self) -> Result<String> {
        let authority = Arc::new(CertificateAuthority::regenerate(self.workspace.root())?);
        let pem = authority.certificate_pem().to_string();
        *self.authority.write().expect("CA lock poisoned") = authority;
        Ok(pem)
    }

    pub async fn regenerate_ca(&self) -> Result<String> {
        self.proxy.regenerate_ca(self).await
    }

    pub(crate) fn ensure_proxy_stopped(&self) -> Result<()> {
        if !matches!(
            self.proxy_status(),
            ProxyStatus::Stopped | ProxyStatus::Failed { .. }
        ) {
            bail!("proxy must be stopped before regenerating the CA");
        }
        Ok(())
    }

    pub fn log_buffer(&self) -> &Arc<LogBuffer> {
        &self.log_buffer
    }

    pub fn system_logs(&self, after_seq: Option<u64>, limit: usize) -> Vec<SystemLogEntry> {
        self.log_buffer.query(after_seq, limit)
    }

    pub fn clear_system_logs(&self) {
        self.log_buffer.clear();
    }

    pub fn bypass_entries(&self, limit: usize, before_id: Option<u64>) -> Result<Vec<BypassEntry>> {
        self.bypass.list(limit, before_id)
    }

    pub fn delete_bypass_entry(&self, id: u64) -> Result<()> {
        self.bypass.delete(id, self.proxy_started_at())
    }

    pub fn delete_bypass_entries(&self, ids: &[u64]) -> Result<usize> {
        self.bypass.delete_many(ids, self.proxy_started_at())
    }

    pub fn clear_bypass_entries(&self) -> Result<usize> {
        self.bypass.clear_deletable(self.proxy_started_at())
    }

    fn proxy_started_at(&self) -> Option<u64> {
        match self.proxy_status() {
            ProxyStatus::Running { started_at, .. } => Some(started_at),
            _ => None,
        }
    }

    pub(crate) fn bypass_store(&self) -> &BypassStore {
        &self.bypass
    }

    pub(crate) fn upstream_client(&self) -> UpstreamClient {
        self.upstream
            .read()
            .expect("upstream client lock poisoned")
            .clone()
    }

    pub(crate) fn reset_upstream_client(&self) {
        *self
            .upstream
            .write()
            .expect("upstream client lock poisoned") = UpstreamClient::new();
    }

    pub async fn start_proxy(self: &Arc<Self>) -> Result<ProxyStatus> {
        self.proxy.start(self.clone()).await
    }

    pub async fn stop_proxy(&self) -> Result<ProxyStatus> {
        self.breakpoints.release_all();
        self.proxy.stop(self).await
    }

    pub fn proxy_status(&self) -> ProxyStatus {
        self.proxy.status()
    }

    pub fn subscribe_proxy_status_changes(&self) -> tokio::sync::watch::Receiver<u64> {
        self.proxy.subscribe_status_changes()
    }
}

fn first_header<'a>(headers: &'a crate::model::HeaderValues, name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .and_then(|(_, values)| values.first())
        .map(String::as_str)
}

fn header_tokens(headers: &crate::model::HeaderValues, name: &str) -> Vec<String> {
    headers
        .iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case(name))
        .flat_map(|(_, values)| values)
        .flat_map(|value| value.split(','))
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty() && value != "identity")
        .collect()
}

pub struct SessionPin {
    runtime: Arc<ProxyCrab>,
    session_id: u64,
    store: CaptureStore,
}

impl SessionPin {
    pub fn runtime(&self) -> Arc<ProxyCrab> {
        self.runtime.clone()
    }

    pub fn session_id(&self) -> u64 {
        self.session_id
    }

    pub fn store(&self) -> &CaptureStore {
        &self.store
    }
}

impl Drop for SessionPin {
    fn drop(&mut self) {
        self.runtime.unpin_session(self.session_id);
    }
}

struct CaptureStorePin<'a> {
    runtime: &'a ProxyCrab,
    session_id: u64,
    store: CaptureStore,
}

impl Drop for CaptureStorePin<'_> {
    fn drop(&mut self) {
        self.runtime.unpin_session(self.session_id);
    }
}

fn interceptor_script_kind(kind: InterceptorKind) -> ScriptKind {
    match kind {
        InterceptorKind::Request => ScriptKind::RequestInterceptor,
        InterceptorKind::Response => ScriptKind::ResponseInterceptor,
    }
}

fn replace_filter_reference(filter: &mut SessionFilter, kind: ScriptKind, name: &str) {
    let referenced_name = match (&mut filter.option, kind) {
        (
            Some(FilterOption::Column {
                column: FilterColumn::Script { script_name },
                ..
            }),
            ScriptKind::Column,
        ) => Some(script_name),
        (Some(FilterOption::Script { script_name }), ScriptKind::Filter) => Some(script_name),
        _ => None,
    };
    let Some(script_name) = referenced_name else {
        return;
    };
    if script_name != name {
        return;
    }
    *filter = SessionFilter::default();
}

fn validate_session_interceptors(value: &SessionInterceptors) -> Result<()> {
    for (label, entries) in [("request", &value.request), ("response", &value.response)] {
        if entries.len() > MAX_SESSION_INTERCEPTORS_PER_KIND {
            bail!("{label} interceptor chain cannot contain more than 12 entries");
        }
        let mut names = HashSet::new();
        if entries
            .iter()
            .any(|entry| !names.insert(entry.name.as_str()))
        {
            bail!("{label} interceptor chain contains duplicate scripts");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::tempdir;

    use crate::{
        log_buffer::LogBuffer,
        model::{
            Column, FilterColumn, FilterOption, MAX_SESSION_INTERCEPTORS_PER_KIND, Script,
            ScriptKind, SessionFilter, SessionInterceptor, SessionInterceptors, SessionView,
        },
    };

    use super::ProxyCrab;

    #[tokio::test]
    async fn config_replacement_validates_active_session() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::new(32))).unwrap();
        let session = runtime.create_session(Some("one".into()), None).unwrap();
        let mut config = runtime.config();

        config.active_session_id = Some(u64::MAX);
        assert!(runtime.replace_config(config.clone()).await.is_err());
        assert_eq!(runtime.active_session_id(), Some(session.id));

        config.active_session_id = None;
        runtime.replace_config(config).await.unwrap();
        assert_eq!(runtime.active_session_id(), None);
    }

    #[tokio::test]
    async fn capture_store_pin_releases_registry_lock_during_storage_work() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::new(32))).unwrap();
        let session = runtime.create_session(None, None).unwrap();
        runtime.replace_active_session(None).await.unwrap();

        let pin = runtime.pin_capture_store(session.id).unwrap();

        assert!(runtime.session_pins.try_lock().is_ok());
        assert!(
            runtime
                .archive_session(session.id)
                .await
                .unwrap_err()
                .to_string()
                .contains("requests in progress")
        );
        drop(pin);
        runtime.archive_session(session.id).await.unwrap();
    }

    #[test]
    fn session_interceptor_chains_enforce_limit_and_uniqueness() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::new(32))).unwrap();
        let session = runtime.create_session(Some("one".into()), None).unwrap();
        let duplicate = SessionInterceptors {
            request: vec![
                SessionInterceptor {
                    name: "a".into(),
                    enabled: true,
                },
                SessionInterceptor {
                    name: "a".into(),
                    enabled: false,
                },
            ],
            response: vec![],
        };
        assert!(
            runtime
                .replace_session_interceptors(session.id, duplicate)
                .is_err()
        );

        let too_many = SessionInterceptors {
            request: (0..=MAX_SESSION_INTERCEPTORS_PER_KIND)
                .map(|index| SessionInterceptor {
                    name: format!("script-{index}"),
                    enabled: true,
                })
                .collect(),
            response: vec![],
        };
        assert!(
            runtime
                .replace_session_interceptors(session.id, too_many)
                .is_err()
        );
    }

    #[test]
    fn interceptor_delete_updates_every_session() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::new(32))).unwrap();
        let first = runtime.create_session(Some("one".into()), None).unwrap();
        let second = runtime.create_session(Some("two".into()), None).unwrap();
        runtime
            .create_script(
                ScriptKind::RequestInterceptor,
                Script {
                    name: "old".into(),
                    content: String::new(),
                },
            )
            .unwrap();
        for id in [first.id, second.id] {
            runtime
                .replace_session_interceptors(
                    id,
                    SessionInterceptors {
                        request: vec![SessionInterceptor {
                            name: "old".into(),
                            enabled: true,
                        }],
                        response: vec![],
                    },
                )
                .unwrap();
        }

        runtime
            .delete_script(ScriptKind::RequestInterceptor, "old")
            .unwrap();
        assert!(
            runtime
                .session_interceptors(first.id)
                .unwrap()
                .request
                .is_empty()
        );
        assert!(
            runtime
                .session_interceptors(second.id)
                .unwrap()
                .request
                .is_empty()
        );
    }

    #[test]
    fn column_script_delete_removes_view_references() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::new(32))).unwrap();
        let session = runtime.create_session(Some("one".into()), None).unwrap();
        runtime
            .create_script(
                ScriptKind::Column,
                Script {
                    name: "old".into(),
                    content: "return entry.req.uri.host".into(),
                },
            )
            .unwrap();
        runtime
            .replace_session_view(
                session.id,
                SessionView {
                    columns: vec![Column::Script {
                        width: 120.0,
                        script_name: "old".into(),
                    }],
                    filter: SessionFilter {
                        option: Some(FilterOption::Column {
                            column: FilterColumn::Script {
                                script_name: "old".into(),
                            },
                            regex: false,
                        }),
                        input: "example".into(),
                    },
                },
            )
            .unwrap();

        runtime.delete_script(ScriptKind::Column, "old").unwrap();
        let deleted = runtime.session_view(session.id).unwrap();
        assert!(deleted.columns.is_empty());
        assert_eq!(deleted.filter, SessionFilter::default());
    }

    #[test]
    fn filter_script_delete_and_external_removal_repair_sessions() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::new(32))).unwrap();
        let session = runtime.create_session(Some("one".into()), None).unwrap();
        runtime
            .create_script(
                ScriptKind::Filter,
                Script {
                    name: "old".into(),
                    content: "return true".into(),
                },
            )
            .unwrap();
        runtime
            .replace_session_view(
                session.id,
                SessionView {
                    filter: SessionFilter {
                        option: Some(FilterOption::Script {
                            script_name: "old".into(),
                        }),
                        input: "input".into(),
                    },
                    ..SessionView::default()
                },
            )
            .unwrap();

        runtime.delete_script(ScriptKind::Filter, "old").unwrap();
        assert_eq!(
            runtime.session_view(session.id).unwrap().filter,
            SessionFilter::default()
        );

        runtime
            .create_script(
                ScriptKind::Filter,
                Script {
                    name: "external".into(),
                    content: "return true".into(),
                },
            )
            .unwrap();
        runtime
            .replace_session_view(
                session.id,
                SessionView {
                    filter: SessionFilter {
                        option: Some(FilterOption::Script {
                            script_name: "external".into(),
                        }),
                        input: "input".into(),
                    },
                    ..SessionView::default()
                },
            )
            .unwrap();
        runtime
            .workspace
            .delete_script(ScriptKind::Filter, "external")
            .unwrap();

        assert_eq!(
            runtime.session_view(session.id).unwrap().filter,
            SessionFilter::default()
        );
        assert_eq!(
            runtime.workspace.session_view(session.id).unwrap().filter,
            SessionFilter::default()
        );
    }
}
