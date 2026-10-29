use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
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

/// A staged bundle write, including the bytes observed during conflict checking.
pub struct ImportFile {
    pub path: String,
    pub data: Vec<u8>,
    pub expected: Option<Vec<u8>>,
}

pub struct Workspace {
    pub(crate) file_mutation: Arc<Mutex<()>>,
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
        for directory in [
            "column",
            "filter",
            "routing",
            "request",
            "response",
            "redaction",
        ] {
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
            file_mutation: Arc::new(Mutex::new(())),
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

    /// Publish new sessions only after every file and session has been written successfully.
    pub fn import_batch(
        &self,
        files: Vec<ImportFile>,
        params: Vec<NewSession>,
    ) -> Result<Vec<SessionMetadata>> {
        let _mutation = self.file_mutation.lock().unwrap();
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| anyhow!("sessions lock poisoned"))?;
        let archived = self
            .archived_sessions
            .read()
            .map_err(|_| anyhow!("archives lock poisoned"))?;
        // Reject changed targets before touching any files, including symlink ancestors.
        for file in &files {
            if !["scripts/", "assets/"]
                .iter()
                .any(|prefix| file.path.starts_with(prefix))
                || file.path.contains('\\')
                || file
                    .path
                    .split('/')
                    .any(|part| part.is_empty() || part == "." || part == "..")
            {
                anyhow::bail!("invalid import target: {}", file.path);
            }
            let path = self.root.join(&file.path);
            for ancestor in path.ancestors().take_while(|p| *p != self.root) {
                if fs::symlink_metadata(ancestor).is_ok_and(|m| m.is_symlink()) {
                    anyhow::bail!("symlink target: {}", file.path);
                }
            }
            let current = match fs::read(&path) {
                Ok(bytes) => Some(bytes),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(e.into()),
            };
            if current != file.expected {
                anyhow::bail!("import target changed: {}", file.path);
            }
        }
        let mut written = Vec::new();
        let mut created: Vec<SessionMetadata> = Vec::new();
        let result: Result<()> = (|| {
            for file in &files {
                let path = self.root.join(&file.path);
                fs::create_dir_all(path.parent().unwrap())?;
                // Track before writing: partial writes must also be restored.
                written.push(file);
                write_import_file(&path, &file.data)?;
            }
            let mut reserved = sessions.clone();
            for param in &params {
                let session = self.create_session_locked(&reserved, &archived, param)?;
                reserved.push(session.clone());
                created.push(session);
            }
            Ok(())
        })();
        if let Err(error) = result {
            let mut failures = Vec::new();
            for session in &created {
                if let Err(e) = fs::remove_dir_all(self.session_dir(session.id)) {
                    failures.push(e.to_string());
                }
            }
            for file in written.into_iter().rev() {
                let path = self.root.join(&file.path);
                let restored = match &file.expected {
                    Some(bytes) => write_import_file(&path, bytes),
                    None => fs::remove_file(path).map_err(anyhow::Error::from),
                };
                if let Err(e) = restored {
                    failures.push(e.to_string());
                }
            }
            if !failures.is_empty() {
                anyhow::bail!("{error}; rollback failed: {}", failures.join("; "));
            }
            return Err(error);
        }
        sessions.extend(created.clone());
        Ok(created)
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
        let root = self.script_dir(kind);
        let mut directories = vec![root.clone()];
        while let Some(directory) = directories.pop() {
            for entry in fs::read_dir(directory)? {
                let entry = entry?;
                let file_type = entry.file_type()?;
                let path = entry.path();
                if file_type.is_dir() {
                    directories.push(path);
                } else if file_type.is_file()
                    && path.extension().and_then(|value| value.to_str()) == Some("lua")
                {
                    let name = path
                        .strip_prefix(&root)?
                        .with_extension("")
                        .to_string_lossy()
                        .replace('\\', "/");
                    scripts.push(Script {
                        name,
                        content: fs::read_to_string(path)?,
                    });
                }
            }
        }
        scripts.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(scripts)
    }

    pub fn get_script(&self, kind: ScriptKind, name: &str) -> Result<Script> {
        let path = self.script_path(kind, name)?;
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
        let _mutation = self.file_mutation.lock().unwrap();
        let path = self.script_path(kind, &script.name)?;
        if !overwrite && path.exists() {
            return Err(Error::ScriptAlreadyExists(script.name.clone()).into());
        }
        fs::create_dir_all(path.parent().unwrap())?;
        write_text_atomic(&path, &script.content)
    }

    pub fn delete_script(&self, kind: ScriptKind, name: &str) -> Result<()> {
        let _mutation = self.file_mutation.lock().unwrap();
        let path = self.script_path(kind, name)?;
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err(Error::ScriptNotFound(name.to_string()).into())
            }
            Err(error) => Err(error).with_context(|| format!("failed to delete script {name}")),
        }
    }

    fn script_path(&self, kind: ScriptKind, name: &str) -> Result<PathBuf> {
        validate_script_name(name)?;
        let root = self.script_dir(kind);
        let path = root.join(format!("{name}.lua"));
        // Do not follow symlinks in script paths, including the atomic-write temporary file.
        for candidate in path.ancestors().take_while(|ancestor| *ancestor != root) {
            if fs::symlink_metadata(candidate).is_ok_and(|metadata| metadata.is_symlink()) {
                return Err(Error::InvalidScriptName.into());
            }
        }
        if fs::symlink_metadata(path.with_extension("tmp"))
            .is_ok_and(|metadata| metadata.is_symlink())
        {
            return Err(Error::InvalidScriptName.into());
        }
        Ok(path)
    }

    fn script_dir(&self, kind: ScriptKind) -> PathBuf {
        let directory = match kind {
            ScriptKind::Redaction => "redaction",
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
        || name.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || !part.chars().all(|character| {
                    character.is_alphanumeric() || matches!(character, '-' | '_' | '.')
                })
        })
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

// Separate from script persistence: arbitrary asset extensions must not alias .tmp paths.
fn write_import_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path)?;
    Ok(())
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
    fn nested_scripts_round_trip_for_every_kind() {
        use crate::model::{Script, ScriptKind};
        let root = tempdir().unwrap();
        let workspace = Workspace::open(root.path()).unwrap();
        for kind in [
            ScriptKind::Column,
            ScriptKind::Filter,
            ScriptKind::Routing,
            ScriptKind::RequestInterceptor,
            ScriptKind::ResponseInterceptor,
            ScriptKind::Redaction,
        ] {
            for name in ["root", "业务/nested/example"] {
                workspace
                    .save_script(
                        kind,
                        Script {
                            name: name.into(),
                            content: "return true".into(),
                        },
                        false,
                    )
                    .unwrap();
                assert!(
                    workspace
                        .save_script(
                            kind,
                            Script {
                                name: name.into(),
                                content: "".into()
                            },
                            false
                        )
                        .is_err()
                );
                assert_eq!(
                    workspace.get_script(kind, name).unwrap().content,
                    "return true"
                );
                workspace
                    .save_script(
                        kind,
                        Script {
                            name: name.into(),
                            content: "return false".into(),
                        },
                        true,
                    )
                    .unwrap();
            }
            assert_eq!(
                workspace
                    .list_scripts(kind)
                    .unwrap()
                    .iter()
                    .map(|script| script.name.as_str())
                    .collect::<Vec<_>>(),
                vec!["root", "业务/nested/example"]
            );
            workspace
                .delete_script(kind, "业务/nested/example")
                .unwrap();
            assert_eq!(workspace.list_scripts(kind).unwrap().len(), 1);
            for name in [
                "../escape",
                "/absolute",
                "a/../../escape",
                "a//b",
                "a/",
                "./a",
                "a\\b",
            ] {
                assert!(
                    workspace
                        .save_script(
                            kind,
                            Script {
                                name: name.into(),
                                content: "".into()
                            },
                            false
                        )
                        .is_err(),
                    "{name}"
                );
                assert!(workspace.get_script(kind, name).is_err());
                assert!(workspace.delete_script(kind, name).is_err());
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn nested_scripts_do_not_follow_symlinks() {
        use crate::model::{Script, ScriptKind};
        use std::os::unix::fs::symlink;
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let workspace = Workspace::open(root.path()).unwrap();
        std::fs::write(outside.path().join("secret.lua"), "secret").unwrap();
        symlink(outside.path(), root.path().join("scripts/filter/link")).unwrap();
        for name in ["link/secret", "link/new"] {
            assert!(workspace.get_script(ScriptKind::Filter, name).is_err());
            assert!(workspace.delete_script(ScriptKind::Filter, name).is_err());
            assert!(
                workspace
                    .save_script(
                        ScriptKind::Filter,
                        Script {
                            name: name.into(),
                            content: "changed".into()
                        },
                        true
                    )
                    .is_err()
            );
        }
        assert!(
            workspace
                .list_scripts(ScriptKind::Filter)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            std::fs::read_to_string(outside.path().join("secret.lua")).unwrap(),
            "secret"
        );
    }

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
                ..Default::default()
            })
            .unwrap();
        workspace.replace_active_session(Some(session.id)).unwrap();

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
    fn new_session_accepts_initial_view_and_interceptors() {
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
                ..Default::default()
            })
            .unwrap();

        assert_eq!(workspace.session_view(session.id).unwrap(), view);
        assert_eq!(
            workspace.session_interceptors(session.id).unwrap(),
            interceptors
        );
        // 创建不再携带活跃语义：激活必须显式调用 replace_active_session。
        assert_eq!(workspace.active_session_id(), None);
    }

    #[test]
    fn creating_a_session_never_changes_the_active_session() {
        let root = tempdir().unwrap();
        let workspace = Workspace::open(root.path()).unwrap();
        workspace.create_session(NewSession::default()).unwrap();
        assert_eq!(workspace.active_session_id(), None);
        let second = workspace
            .create_session(NewSession {
                name: Some("second".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(workspace.active_session_id(), None);

        workspace.replace_active_session(Some(second.id)).unwrap();
        assert_eq!(workspace.active_session_id(), Some(second.id));
    }

    #[test]
    fn later_session_does_not_replace_active_and_active_can_be_switched_or_cleared() {
        let root = tempdir().unwrap();
        let workspace = Workspace::open(root.path()).unwrap();
        let first = workspace
            .create_session(NewSession {
                name: Some("first".into()),
                ..Default::default()
            })
            .unwrap();
        workspace.replace_active_session(Some(first.id)).unwrap();
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

#[cfg(test)]
mod import_tests {
    use super::*;
    #[test]
    fn overwrites_preserve_already_open_resource_streams() {
        use std::io::Read;
        let dir = tempfile::tempdir().unwrap();
        let workspace = Workspace::open(dir.path()).unwrap();
        fs::create_dir_all(dir.path().join("assets")).unwrap();
        let path = dir.path().join("assets/body.bin");
        fs::write(&path, b"old").unwrap();
        let mut stream = File::open(&path).unwrap();
        workspace
            .import_batch(
                vec![ImportFile {
                    path: "assets/body.bin".into(),
                    data: b"replacement".to_vec(),
                    expected: Some(b"old".to_vec()),
                }],
                vec![],
            )
            .unwrap();
        let mut old = Vec::new();
        stream.read_to_end(&mut old).unwrap();
        assert_eq!(old, b"old");
        assert_eq!(fs::read(path).unwrap(), b"replacement");
    }

    #[test]
    fn failed_file_placement_restores_overwrites_and_removes_new_files() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::open(directory.path()).unwrap();
        fs::write(
            directory.path().join("scripts/request/old.lua"),
            b"original",
        )
        .unwrap();
        let file = |path: &str, data: &[u8], expected| ImportFile {
            path: path.into(),
            data: data.into(),
            expected,
        };
        // The last file cannot be placed because the previous staged file occupies its parent.
        let result = workspace.import_batch(
            vec![
                file(
                    "scripts/request/old.lua",
                    b"replacement",
                    Some(b"original".to_vec()),
                ),
                file("assets/block", b"new", None),
                file("assets/block/child", b"unreachable", None),
            ],
            vec![NewSession::default(), NewSession::default()],
        );
        assert!(result.is_err());
        assert_eq!(
            fs::read(directory.path().join("scripts/request/old.lua")).unwrap(),
            b"original"
        );
        assert!(!directory.path().join("assets/block").exists());
        assert!(workspace.sessions().is_empty());
    }
    #[test]
    fn sessions_are_not_published_when_creation_fails_and_assets_are_restored() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::open(directory.path()).unwrap();
        fs::remove_dir(directory.path().join("sessions")).unwrap();
        fs::write(directory.path().join("sessions"), b"obstruction").unwrap();
        let result = workspace.import_batch(
            vec![ImportFile {
                path: "assets/new.txt".into(),
                data: b"new".to_vec(),
                expected: None,
            }],
            vec![NewSession::default()],
        );
        assert!(result.is_err());
        assert!(workspace.sessions().is_empty());
        assert!(!directory.path().join("assets/new.txt").exists());
    }
}
