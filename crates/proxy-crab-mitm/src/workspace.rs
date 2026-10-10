use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow};
use fs2::FileExt;
use serde::{Serialize, de::DeserializeOwned};

use crate::error::Error;
use crate::model::{
    AppConfig, NewSession, Script, ScriptKind, SessionInterceptors, SessionMetadata, SessionView,
};

const WORKSPACE_CONFIG_FILE: &str = "app_config.json";
pub(crate) const SESSIONS_DIRECTORY: &str = "sessions";
const ARCHIVED_SESSIONS_DIRECTORY: &str = "sessions_archived";

pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub struct Workspace {
    root: PathBuf,
    _lock: File,
    config: RwLock<AppConfig>,
    sessions: RwLock<Vec<SessionMetadata>>,
    archived_sessions: RwLock<Vec<SessionMetadata>>,
}

impl Workspace {
    /// Accept an empty directory (ignoring dot-prefixed entries) or a labeled workspace.
    /// Unlike `open`, this never initializes, repairs, or migrates the selected directory.
    pub fn validate_selection(root: &Path) -> Result<()> {
        let mut empty = true;
        for entry in fs::read_dir(root)? {
            if !entry?.file_name().as_encoded_bytes().starts_with(b".") {
                empty = false;
                break;
            }
        }
        if !empty {
            crate::migration::validate_workspace_schema(root).context(
                "所选目录不是有效工作区：workspace_schema.json 缺失、损坏或版本不受支持",
            )?;
            read_json::<AppConfig>(&root.join(WORKSPACE_CONFIG_FILE))
                .context("所选目录不是有效工作区：app_config.json 缺失或损坏")?;
        }
        Ok(())
    }

    pub fn open(root: impl Into<PathBuf>) -> Result<Arc<Self>> {
        let root = root.into();
        fs::create_dir_all(root.join(SESSIONS_DIRECTORY))?;
        fs::create_dir_all(root.join(ARCHIVED_SESSIONS_DIRECTORY))?;
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
        let mut sessions = load_sessions(&root.join(SESSIONS_DIRECTORY))?;
        sessions.sort_by_key(|session| session.created_at);
        let mut archived_sessions = load_sessions(&root.join(ARCHIVED_SESSIONS_DIRECTORY))?;
        archived_sessions.sort_by_key(|session| session.created_at);
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
            archived_sessions: RwLock::new(archived_sessions),
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

    /// Mutates the in-memory config without persisting it. Used by the
    /// headless CLI to apply command-line overrides for the current run only.
    pub fn override_config_in_memory(&self, update: impl FnOnce(&mut AppConfig)) -> AppConfig {
        let mut config = self.config.write().expect("workspace config lock poisoned");
        update(&mut config);
        config.clone()
    }

    pub fn sessions(&self) -> Vec<SessionMetadata> {
        self.sessions
            .read()
            .expect("workspace sessions lock poisoned")
            .clone()
    }

    pub fn archived_sessions(&self) -> Vec<SessionMetadata> {
        self.archived_sessions
            .read()
            .expect("workspace archived sessions lock poisoned")
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
            return Err(Error::SessionNotFound(id).into());
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
            return Err(Error::SessionNotFound(id).into());
        }
        self.update_config(|config| *config = next)
    }

    pub fn create_session(&self, params: NewSession) -> Result<SessionMetadata> {
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| anyhow!("workspace sessions lock poisoned"))?;
        let archived_sessions = self
            .archived_sessions
            .read()
            .map_err(|_| anyhow!("workspace archived sessions lock poisoned"))?;
        let session = self.create_session_locked(&sessions, &archived_sessions, &params)?;
        if params.active == Some(true)
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
        archived_sessions: &[SessionMetadata],
        params: &NewSession,
    ) -> Result<SessionMetadata> {
        let view = params.view.clone().unwrap_or_default();
        let interceptors = params.interceptors.clone().unwrap_or_default();
        let mut id = now_millis();
        while sessions.iter().any(|session| session.id == id)
            || archived_sessions.iter().any(|session| session.id == id)
        {
            id += 1;
        }
        let session = SessionMetadata {
            id,
            name: params
                .name
                .clone()
                .unwrap_or_else(|| format!("Session-{id}")),
            created_at: now_millis(),
            description: params.description.clone(),
        };
        let directory = self.session_dir(id);
        let result = (|| {
            fs::create_dir_all(directory.join("blob"))?;
            write_json_atomic(&directory.join("metadata.json"), &session)?;
            write_json_atomic(&directory.join("view.json"), &view)?;
            write_json_atomic(&directory.join("interceptors.json"), &interceptors)
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
            .ok_or(Error::SessionNotFound(id))?;
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

    pub fn archive_session(&self, id: u64) -> Result<SessionMetadata> {
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| anyhow!("workspace sessions lock poisoned"))?;
        let index = sessions
            .iter()
            .position(|session| session.id == id)
            .ok_or(Error::SessionNotFound(id))?;
        if self.active_session_id() == Some(id) {
            return Err(Error::ActiveSessionArchive(id).into());
        }
        let mut archived_sessions = self
            .archived_sessions
            .write()
            .map_err(|_| anyhow!("workspace archived sessions lock poisoned"))?;
        let directory = self.session_dir(id);
        let archived = self.archived_session_dir(id);
        if archived.exists() {
            return Err(Error::ArchivedSessionAlreadyExists(id).into());
        }
        fs::rename(&directory, &archived)?;
        let session = sessions.remove(index);
        archived_sessions.push(session.clone());
        archived_sessions.sort_by_key(|session| session.created_at);
        Ok(session)
    }

    pub fn restore_session(&self, id: u64) -> Result<SessionMetadata> {
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| anyhow!("workspace sessions lock poisoned"))?;
        let mut archived_sessions = self
            .archived_sessions
            .write()
            .map_err(|_| anyhow!("workspace archived sessions lock poisoned"))?;
        let index = archived_sessions
            .iter()
            .position(|session| session.id == id)
            .ok_or(Error::ArchivedSessionNotFound(id))?;
        let directory = self.session_dir(id);
        if directory.exists() {
            return Err(Error::SessionAlreadyExists(id).into());
        }
        fs::rename(self.archived_session_dir(id), &directory)?;
        let session = archived_sessions.remove(index);
        sessions.push(session.clone());
        sessions.sort_by_key(|session| session.created_at);
        Ok(session)
    }

    pub fn delete_archived_session(&self, id: u64) -> Result<()> {
        let mut archived_sessions = self
            .archived_sessions
            .write()
            .map_err(|_| anyhow!("workspace archived sessions lock poisoned"))?;
        let index = archived_sessions
            .iter()
            .position(|session| session.id == id)
            .ok_or(Error::ArchivedSessionNotFound(id))?;
        let directory = self.archived_session_dir(id);
        let deleted = self
            .root
            .join(ARCHIVED_SESSIONS_DIRECTORY)
            .join(format!(".deleted-{id}"));
        fs::rename(&directory, &deleted)?;
        archived_sessions.remove(index);
        if let Err(error) = fs::remove_dir_all(&deleted) {
            tracing::warn!("failed to remove archived session directory {id}: {error}");
        }
        Ok(())
    }

    pub fn session_dir(&self, id: u64) -> PathBuf {
        self.root.join(SESSIONS_DIRECTORY).join(id.to_string())
    }

    pub fn archived_session_dir(&self, id: u64) -> PathBuf {
        self.root
            .join(ARCHIVED_SESSIONS_DIRECTORY)
            .join(id.to_string())
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
            Err(Error::SessionNotFound(id).into())
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
        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::ScriptNotFound(name.to_string()).into());
            }
            Err(error) => {
                return Err(error).with_context(|| format!("failed to read script {name}"));
            }
        };
        Ok(Script {
            name: name.to_string(),
            content,
        })
    }

    pub fn save_script(&self, kind: ScriptKind, script: Script, overwrite: bool) -> Result<()> {
        validate_script_name(&script.name)?;
        let path = self.script_dir(kind).join(format!("{}.lua", script.name));
        if !overwrite && path.exists() {
            return Err(Error::ScriptAlreadyExists(script.name.clone()).into());
        }
        write_text_atomic(&path, &script.content)
    }

    pub fn delete_script(&self, kind: ScriptKind, name: &str) -> Result<()> {
        validate_script_name(name)?;
        match fs::remove_file(self.script_dir(kind).join(format!("{name}.lua"))) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err(Error::ScriptNotFound(name.to_string()).into())
            }
            Err(error) => Err(error).with_context(|| format!("failed to delete script {name}")),
        }
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
        return Err(Error::InvalidScriptName.into());
    }
    Ok(())
}

fn load_sessions(directory: &Path) -> Result<Vec<SessionMetadata>> {
    let mut sessions = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_dir()
            && let Ok(session) =
                read_json::<SessionMetadata>(entry.path().join("metadata.json").as_path())
            && entry.file_name().to_string_lossy() == session.id.to_string()
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
        Column, FilterColumn, FilterOption, NewSession, SessionFilter, SessionInterceptor,
        SessionInterceptors, SessionView,
    };

    use super::Workspace;

    #[test]
    fn selection_accepts_empty_directories_and_ignores_dot_entries_without_writing() {
        let root = tempdir().unwrap();
        assert!(Workspace::validate_selection(root.path()).is_ok());
        std::fs::write(root.path().join(".DS_Store"), b"keep").unwrap();
        std::fs::create_dir(root.path().join(".hidden")).unwrap();
        assert!(Workspace::validate_selection(root.path()).is_ok());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
    }

    #[test]
    fn selection_requires_valid_config_and_supported_schema_for_nonempty_directories() {
        let supported = format!(
            r#"{{"version":{}}}"#,
            crate::migration::CURRENT_WORKSPACE_SCHEMA_VERSION
        );
        let future = format!(
            r#"{{"version":{}}}"#,
            crate::migration::CURRENT_WORKSPACE_SCHEMA_VERSION + 1
        );
        let cases = [
            (None, None, false),
            (None, Some("{}"), false),
            (Some(supported.as_str()), None, false),
            (Some("not json"), Some("{}"), false),
            (Some("{}"), Some("{}"), false),
            (Some(future.as_str()), Some("{}"), false),
            (Some(supported.as_str()), Some("not json"), false),
            (
                Some(supported.as_str()),
                Some(r#"{"proxy_port":"bad"}"#),
                false,
            ),
            (Some(supported.as_str()), Some("{}"), true),
            (Some(r#"{"version":1}"#), Some("{}"), true),
        ];
        for (schema, config, valid) in cases {
            let root = tempdir().unwrap();
            std::fs::write(root.path().join("notes.txt"), b"keep").unwrap();
            if let Some(schema) = schema {
                std::fs::write(root.path().join("workspace_schema.json"), schema).unwrap();
            }
            if let Some(config) = config {
                std::fs::write(root.path().join("app_config.json"), config).unwrap();
            }
            let entries = std::fs::read_dir(root.path()).unwrap().count();
            assert_eq!(
                Workspace::validate_selection(root.path()).is_ok(),
                valid,
                "schema={schema:?}, config={config:?}"
            );
            assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), entries);
            if let Some(schema) = schema {
                assert_eq!(
                    std::fs::read_to_string(root.path().join("workspace_schema.json")).unwrap(),
                    schema
                );
            }
        }
    }

    #[test]
    fn active_session_cannot_be_archived_and_archive_can_be_restored_or_deleted() {
        let root = tempdir().unwrap();
        let workspace = Workspace::open(root.path()).unwrap();
        let session = workspace
            .create_session(NewSession {
                name: Some("capture".into()),
                active: Some(true),
                ..Default::default()
            })
            .unwrap();

        assert_eq!(workspace.active_session_id(), Some(session.id));
        assert!(
            workspace
                .archive_session(session.id)
                .unwrap_err()
                .to_string()
                .contains("active session")
        );
        workspace.replace_active_session(None).unwrap();
        workspace.archive_session(session.id).unwrap();
        assert!(workspace.sessions().is_empty());
        assert_eq!(workspace.archived_sessions(), vec![session.clone()]);
        assert!(workspace.session_view(session.id).is_err());
        workspace.restore_session(session.id).unwrap();
        assert_eq!(workspace.sessions(), vec![session.clone()]);
        workspace.archive_session(session.id).unwrap();
        workspace.delete_archived_session(session.id).unwrap();
        assert!(workspace.archived_sessions().is_empty());
        assert_eq!(workspace.active_session_id(), None);
    }

    #[test]
    fn archived_sessions_survive_workspace_reopen() {
        let root = tempdir().unwrap();
        let workspace = Workspace::open(root.path()).unwrap();
        let session = workspace
            .create_session(NewSession {
                name: Some("archive".into()),
                ..Default::default()
            })
            .unwrap();
        workspace.replace_active_session(None).unwrap();
        workspace.archive_session(session.id).unwrap();
        drop(workspace);

        let reopened = Workspace::open(root.path()).unwrap();
        assert!(reopened.sessions().is_empty());
        assert_eq!(reopened.archived_sessions(), vec![session]);
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
            .create_session(NewSession {
                name: Some("first".into()),
                ..Default::default()
            })
            .unwrap();
        workspace
            .replace_session_view(
                first.id,
                SessionView {
                    columns: vec![Column::Stage { width: 91.0 }],
                    filter: SessionFilter {
                        option: Some(FilterOption::Column {
                            column: FilterColumn::Stage,
                            regex: false,
                        }),
                        input: "response".into(),
                    },
                },
            )
            .unwrap();
        let second = workspace
            .create_session(NewSession {
                name: Some("second".into()),
                ..Default::default()
            })
            .unwrap();

        assert_eq!(
            workspace.session_view(second.id).unwrap(),
            SessionView::default()
        );
    }

    #[test]
    fn new_session_accepts_initial_view_interceptors_and_active_flag() {
        let root = tempdir().unwrap();
        let workspace = Workspace::open(root.path()).unwrap();
        let view = SessionView {
            columns: vec![Column::Stage { width: 91.0 }],
            filter: SessionFilter {
                option: None,
                input: "example".into(),
            },
        };
        let interceptors = SessionInterceptors {
            request: vec![SessionInterceptor {
                name: "rewrite".into(),
                enabled: true,
            }],
            response: vec![],
        };
        let session = workspace
            .create_session(NewSession {
                view: Some(view.clone()),
                interceptors: Some(interceptors.clone()),
                active: Some(true),
                ..Default::default()
            })
            .unwrap();

        assert_eq!(workspace.session_view(session.id).unwrap(), view);
        assert_eq!(
            workspace.session_interceptors(session.id).unwrap(),
            interceptors
        );
        assert_eq!(workspace.active_session_id(), Some(session.id));
    }

    #[test]
    fn new_session_is_not_active_unless_requested() {
        let root = tempdir().unwrap();
        let workspace = Workspace::open(root.path()).unwrap();
        workspace.create_session(NewSession::default()).unwrap();
        assert_eq!(workspace.active_session_id(), None);
        workspace
            .create_session(NewSession {
                active: Some(false),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(workspace.active_session_id(), None);
        let third = workspace
            .create_session(NewSession {
                active: Some(true),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(workspace.active_session_id(), Some(third.id));
    }

    #[test]
    fn later_session_does_not_replace_active_and_active_can_be_switched_or_cleared() {
        let root = tempdir().unwrap();
        let workspace = Workspace::open(root.path()).unwrap();
        let first = workspace
            .create_session(NewSession {
                name: Some("first".into()),
                active: Some(true),
                ..Default::default()
            })
            .unwrap();
        let second = workspace
            .create_session(NewSession {
                name: Some("second".into()),
                ..Default::default()
            })
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
        let session = workspace
            .create_session(NewSession {
                name: Some("one".into()),
                ..Default::default()
            })
            .unwrap();
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
        let session = workspace.create_session(NewSession::default()).unwrap();
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
        let session = workspace
            .create_session(NewSession {
                name: Some("one".into()),
                ..Default::default()
            })
            .unwrap();

        assert_eq!(
            workspace.session_interceptors(session.id).unwrap(),
            SessionInterceptors::default()
        );
    }

    #[test]
    fn session_interceptors_round_trip_without_resolving_script_files() {
        let root = tempdir().unwrap();
        let workspace = Workspace::open(root.path()).unwrap();
        let session = workspace
            .create_session(NewSession {
                name: Some("one".into()),
                ..Default::default()
            })
            .unwrap();
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
