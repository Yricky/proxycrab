use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow, bail};
use fs2::FileExt;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::model::{
    AppConfig, Script, ScriptKind, SessionInterceptors, SessionMetadata, SessionView,
    WorkspacePaths,
};

const POINTER_FILE: &str = "config.json";
const WORKSPACE_CONFIG_FILE: &str = "app_config.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkspacePointer {
    workspace_path: PathBuf,
}

pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn resolve_workspace(app_data_dir: &Path) -> Result<WorkspacePaths> {
    fs::create_dir_all(app_data_dir)?;
    let default_path = app_data_dir.join("workspace");
    fs::create_dir_all(&default_path)?;
    let pointer_path = app_data_dir.join(POINTER_FILE);
    let configured = read_json::<WorkspacePointer>(&pointer_path)
        .ok()
        .map(|pointer| pointer.workspace_path)
        .unwrap_or_else(|| default_path.clone());

    let current = if valid_existing_workspace(&configured) {
        configured.clone()
    } else {
        write_json_atomic(
            &pointer_path,
            &WorkspacePointer {
                workspace_path: default_path.clone(),
            },
        )?;
        default_path.clone()
    };

    if !pointer_path.exists() {
        write_json_atomic(
            &pointer_path,
            &WorkspacePointer {
                workspace_path: current.clone(),
            },
        )?;
    }

    Ok(WorkspacePaths {
        current_path: current.to_string_lossy().into_owned(),
        configured_path: current.to_string_lossy().into_owned(),
    })
}

pub fn configure_workspace_for_next_start(app_data_dir: &Path, path: &Path) -> Result<()> {
    if !path.is_absolute() {
        bail!("workspace path must be absolute");
    }
    fs::create_dir_all(app_data_dir)?;
    write_json_atomic(
        &app_data_dir.join(POINTER_FILE),
        &WorkspacePointer {
            workspace_path: path.to_path_buf(),
        },
    )
}

pub fn configured_workspace(app_data_dir: &Path) -> Result<PathBuf> {
    Ok(read_json::<WorkspacePointer>(&app_data_dir.join(POINTER_FILE))?.workspace_path)
}

fn valid_existing_workspace(path: &Path) -> bool {
    if !path.is_absolute() || !path.is_dir() {
        return false;
    }
    let probe = path.join(".proxycrab-write-probe");
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .and_then(|_| fs::remove_file(probe))
        .is_ok()
}

pub struct Workspace {
    root: PathBuf,
    _lock: File,
    config: RwLock<AppConfig>,
    sessions: RwLock<Vec<SessionMetadata>>,
}

impl Workspace {
    pub fn open(root: impl Into<PathBuf>) -> Result<Arc<Self>> {
        let root = root.into();
        fs::create_dir_all(root.join("sessions"))?;
        for directory in ["column", "filter", "routing", "request", "response"] {
            fs::create_dir_all(root.join("scripts").join(directory))?;
        }
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join(".proxycrab.lock"))?;
        lock.try_lock_exclusive()
            .context("workspace is already in use by another ProxyCrab process")?;
        crate::migration::migrate_workspace(&root)?;

        let config_path = root.join(WORKSPACE_CONFIG_FILE);
        let mut config = read_json::<AppConfig>(&config_path).unwrap_or_default();
        let mut sessions = load_sessions(&root)?;
        sessions.sort_by_key(|session| session.created_at);
        if config.routing_script_name.as_ref().is_some_and(|name| {
            !root
                .join("scripts")
                .join("routing")
                .join(format!("{name}.lua"))
                .exists()
        }) {
            config.routing_script_name = None;
        }
        if config
            .active_session_id
            .is_some_and(|id| !sessions.iter().any(|session| session.id == id))
        {
            config.active_session_id = None;
        }
        write_json_atomic(&config_path, &config)?;

        Ok(Arc::new(Self {
            root,
            _lock: lock,
            config: RwLock::new(config),
            sessions: RwLock::new(sessions),
        }))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn config(&self) -> AppConfig {
        self.config
            .read()
            .expect("workspace config lock poisoned")
            .clone()
    }

    pub fn update_config(&self, update: impl FnOnce(&mut AppConfig)) -> Result<AppConfig> {
        let mut config = self
            .config
            .write()
            .map_err(|_| anyhow!("workspace config lock poisoned"))?;
        let mut next = config.clone();
        update(&mut next);
        write_json_atomic(&self.root.join(WORKSPACE_CONFIG_FILE), &next)?;
        *config = next.clone();
        Ok(next)
    }

    pub fn sessions(&self) -> Vec<SessionMetadata> {
        self.sessions
            .read()
            .expect("workspace sessions lock poisoned")
            .clone()
    }

    pub fn active_session_id(&self) -> Option<u64> {
        self.config().active_session_id
    }

    pub fn replace_active_session(&self, session_id: Option<u64>) -> Result<Option<u64>> {
        let sessions = self
            .sessions
            .read()
            .map_err(|_| anyhow!("workspace sessions lock poisoned"))?;
        if let Some(id) = session_id
            && !sessions.iter().any(|session| session.id == id)
        {
            bail!("session {id} not found");
        }
        self.update_config(|config| config.active_session_id = session_id)?;
        Ok(session_id)
    }

    pub fn replace_config(&self, next: AppConfig) -> Result<AppConfig> {
        let sessions = self
            .sessions
            .read()
            .map_err(|_| anyhow!("workspace sessions lock poisoned"))?;
        if let Some(id) = next.active_session_id
            && !sessions.iter().any(|session| session.id == id)
        {
            bail!("session {id} not found");
        }
        self.update_config(|config| *config = next)
    }

    pub fn create_session(
        &self,
        name: Option<String>,
        description: Option<String>,
    ) -> Result<SessionMetadata> {
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| anyhow!("workspace sessions lock poisoned"))?;
        let first = sessions.is_empty();
        let session = self.create_session_locked(&sessions, name, description)?;
        if first
            && let Err(error) =
                self.update_config(|config| config.active_session_id = Some(session.id))
        {
            let _ = fs::remove_dir_all(self.session_dir(session.id));
            return Err(error);
        }
        sessions.push(session.clone());
        Ok(session)
    }

    fn create_session_locked(
        &self,
        sessions: &[SessionMetadata],
        name: Option<String>,
        description: Option<String>,
    ) -> Result<SessionMetadata> {
        let initial_view = SessionView::default();
        let mut id = now_millis();
        while sessions.iter().any(|session| session.id == id) {
            id += 1;
        }
        let session = SessionMetadata {
            id,
            name: name.unwrap_or_else(|| format!("Session-{id}")),
            created_at: now_millis(),
            description,
        };
        let directory = self.session_dir(id);
        let result = (|| {
            fs::create_dir_all(directory.join("blob"))?;
            write_json_atomic(&directory.join("metadata.json"), &session)?;
            write_json_atomic(&directory.join("view.json"), &initial_view)?;
            write_json_atomic(
                &directory.join("interceptors.json"),
                &SessionInterceptors::default(),
            )
        })();
        if let Err(error) = result {
            let _ = fs::remove_dir_all(&directory);
            return Err(error);
        }
        Ok(session)
    }

    pub fn update_session(
        &self,
        id: u64,
        name: Option<String>,
        description: Option<Option<String>>,
    ) -> Result<SessionMetadata> {
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| anyhow!("workspace sessions lock poisoned"))?;
        let index = sessions
            .iter()
            .position(|session| session.id == id)
            .ok_or_else(|| anyhow!("session {id} not found"))?;
        let mut next = sessions[index].clone();
        if let Some(name) = name {
            next.name = name;
        }
        if let Some(description) = description {
            next.description = description;
        }
        write_json_atomic(&self.session_dir(id).join("metadata.json"), &next)?;
        sessions[index] = next.clone();
        Ok(next)
    }

    pub fn delete_session(&self, id: u64) -> Result<()> {
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| anyhow!("workspace sessions lock poisoned"))?;
        let index = sessions
            .iter()
            .position(|session| session.id == id)
            .ok_or_else(|| anyhow!("session {id} not found"))?;
        let directory = self.session_dir(id);
        let deleted = self.root.join("sessions").join(format!(".deleted-{id}"));
        fs::rename(&directory, &deleted)?;
        if self.active_session_id() == Some(id)
            && let Err(error) = self.update_config(|config| config.active_session_id = None)
        {
            let _ = fs::rename(&deleted, &directory);
            return Err(error);
        }
        sessions.remove(index);
        if let Err(error) = fs::remove_dir_all(&deleted) {
            tracing::warn!("failed to remove deleted session directory {id}: {error}");
        }
        Ok(())
    }

    pub fn session_dir(&self, id: u64) -> PathBuf {
        self.root.join("sessions").join(id.to_string())
    }

    pub fn session_view(&self, id: u64) -> Result<SessionView> {
        self.require_session(id)?;
        let path = self.session_dir(id).join("view.json");
        match read_json(&path) {
            Ok(view) => Ok(view),
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                Ok(SessionView::default())
            }
            Err(error) => Err(error),
        }
    }

    pub fn replace_session_view(&self, id: u64, view: SessionView) -> Result<SessionView> {
        self.require_session(id)?;
        write_json_atomic(&self.session_dir(id).join("view.json"), &view)?;
        Ok(view)
    }

    pub fn session_interceptors(&self, id: u64) -> Result<SessionInterceptors> {
        self.require_session(id)?;
        let path = self.session_dir(id).join("interceptors.json");
        match read_json(&path) {
            Ok(value) => Ok(value),
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                Ok(SessionInterceptors::default())
            }
            Err(error) => Err(error),
        }
    }

    pub fn replace_session_interceptors(
        &self,
        id: u64,
        value: SessionInterceptors,
    ) -> Result<SessionInterceptors> {
        self.require_session(id)?;
        write_json_atomic(&self.session_dir(id).join("interceptors.json"), &value)?;
        Ok(value)
    }

    fn require_session(&self, id: u64) -> Result<()> {
        if self.sessions().iter().any(|session| session.id == id) {
            Ok(())
        } else {
            bail!("session {id} not found")
        }
    }

    pub fn list_scripts(&self, kind: ScriptKind) -> Result<Vec<Script>> {
        let mut scripts = Vec::new();
        for entry in fs::read_dir(self.script_dir(kind))? {
            let entry = entry?;
            if entry.path().extension().and_then(|value| value.to_str()) != Some("lua") {
                continue;
            }
            let name = entry
                .path()
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_string();
            scripts.push(Script {
                name,
                content: fs::read_to_string(entry.path())?,
            });
        }
        scripts.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(scripts)
    }

    pub fn get_script(&self, kind: ScriptKind, name: &str) -> Result<Script> {
        validate_script_name(name)?;
        let path = self.script_dir(kind).join(format!("{name}.lua"));
        Ok(Script {
            name: name.to_string(),
            content: fs::read_to_string(path)
                .with_context(|| format!("script {name} not found"))?,
        })
    }

    pub fn save_script(&self, kind: ScriptKind, script: Script, overwrite: bool) -> Result<()> {
        validate_script_name(&script.name)?;
        let path = self.script_dir(kind).join(format!("{}.lua", script.name));
        if !overwrite && path.exists() {
            bail!("script {} already exists", script.name);
        }
        write_text_atomic(&path, &script.content)
    }

    pub fn delete_script(&self, kind: ScriptKind, name: &str) -> Result<()> {
        validate_script_name(name)?;
        fs::remove_file(self.script_dir(kind).join(format!("{name}.lua")))
            .with_context(|| format!("script {name} not found"))
    }

    fn script_dir(&self, kind: ScriptKind) -> PathBuf {
        let directory = match kind {
            ScriptKind::Column => "column",
            ScriptKind::Filter => "filter",
            ScriptKind::Routing => "routing",
            ScriptKind::RequestInterceptor => "request",
            ScriptKind::ResponseInterceptor => "response",
        };
        self.root.join("scripts").join(directory)
    }
}

fn validate_script_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 128
        || !name
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '-' | '_' | '.'))
        || name == "."
        || name == ".."
    {
        bail!("invalid script name");
    }
    Ok(())
}

fn load_sessions(root: &Path) -> Result<Vec<SessionMetadata>> {
    let mut sessions = Vec::new();
    for entry in fs::read_dir(root.join("sessions"))? {
        let entry = entry?;
        if entry.file_type()?.is_dir()
            && let Ok(session) = read_json(entry.path().join("metadata.json").as_path())
        {
            sessions.push(session);
        }
    }
    Ok(sessions)
}

pub(crate) fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

pub(crate) fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<()> {
    let content = serde_json::to_vec_pretty(value)?;
    write_bytes_atomic(path, &content)
}

fn write_text_atomic(path: &Path, content: &str) -> Result<()> {
    write_bytes_atomic(path, content.as_bytes())
}

fn write_bytes_atomic(path: &Path, content: &[u8]) -> Result<()> {
    let temporary = path.with_extension("tmp");
    let mut file = File::create(&temporary)?;
    file.write_all(content)?;
    file.sync_all()?;
    fs::rename(temporary, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use crate::model::{
        Column, FilterColumn, FilterOption, SessionFilter, SessionInterceptor, SessionInterceptors,
        SessionView,
    };

    use super::{Workspace, configure_workspace_for_next_start, resolve_workspace};

    #[test]
    fn invalid_pointer_falls_back_to_default() {
        let app_data = tempdir().unwrap();
        let missing = app_data.path().join("missing");
        configure_workspace_for_next_start(app_data.path(), &missing).unwrap();

        let paths = resolve_workspace(app_data.path()).unwrap();

        assert_eq!(
            paths.current_path,
            app_data.path().join("workspace").to_string_lossy()
        );
        assert_eq!(paths.current_path, paths.configured_path);
    }

    #[test]
    fn first_session_becomes_active_and_deletion_clears_active() {
        let root = tempdir().unwrap();
        let workspace = Workspace::open(root.path()).unwrap();
        let session = workspace
            .create_session(Some("capture".into()), None)
            .unwrap();

        assert_eq!(workspace.active_session_id(), Some(session.id));
        workspace.delete_session(session.id).unwrap();
        assert!(workspace.sessions().is_empty());
        assert_eq!(workspace.active_session_id(), None);
    }

    #[test]
    fn workspace_is_exclusively_locked() {
        let root = tempdir().unwrap();
        let _first = Workspace::open(root.path()).unwrap();
        assert!(Workspace::open(root.path()).is_err());
    }

    #[test]
    fn new_session_uses_default_view() {
        let root = tempdir().unwrap();
        let workspace = Workspace::open(root.path()).unwrap();
        let first = workspace
            .create_session(Some("first".into()), None)
            .unwrap();
        workspace
            .replace_session_view(
                first.id,
                SessionView {
                    columns: vec![Column::Stage { width: 91.0 }],
                    filter: SessionFilter {
                        option: Some(FilterOption::Column {
                            column: FilterColumn::Stage,
                            case_sensitive: true,
                        }),
                        input: "response".into(),
                    },
                },
            )
            .unwrap();
        let second = workspace
            .create_session(Some("second".into()), None)
            .unwrap();

        assert_eq!(
            workspace.session_view(second.id).unwrap(),
            SessionView::default()
        );
    }

    #[test]
    fn later_session_does_not_replace_active_and_active_can_be_switched_or_cleared() {
        let root = tempdir().unwrap();
        let workspace = Workspace::open(root.path()).unwrap();
        let first = workspace
            .create_session(Some("first".into()), None)
            .unwrap();
        let second = workspace
            .create_session(Some("second".into()), None)
            .unwrap();

        assert_eq!(workspace.active_session_id(), Some(first.id));
        assert_eq!(
            workspace.replace_active_session(Some(second.id)).unwrap(),
            Some(second.id)
        );
        assert_eq!(workspace.active_session_id(), Some(second.id));
        assert_eq!(workspace.replace_active_session(None).unwrap(), None);
        assert_eq!(workspace.active_session_id(), None);
        assert!(workspace.replace_active_session(Some(u64::MAX)).is_err());

        drop(workspace);
        let reopened = Workspace::open(root.path()).unwrap();
        assert_eq!(reopened.active_session_id(), None);
    }

    #[test]
    fn missing_active_session_is_cleared_when_workspace_reopens() {
        let root = tempdir().unwrap();
        let workspace = Workspace::open(root.path()).unwrap();
        workspace
            .update_config(|config| config.active_session_id = Some(u64::MAX))
            .unwrap();
        drop(workspace);

        let reopened = Workspace::open(root.path()).unwrap();

        assert_eq!(reopened.active_session_id(), None);
    }

    #[test]
    fn session_filter_round_trip() {
        let root = tempdir().unwrap();
        let workspace = Workspace::open(root.path()).unwrap();
        let session = workspace.create_session(Some("one".into()), None).unwrap();
        let view = SessionView {
            columns: vec![Column::Method { width: 72.0 }],
            filter: SessionFilter {
                option: Some(FilterOption::Script {
                    script_name: "host".into(),
                }),
                input: " example.com ".into(),
            },
        };

        workspace
            .replace_session_view(session.id, view.clone())
            .unwrap();

        assert_eq!(workspace.session_view(session.id).unwrap(), view);
    }

    #[test]
    fn missing_session_view_uses_fixed_defaults_without_migration() {
        let root = tempdir().unwrap();
        let workspace = Workspace::open(root.path()).unwrap();
        let session = workspace.create_session(None, None).unwrap();
        std::fs::remove_file(workspace.session_dir(session.id).join("view.json")).unwrap();

        assert_eq!(
            workspace.session_view(session.id).unwrap(),
            SessionView::default()
        );
        assert!(!workspace.session_dir(session.id).join("view.json").exists());
    }

    #[test]
    fn new_session_has_empty_interceptor_chains() {
        let root = tempdir().unwrap();
        let workspace = Workspace::open(root.path()).unwrap();
        let session = workspace.create_session(Some("one".into()), None).unwrap();

        assert_eq!(
            workspace.session_interceptors(session.id).unwrap(),
            SessionInterceptors::default()
        );
    }

    #[test]
    fn session_interceptors_round_trip_without_resolving_script_files() {
        let root = tempdir().unwrap();
        let workspace = Workspace::open(root.path()).unwrap();
        let session = workspace.create_session(Some("one".into()), None).unwrap();
        let value = SessionInterceptors {
            request: vec![
                SessionInterceptor {
                    name: "present".into(),
                    enabled: true,
                },
                SessionInterceptor {
                    name: "manually-removed".into(),
                    enabled: false,
                },
            ],
            response: vec![],
        };

        workspace
            .replace_session_interceptors(session.id, value.clone())
            .unwrap();

        assert_eq!(workspace.session_interceptors(session.id).unwrap(), value);
    }
}
