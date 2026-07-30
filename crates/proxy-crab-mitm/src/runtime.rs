use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
};

use anyhow::{Result, bail};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::{
    ca::CertificateAuthority,
    log_buffer::LogBuffer,
    model::{
        AppConfig, CaptureDetail, CaptureSummary, Column, InterceptorKind, InterceptorLibraryItem,
        MAX_SESSION_INTERCEPTORS_PER_KIND, ProxyStatus, ResolvedSessionInterceptor,
        ResolvedSessionInterceptors, Script, ScriptKind, SessionInterceptors, SessionMetadata,
        SessionView, SystemLogEntry, WorkspacePaths,
    },
    proxy::ProxyController,
    storage::CaptureStore,
    workspace::{
        Workspace, configure_workspace_for_next_start, configured_workspace, resolve_workspace,
    },
};

pub struct ProxyCrab {
    app_data_dir: PathBuf,
    workspace_paths: RwLock<WorkspacePaths>,
    workspace: Arc<Workspace>,
    authority: RwLock<Arc<CertificateAuthority>>,
    stores: Mutex<HashMap<u64, CaptureStore>>,
    session_pins: Mutex<HashMap<u64, usize>>,
    capture_slots: Arc<Semaphore>,
    log_buffer: Arc<LogBuffer>,
    proxy: ProxyController,
}

impl ProxyCrab {
    pub fn open(app_data_dir: impl Into<PathBuf>, log_buffer: Arc<LogBuffer>) -> Result<Arc<Self>> {
        let app_data_dir = app_data_dir.into();
        let workspace_paths = resolve_workspace(&app_data_dir)?;
        let workspace = Workspace::open(PathBuf::from(&workspace_paths.current_path))?;
        let authority = Arc::new(CertificateAuthority::load_or_generate(workspace.root())?);
        Ok(Arc::new(Self {
            app_data_dir,
            workspace_paths: RwLock::new(workspace_paths),
            workspace,
            authority: RwLock::new(authority),
            stores: Mutex::new(HashMap::new()),
            session_pins: Mutex::new(HashMap::new()),
            capture_slots: Arc::new(Semaphore::new(4)),
            log_buffer,
            proxy: ProxyController::new(),
        }))
    }

    pub fn workspace(&self) -> &Arc<Workspace> {
        &self.workspace
    }

    pub fn workspace_paths(&self) -> WorkspacePaths {
        self.workspace_paths
            .read()
            .expect("workspace paths lock poisoned")
            .clone()
    }

    pub fn set_workspace_for_next_start(&self, path: &Path) -> Result<WorkspacePaths> {
        configure_workspace_for_next_start(&self.app_data_dir, path)?;
        let configured_path = configured_workspace(&self.app_data_dir)?;
        let mut paths = self
            .workspace_paths
            .write()
            .expect("workspace paths lock poisoned");
        paths.configured_path = configured_path.to_string_lossy().into_owned();
        Ok(paths.clone())
    }

    pub fn config(&self) -> AppConfig {
        self.workspace.config()
    }

    pub fn replace_config(&self, config: AppConfig) -> Result<AppConfig> {
        self.workspace.update_config(|current| *current = config)
    }

    pub fn sessions(&self) -> Vec<SessionMetadata> {
        self.workspace.sessions()
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

    pub fn delete_session(&self, id: u64) -> Result<()> {
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
        let result = self.workspace.delete_session(id);
        drop(pins);
        result
    }

    pub fn activate_session(&self, id: u64) -> Result<SessionMetadata> {
        self.workspace.activate_session(id)
    }

    pub fn active_session(&self) -> Option<SessionMetadata> {
        self.workspace.active_session()
    }

    pub fn ensure_active_session(&self) -> Result<SessionMetadata> {
        self.workspace.ensure_active_session()
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

    pub fn pin_active_session(self: &Arc<Self>) -> Result<SessionPin> {
        let mut pins = self
            .session_pins
            .lock()
            .expect("session pins lock poisoned");
        let session = self.ensure_active_session()?;
        let store = self.capture_store_locked(session.id)?;
        *pins.entry(session.id).or_default() += 1;
        Ok(SessionPin {
            runtime: self.clone(),
            session_id: session.id,
            store,
        })
    }

    pub async fn acquire_capture_slot(&self) -> OwnedSemaphorePermit {
        self.capture_slots
            .clone()
            .acquire_owned()
            .await
            .expect("capture semaphore is never closed")
    }

    pub fn list_captures(
        &self,
        session_id: u64,
        limit: usize,
        after_id: Option<u64>,
    ) -> Result<Vec<CaptureSummary>> {
        let _pins = self
            .session_pins
            .lock()
            .expect("session pins lock poisoned");
        self.capture_store_locked(session_id)?.list(limit, after_id)
    }

    pub fn capture(&self, session_id: u64, id: u64) -> Result<Option<CaptureDetail>> {
        let _pins = self
            .session_pins
            .lock()
            .expect("session pins lock poisoned");
        self.capture_store_locked(session_id)?.get(id)
    }

    pub fn list_captures_before(
        &self,
        session_id: u64,
        limit: usize,
        before_id: Option<u64>,
    ) -> Result<Vec<CaptureSummary>> {
        let _pins = self
            .session_pins
            .lock()
            .expect("session pins lock poisoned");
        self.capture_store_locked(session_id)?
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
        let _pins = self
            .session_pins
            .lock()
            .expect("session pins lock poisoned");
        self.capture_store_locked(session_id)?
            .list_range(limit, min_id, max_id, ascending)
    }

    pub fn captures(&self, session_id: u64, ids: &[u64]) -> Result<Vec<CaptureSummary>> {
        let _pins = self
            .session_pins
            .lock()
            .expect("session pins lock poisoned");
        self.capture_store_locked(session_id)?.get_many(ids)
    }

    pub fn mark_in_progress_as_shutdown(&self) {
        let _pins = self
            .session_pins
            .lock()
            .expect("session pins lock poisoned");
        for session in self.sessions() {
            if let Ok(store) = self.capture_store_locked(session.id) {
                let _ = store.mark_in_progress_as_shutdown();
            }
        }
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

    pub fn update_script(&self, kind: ScriptKind, old_name: &str, script: Script) -> Result<()> {
        crate::lua::validate_script(kind, &script.content)?;
        if old_name != script.name {
            self.workspace.get_script(kind, old_name)?;
            self.workspace.save_script(kind, script.clone(), false)?;
            let update_result = match kind {
                ScriptKind::Column => self.rename_column_references(old_name, &script.name),
                ScriptKind::RequestInterceptor | ScriptKind::ResponseInterceptor => {
                    self.replace_interceptor_references(kind, old_name, Some(script.name.as_str()))
                }
            };
            if let Err(error) = update_result {
                let _ = self.workspace.delete_script(kind, &script.name);
                return Err(error);
            }
            if let Err(error) = self.workspace.delete_script(kind, old_name) {
                tracing::warn!(
                    "script {old_name} was replaced but the old file could not be removed: {error}"
                );
            }
            return Ok(());
        }
        self.workspace.save_script(kind, script, true)
    }

    pub fn delete_script(&self, kind: ScriptKind, name: &str) -> Result<()> {
        self.workspace.get_script(kind, name)?;
        if kind == ScriptKind::Column {
            return self.workspace.delete_script(kind, name);
        }
        let previous = self.interceptor_reference_snapshots()?;
        self.replace_interceptor_references(kind, name, None)?;
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
        self.workspace.session_view(session_id)
    }

    pub fn replace_session_view(&self, session_id: u64, view: SessionView) -> Result<SessionView> {
        for column in &view.columns {
            if !column.width().is_finite() || column.width() <= 0.0 {
                bail!("column width must be positive");
            }
            if let Column::Script { script_name, .. } = column {
                self.workspace.get_script(ScriptKind::Column, script_name)?;
            }
        }
        self.workspace.replace_session_view(session_id, view)
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

    fn replace_interceptor_references(
        &self,
        kind: ScriptKind,
        old_name: &str,
        new_name: Option<&str>,
    ) -> Result<()> {
        let snapshots = self.interceptor_reference_snapshots()?;
        let mut changed = Vec::new();
        for (session_id, previous) in &snapshots {
            let mut next = previous.clone();
            let entries = match kind {
                ScriptKind::RequestInterceptor => &mut next.request,
                ScriptKind::ResponseInterceptor => &mut next.response,
                ScriptKind::Column => return Ok(()),
            };
            match new_name {
                Some(new_name) => {
                    for entry in entries {
                        if entry.name == old_name {
                            entry.name = new_name.to_string();
                        }
                    }
                }
                None => entries.retain(|entry| entry.name != old_name),
            }
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

    fn rename_column_references(&self, old_name: &str, new_name: &str) -> Result<()> {
        let mut changed = Vec::new();
        for session in self.sessions() {
            let previous = self.workspace.session_view(session.id)?;
            let mut next = previous.clone();
            for column in &mut next.columns {
                if let Column::Script { script_name, .. } = column
                    && script_name == old_name
                {
                    *script_name = new_name.to_string();
                }
            }
            if next != previous {
                if let Err(error) = self.workspace.replace_session_view(session.id, next) {
                    for (id, view) in changed {
                        let _ = self.workspace.replace_session_view(id, view);
                    }
                    return Err(error);
                }
                changed.push((session.id, previous));
            }
        }
        Ok(())
    }

    pub fn filter_history(&self) -> Vec<String> {
        self.config().filter_history
    }

    pub fn add_filter_history(&self, source: String) -> Result<Vec<String>> {
        self.workspace.update_config(|config| {
            config.filter_history.retain(|item| item != &source);
            if !source.trim().is_empty() {
                config.filter_history.insert(0, source);
                config.filter_history.truncate(20);
            }
        })?;
        Ok(self.filter_history())
    }

    pub fn remove_filter_history(&self, source: Option<&str>) -> Result<Vec<String>> {
        self.workspace.update_config(|config| match source {
            Some(source) => config.filter_history.retain(|item| item != source),
            None => config.filter_history.clear(),
        })?;
        Ok(self.filter_history())
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

    pub async fn start_proxy(self: &Arc<Self>) -> Result<ProxyStatus> {
        self.proxy.start(self.clone()).await
    }

    pub async fn stop_proxy(&self) -> Result<ProxyStatus> {
        self.proxy.stop(self).await
    }

    pub fn proxy_status(&self) -> ProxyStatus {
        self.proxy.status()
    }
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
        let mut pins = self
            .runtime
            .session_pins
            .lock()
            .expect("session pins lock poisoned");
        if let Some(count) = pins.get_mut(&self.session_id) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                pins.remove(&self.session_id);
            }
        }
    }
}

fn interceptor_script_kind(kind: InterceptorKind) -> ScriptKind {
    match kind {
        InterceptorKind::Request => ScriptKind::RequestInterceptor,
        InterceptorKind::Response => ScriptKind::ResponseInterceptor,
    }
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
            MAX_SESSION_INTERCEPTORS_PER_KIND, Script, ScriptKind, SessionInterceptor,
            SessionInterceptors,
        },
    };

    use super::ProxyCrab;

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
    fn interceptor_rename_and_delete_update_every_session() {
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
            .update_script(
                ScriptKind::RequestInterceptor,
                "old",
                Script {
                    name: "new".into(),
                    content: String::new(),
                },
            )
            .unwrap();

        assert_eq!(
            runtime.session_interceptors(first.id).unwrap().request[0].name,
            "new"
        );
        runtime
            .delete_script(ScriptKind::RequestInterceptor, "new")
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
}
