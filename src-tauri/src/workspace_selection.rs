use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::RwLock,
};

use anyhow::Result;
use proxy_crab_mgr::dto::ManagerError;
use serde::{Deserialize, Serialize};

const POINTER_FILE: &str = "config.json";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkspacePaths {
    pub current_path: String,
    pub configured_path: String,
}

#[derive(Serialize, Deserialize)]
struct WorkspacePointer {
    workspace_path: PathBuf,
}

pub struct WorkspaceSelection {
    app_data_dir: PathBuf,
    current_path: PathBuf,
    configured_path: RwLock<PathBuf>,
}

impl WorkspaceSelection {
    pub fn open(app_data_dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&app_data_dir)?;
        let default_path = app_data_dir.join("workspace");
        fs::create_dir_all(&default_path)?;
        let pointer_path = app_data_dir.join(POINTER_FILE);
        let current_path = match read_pointer(&pointer_path) {
            Ok(pointer) if valid_existing_workspace(&pointer.workspace_path) => {
                pointer.workspace_path
            }
            _ => {
                write_pointer(&pointer_path, &default_path)?;
                default_path
            }
        };
        Ok(Self {
            app_data_dir,
            configured_path: RwLock::new(current_path.clone()),
            current_path,
        })
    }

    pub fn current_path(&self) -> &Path {
        &self.current_path
    }

    pub fn paths(&self) -> Result<WorkspacePaths, ManagerError> {
        let configured = self
            .configured_path
            .read()
            .map_err(|_| ManagerError::internal("workspace selection lock poisoned"))?;
        Ok(WorkspacePaths {
            current_path: self.current_path.to_string_lossy().into_owned(),
            configured_path: configured.to_string_lossy().into_owned(),
        })
    }

    pub fn set_for_next_start(&self, path: &str) -> Result<WorkspacePaths, ManagerError> {
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return Err(ManagerError::bad_request("workspace path must be absolute"));
        }
        write_pointer(&self.app_data_dir.join(POINTER_FILE), &path).map_err(|error| {
            ManagerError::internal(format!("failed to save workspace selection: {error}"))
        })?;
        *self
            .configured_path
            .write()
            .map_err(|_| ManagerError::internal("workspace selection lock poisoned"))? = path;
        self.paths()
    }
}

fn read_pointer(path: &Path) -> Result<WorkspacePointer> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn write_pointer(path: &Path, workspace_path: &Path) -> Result<()> {
    let temporary = path.with_extension("tmp");
    let mut file = File::create(&temporary)?;
    serde_json::to_writer_pretty(
        &mut file,
        &WorkspacePointer {
            workspace_path: workspace_path.to_path_buf(),
        },
    )?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    fs::rename(temporary, path)?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::WorkspaceSelection;

    #[test]
    fn missing_pointer_uses_and_persists_default_workspace() {
        let app_data = tempdir().unwrap();
        let selection = WorkspaceSelection::open(app_data.path().to_path_buf()).unwrap();
        let expected = app_data.path().join("workspace");

        assert_eq!(selection.current_path(), expected);
        assert_eq!(
            selection.paths().unwrap().configured_path,
            expected.to_string_lossy()
        );
        assert!(app_data.path().join("config.json").is_file());
    }

    #[test]
    fn setting_next_workspace_does_not_change_current_workspace() {
        let app_data = tempdir().unwrap();
        let next = tempdir().unwrap();
        let selection = WorkspaceSelection::open(app_data.path().to_path_buf()).unwrap();
        let current = selection.current_path().to_path_buf();

        let paths = selection
            .set_for_next_start(next.path().to_str().unwrap())
            .unwrap();

        assert_eq!(paths.current_path, current.to_string_lossy());
        assert_eq!(paths.configured_path, next.path().to_string_lossy());
        assert!(selection.set_for_next_start("relative/path").is_err());
    }

    #[test]
    fn missing_configured_workspace_falls_back_to_default_on_restart() {
        let app_data = tempdir().unwrap();
        let selection = WorkspaceSelection::open(app_data.path().to_path_buf()).unwrap();
        let missing = app_data.path().join("missing");
        selection
            .set_for_next_start(missing.to_str().unwrap())
            .unwrap();
        drop(selection);

        let reopened = WorkspaceSelection::open(app_data.path().to_path_buf()).unwrap();
        let expected = app_data.path().join("workspace");
        assert_eq!(reopened.current_path(), expected);
        assert_eq!(
            reopened.paths().unwrap().configured_path,
            expected.to_string_lossy()
        );
    }

    #[test]
    fn malformed_pointer_is_replaced_with_default_workspace() {
        let app_data = tempdir().unwrap();
        std::fs::write(app_data.path().join("config.json"), b"not json").unwrap();

        let selection = WorkspaceSelection::open(app_data.path().to_path_buf()).unwrap();
        let expected = app_data.path().join("workspace");

        assert_eq!(selection.current_path(), expected);
        let pointer: serde_json::Value =
            serde_json::from_slice(&std::fs::read(app_data.path().join("config.json")).unwrap())
                .unwrap();
        assert_eq!(
            pointer["workspace_path"],
            expected.to_string_lossy().as_ref()
        );
    }
}
