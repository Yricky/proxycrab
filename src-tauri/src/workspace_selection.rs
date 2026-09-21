use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Result;
use proxy_crab_mgr::dto::ManagerError;
use proxy_crab_mitm::{ProxyCrab, log_buffer::LogBuffer, workspace::Workspace};
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
            current_path,
        })
    }

    pub fn current_path(&self) -> &Path {
        &self.current_path
    }

    pub fn paths(&self) -> Result<WorkspacePaths, ManagerError> {
        Ok(WorkspacePaths {
            current_path: self.current_path.to_string_lossy().into_owned(),
            configured_path: self.current_path.to_string_lossy().into_owned(),
        })
    }

    /// Validate and prepare the selected workspace before committing the restart pointer.
    /// Returns false for the current directory, including symlink aliases.
    pub fn select_for_restart(&self, path: &Path) -> Result<bool, ManagerError> {
        let path = fs::canonicalize(path)
            .map_err(|error| ManagerError::bad_request(format!("无法访问所选目录：{error}")))?;
        if path
            == fs::canonicalize(&self.current_path)
                .map_err(|error| ManagerError::internal(format!("无法访问当前工作区：{error}")))?
        {
            return Ok(false);
        }
        Workspace::validate_selection(&path)
            .map_err(|error| ManagerError::bad_request(format!("{error:#}")))?;
        if !valid_existing_workspace(&path) {
            return Err(ManagerError::bad_request("所选工作区目录不可写"));
        }
        // Catch initialization/migration errors and workspaces locked by another process
        // while the current workspace and its saved pointer are still untouched.
        let runtime = ProxyCrab::open(&path, Arc::new(LogBuffer::default()))
            .map_err(|error| ManagerError::bad_request(format!("无法打开所选工作区：{error:#}")))?;
        drop(runtime);
        write_pointer(&self.app_data_dir.join(POINTER_FILE), &path)
            .map_err(|error| ManagerError::internal(format!("保存工作区路径失败：{error}")))?;
        Ok(true)
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
    use std::{fs, sync::Arc};

    use proxy_crab_mitm::{ProxyCrab, log_buffer::LogBuffer, model::AppConfig};
    use tempfile::tempdir;

    use super::{WorkspaceSelection, read_pointer, write_pointer};

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
    fn selecting_empty_workspace_initializes_it_and_persists_restart_pointer() {
        let app_data = tempdir().unwrap();
        let next = tempdir().unwrap();
        let selection = WorkspaceSelection::open(app_data.path().to_path_buf()).unwrap();
        let current = selection.current_path().to_path_buf();

        fs::write(next.path().join(".DS_Store"), b"keep").unwrap();
        assert!(selection.select_for_restart(next.path()).unwrap());
        assert_eq!(selection.current_path(), current);
        assert!(next.path().join("workspace_schema.json").is_file());
        assert!(next.path().join("app_config.json").is_file());
        assert_eq!(fs::read(next.path().join(".DS_Store")).unwrap(), b"keep");
        let reopened = WorkspaceSelection::open(app_data.path().to_path_buf()).unwrap();
        assert_eq!(
            reopened.current_path(),
            fs::canonicalize(next.path()).unwrap()
        );
    }

    #[test]
    fn existing_workspace_retains_its_configuration_and_proxy_starts_stopped() {
        let app_data = tempdir().unwrap();
        let next = tempdir().unwrap();
        let config = AppConfig {
            proxy_port: 9876,
            api_port: 19876,
            ..AppConfig::default()
        };
        let runtime = ProxyCrab::open(next.path(), Arc::new(LogBuffer::default())).unwrap();
        runtime.workspace().replace_config(config.clone()).unwrap();
        drop(runtime);

        let selection = WorkspaceSelection::open(app_data.path().to_path_buf()).unwrap();
        assert!(selection.select_for_restart(next.path()).unwrap());
        let reopened = ProxyCrab::open(next.path(), Arc::new(LogBuffer::default())).unwrap();
        assert_eq!(reopened.config(), config);
        assert!(matches!(
            reopened.proxy_status(),
            proxy_crab_mitm::model::ProxyStatus::Stopped
        ));
    }

    #[test]
    fn current_directory_does_not_trigger_restart_or_initialize_it() {
        let app_data = tempdir().unwrap();
        let selection = WorkspaceSelection::open(app_data.path().to_path_buf()).unwrap();
        assert!(
            !selection
                .select_for_restart(selection.current_path())
                .unwrap()
        );
        assert!(!selection.current_path().join("app_config.json").exists());
    }

    #[cfg(unix)]
    #[test]
    fn current_directory_symlink_does_not_trigger_restart() {
        let app_data = tempdir().unwrap();
        let selection = WorkspaceSelection::open(app_data.path().to_path_buf()).unwrap();
        let alias = app_data.path().join("alias");
        std::os::unix::fs::symlink(selection.current_path(), &alias).unwrap();
        assert!(!selection.select_for_restart(&alias).unwrap());
    }

    #[test]
    fn invalid_nonempty_directory_is_rejected_without_modifying_it_or_pointer() {
        let app_data = tempdir().unwrap();
        let next = tempdir().unwrap();
        fs::write(next.path().join("notes.txt"), b"keep").unwrap();
        let selection = WorkspaceSelection::open(app_data.path().to_path_buf()).unwrap();
        let pointer = fs::read(app_data.path().join("config.json")).unwrap();
        assert!(selection.select_for_restart(next.path()).is_err());
        assert_eq!(
            fs::read(app_data.path().join("config.json")).unwrap(),
            pointer
        );
        assert_eq!(fs::read_dir(next.path()).unwrap().count(), 1);
    }

    #[test]
    fn locked_workspace_is_rejected_without_changing_pointer() {
        let app_data = tempdir().unwrap();
        let next = tempdir().unwrap();
        let _runtime = ProxyCrab::open(next.path(), Arc::new(LogBuffer::default())).unwrap();
        let selection = WorkspaceSelection::open(app_data.path().to_path_buf()).unwrap();
        assert!(selection.select_for_restart(next.path()).is_err());
        assert_eq!(
            read_pointer(&app_data.path().join("config.json"))
                .unwrap()
                .workspace_path,
            selection.current_path()
        );
    }

    #[cfg(unix)]
    #[test]
    fn unwritable_workspace_is_rejected_without_changing_pointer() {
        use std::os::unix::fs::PermissionsExt;
        let app_data = tempdir().unwrap();
        let next = tempdir().unwrap();
        let selection = WorkspaceSelection::open(app_data.path().to_path_buf()).unwrap();
        fs::set_permissions(next.path(), fs::Permissions::from_mode(0o555)).unwrap();
        let result = selection.select_for_restart(next.path());
        fs::set_permissions(next.path(), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(result.is_err());
        assert_eq!(
            read_pointer(&app_data.path().join("config.json"))
                .unwrap()
                .workspace_path,
            selection.current_path()
        );
    }

    #[test]
    fn missing_configured_workspace_falls_back_to_default_on_restart() {
        let app_data = tempdir().unwrap();
        let selection = WorkspaceSelection::open(app_data.path().to_path_buf()).unwrap();
        let missing = app_data.path().join("missing");
        assert!(selection.select_for_restart(&missing).is_err());
        write_pointer(&app_data.path().join("config.json"), &missing).unwrap();
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
