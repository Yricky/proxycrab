use std::{
    collections::HashSet,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::RwLock,
};

use proxy_crab_mgr::{
    dto::ManagerError,
    skill_install::{self, normalize_parent},
};
use serde::{Deserialize, Serialize};

const CONFIG_FILE: &str = "skill-paths.json";
const SKILL_DIRECTORY_NAME: &str = "proxycrab";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SkillPathState {
    pub parent_path: String,
    pub target_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SkillManagerState {
    pub configured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_error: Option<String>,
    pub paths: Vec<SkillPathState>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SkillRemovalError {
    pub parent_path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SkillSaveResult {
    pub state: SkillManagerState,
    pub removal_errors: Vec<SkillRemovalError>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SkillPathsConfig {
    paths: Vec<String>,
}

pub struct SkillManager {
    config_path: PathBuf,
    bundled_path: PathBuf,
    state: RwLock<SkillManagerState>,
}

impl SkillManager {
    pub fn open(
        app_data_dir: PathBuf,
        bundled_path: PathBuf,
        default_parent: Option<PathBuf>,
    ) -> Self {
        let config_path = app_data_dir.join(CONFIG_FILE);
        let state = load_initial_state(&config_path, default_parent);
        let manager = Self {
            config_path,
            bundled_path,
            state: RwLock::new(state),
        };
        if manager
            .state
            .read()
            .map(|state| state.config_error.is_none() && !state.paths.is_empty())
            .unwrap_or(false)
        {
            let _ = manager.sync();
        }
        manager
    }

    pub fn state(&self) -> Result<SkillManagerState, ManagerError> {
        self.state
            .read()
            .map(|state| state.clone())
            .map_err(|_| ManagerError::internal("Skill manager lock poisoned"))
    }

    pub fn sync(&self) -> Result<SkillManagerState, ManagerError> {
        let mut state = self
            .state
            .write()
            .map_err(|_| ManagerError::internal("Skill manager lock poisoned"))?;
        if state.config_error.is_some() {
            return Ok(state.clone());
        }
        sync_paths(&self.bundled_path, &mut state.paths);
        Ok(state.clone())
    }

    pub fn save_paths(
        &self,
        paths: Vec<String>,
        delete_removed: bool,
    ) -> Result<SkillSaveResult, ManagerError> {
        let normalized = normalize_paths(paths)?;
        let mut state = self
            .state
            .write()
            .map_err(|_| ManagerError::internal("Skill manager lock poisoned"))?;
        let previous: HashSet<String> = state
            .paths
            .iter()
            .map(|path| path.parent_path.clone())
            .collect();
        let current: HashSet<String> = normalized.iter().cloned().collect();

        write_config(&self.config_path, &normalized)?;

        let mut removal_errors = Vec::new();
        if delete_removed {
            for parent_path in previous.difference(&current) {
                if let Err(error) = skill_install::uninstall(parent_path) {
                    tracing::warn!("failed to remove managed Skill at {parent_path}: {error}");
                    removal_errors.push(SkillRemovalError {
                        parent_path: parent_path.clone(),
                        message: error.message,
                    });
                }
            }
        }

        state.configured = true;
        state.config_error = None;
        state.paths = normalized.into_iter().map(path_state).collect();
        sync_paths(&self.bundled_path, &mut state.paths);
        Ok(SkillSaveResult {
            state: state.clone(),
            removal_errors,
        })
    }
}

fn load_initial_state(config_path: &Path, default_parent: Option<PathBuf>) -> SkillManagerState {
    match fs::read(config_path) {
        Ok(bytes) => match serde_json::from_slice::<SkillPathsConfig>(&bytes)
            .map_err(|error| ManagerError::internal(format!("read Skill path config: {error}")))
            .and_then(|config| normalize_paths(config.paths))
        {
            Ok(paths) => SkillManagerState {
                configured: true,
                config_error: None,
                paths: paths.into_iter().map(path_state).collect(),
            },
            Err(error) => SkillManagerState {
                configured: true,
                config_error: Some(error.message),
                paths: Vec::new(),
            },
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            adopt_default(config_path, default_parent)
        }
        Err(error) => SkillManagerState {
            configured: true,
            config_error: Some(format!("read Skill path config: {error}")),
            paths: Vec::new(),
        },
    }
}

fn adopt_default(config_path: &Path, default_parent: Option<PathBuf>) -> SkillManagerState {
    let Some(parent) = default_parent.filter(|parent| {
        fs::metadata(parent.join(SKILL_DIRECTORY_NAME))
            .map(|metadata| metadata.is_dir())
            .unwrap_or(false)
    }) else {
        return SkillManagerState {
            configured: false,
            config_error: None,
            paths: Vec::new(),
        };
    };
    let paths = match normalize_paths(vec![parent.to_string_lossy().into_owned()]) {
        Ok(paths) => paths,
        Err(error) => {
            return SkillManagerState {
                configured: false,
                config_error: Some(error.message),
                paths: Vec::new(),
            };
        }
    };
    if let Err(error) = write_config(config_path, &paths) {
        return SkillManagerState {
            configured: false,
            config_error: Some(error.message),
            paths: paths.into_iter().map(path_state).collect(),
        };
    }
    SkillManagerState {
        configured: true,
        config_error: None,
        paths: paths.into_iter().map(path_state).collect(),
    }
}

fn normalize_paths(paths: Vec<String>) -> Result<Vec<String>, ManagerError> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for path in paths {
        let path = normalize_parent(&path)?;
        if seen.insert(path.clone()) {
            normalized.push(path.to_string_lossy().into_owned());
        }
    }
    Ok(normalized)
}

fn path_state(parent_path: String) -> SkillPathState {
    let target_path = Path::new(&parent_path)
        .join(SKILL_DIRECTORY_NAME)
        .to_string_lossy()
        .into_owned();
    SkillPathState {
        parent_path,
        target_path,
        error: None,
    }
}

fn sync_paths(bundled_path: &Path, paths: &mut [SkillPathState]) {
    for path in paths {
        path.error = skill_install::install(bundled_path, &path.parent_path, true)
            .err()
            .map(|error| {
                tracing::warn!(
                    "failed to synchronize managed Skill at {}: {error}",
                    path.parent_path
                );
                error.message
            });
    }
}

fn write_config(config_path: &Path, paths: &[String]) -> Result<(), ManagerError> {
    let temporary = config_path.with_extension("tmp");
    let mut file = File::create(&temporary)
        .map_err(|error| ManagerError::internal(format!("create Skill path config: {error}")))?;
    serde_json::to_writer_pretty(
        &mut file,
        &SkillPathsConfig {
            paths: paths.to_vec(),
        },
    )
    .map_err(|error| ManagerError::internal(format!("write Skill path config: {error}")))?;
    file.write_all(b"\n")
        .and_then(|_| file.sync_all())
        .and_then(|_| fs::rename(&temporary, config_path))
        .map_err(|error| ManagerError::internal(format!("save Skill path config: {error}")))
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{SkillManager, SkillPathsConfig};

    fn bundled_skill() -> tempfile::TempDir {
        let bundled = tempdir().unwrap();
        std::fs::write(bundled.path().join("SKILL.md"), "bundled").unwrap();
        bundled
    }

    #[test]
    fn missing_config_and_default_requires_setup() {
        let app_data = tempdir().unwrap();
        let bundled = bundled_skill();
        let default_parent = app_data.path().join("default-skills");
        let manager = SkillManager::open(
            app_data.path().to_path_buf(),
            bundled.path().to_path_buf(),
            Some(default_parent),
        );
        let state = manager.state().unwrap();
        assert!(!state.configured);
        assert!(state.config_error.is_none());
        assert!(state.paths.is_empty());
        assert!(!app_data.path().join("skill-paths.json").exists());
    }

    #[test]
    fn a_default_target_file_is_not_adopted() {
        let app_data = tempdir().unwrap();
        let bundled = bundled_skill();
        let default_parent = app_data.path().join("default-skills");
        std::fs::create_dir_all(&default_parent).unwrap();
        std::fs::write(default_parent.join("proxycrab"), "not a directory").unwrap();
        let manager = SkillManager::open(
            app_data.path().to_path_buf(),
            bundled.path().to_path_buf(),
            Some(default_parent),
        );
        assert!(!manager.state().unwrap().configured);
        assert!(!app_data.path().join("skill-paths.json").exists());
    }

    #[test]
    fn existing_default_is_adopted_persisted_and_synchronized() {
        let app_data = tempdir().unwrap();
        let bundled = bundled_skill();
        let default_parent = app_data.path().join("default-skills");
        let target = default_parent.join("proxycrab");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("SKILL.md"), "old").unwrap();
        let manager = SkillManager::open(
            app_data.path().to_path_buf(),
            bundled.path().to_path_buf(),
            Some(default_parent.clone()),
        );
        let state = manager.state().unwrap();
        assert!(state.configured);
        assert_eq!(state.paths.len(), 1);
        assert_eq!(state.paths[0].parent_path, default_parent.to_string_lossy());
        assert!(state.paths[0].error.is_none());
        assert_eq!(
            std::fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "bundled"
        );
        assert!(app_data.path().join("skill-paths.json").is_file());
    }

    #[test]
    fn persisted_paths_are_synchronized_on_open() {
        let app_data = tempdir().unwrap();
        let bundled = bundled_skill();
        let parent = app_data.path().join("managed");
        let manager = SkillManager::open(
            app_data.path().to_path_buf(),
            bundled.path().to_path_buf(),
            None,
        );
        manager
            .save_paths(vec![parent.to_string_lossy().into_owned()], false)
            .unwrap();
        std::fs::write(parent.join("proxycrab/SKILL.md"), "edited").unwrap();
        drop(manager);

        let reopened = SkillManager::open(
            app_data.path().to_path_buf(),
            bundled.path().to_path_buf(),
            None,
        );

        assert!(reopened.state().unwrap().paths[0].error.is_none());
        assert_eq!(
            std::fs::read_to_string(parent.join("proxycrab/SKILL.md")).unwrap(),
            "bundled"
        );
    }

    #[test]
    fn existing_empty_config_is_valid() {
        let app_data = tempdir().unwrap();
        let bundled = bundled_skill();
        std::fs::write(app_data.path().join("skill-paths.json"), r#"{"paths":[]}"#).unwrap();
        let manager = SkillManager::open(
            app_data.path().to_path_buf(),
            bundled.path().to_path_buf(),
            None,
        );
        let state = manager.state().unwrap();
        assert!(state.configured);
        assert!(state.config_error.is_none());
        assert!(state.paths.is_empty());
    }

    #[test]
    fn corrupt_config_is_preserved_and_reported() {
        let app_data = tempdir().unwrap();
        let bundled = bundled_skill();
        let config = app_data.path().join("skill-paths.json");
        std::fs::write(&config, "not json").unwrap();
        let manager = SkillManager::open(
            app_data.path().to_path_buf(),
            bundled.path().to_path_buf(),
            None,
        );
        let state = manager.state().unwrap();
        assert!(state.configured);
        assert!(state.config_error.is_some());
        assert_eq!(std::fs::read_to_string(config).unwrap(), "not json");
    }

    #[test]
    fn saving_explicitly_replaces_a_corrupt_config() {
        let app_data = tempdir().unwrap();
        let bundled = bundled_skill();
        let config = app_data.path().join("skill-paths.json");
        std::fs::write(&config, "not json").unwrap();
        let manager = SkillManager::open(
            app_data.path().to_path_buf(),
            bundled.path().to_path_buf(),
            None,
        );

        let result = manager.save_paths(Vec::new(), false).unwrap();

        assert!(result.state.configured);
        assert!(result.state.config_error.is_none());
        let saved: SkillPathsConfig =
            serde_json::from_slice(&std::fs::read(config).unwrap()).unwrap();
        assert!(saved.paths.is_empty());
    }

    #[test]
    fn save_normalizes_deduplicates_and_synchronizes_paths() {
        let app_data = tempdir().unwrap();
        let bundled = bundled_skill();
        let parent = app_data.path().join("managed");
        let manager = SkillManager::open(
            app_data.path().to_path_buf(),
            bundled.path().to_path_buf(),
            None,
        );
        let result = manager
            .save_paths(
                vec![
                    parent.join(".").to_string_lossy().into_owned(),
                    parent.to_string_lossy().into_owned(),
                ],
                false,
            )
            .unwrap();
        assert_eq!(result.state.paths.len(), 1);
        assert_eq!(result.state.paths[0].parent_path, parent.to_string_lossy());
        assert!(parent.join("proxycrab/SKILL.md").is_file());
        let config: SkillPathsConfig = serde_json::from_slice(
            &std::fs::read(app_data.path().join("skill-paths.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(config.paths, vec![parent.to_string_lossy()]);
    }

    #[test]
    fn removed_paths_can_be_kept_or_deleted() {
        let app_data = tempdir().unwrap();
        let bundled = bundled_skill();
        let parent = app_data.path().join("managed");
        let manager = SkillManager::open(
            app_data.path().to_path_buf(),
            bundled.path().to_path_buf(),
            None,
        );
        manager
            .save_paths(vec![parent.to_string_lossy().into_owned()], false)
            .unwrap();
        manager.save_paths(Vec::new(), false).unwrap();
        assert!(parent.join("proxycrab").is_dir());
        manager
            .save_paths(vec![parent.to_string_lossy().into_owned()], false)
            .unwrap();
        manager.save_paths(Vec::new(), true).unwrap();
        assert!(!parent.join("proxycrab").exists());
    }

    #[test]
    fn removal_failures_are_returned_after_the_list_is_saved() {
        let app_data = tempdir().unwrap();
        let bundled = bundled_skill();
        let parent = app_data.path().join("managed");
        std::fs::create_dir_all(&parent).unwrap();
        std::fs::write(parent.join("proxycrab"), "not a directory").unwrap();
        let manager = SkillManager::open(
            app_data.path().to_path_buf(),
            bundled.path().to_path_buf(),
            None,
        );
        manager
            .save_paths(vec![parent.to_string_lossy().into_owned()], false)
            .unwrap();

        let result = manager.save_paths(Vec::new(), true).unwrap();

        assert!(result.state.paths.is_empty());
        assert_eq!(result.removal_errors.len(), 1);
        assert_eq!(
            result.removal_errors[0].parent_path,
            parent.to_string_lossy()
        );
        let saved: SkillPathsConfig = serde_json::from_slice(
            &std::fs::read(app_data.path().join("skill-paths.json")).unwrap(),
        )
        .unwrap();
        assert!(saved.paths.is_empty());
    }

    #[test]
    fn synchronization_failures_are_isolated_per_path() {
        let app_data = tempdir().unwrap();
        let bundled = bundled_skill();
        let good = app_data.path().join("good");
        let bad = app_data.path().join("bad");
        std::fs::create_dir_all(&bad).unwrap();
        std::fs::write(bad.join("proxycrab"), "not a directory").unwrap();
        let manager = SkillManager::open(
            app_data.path().to_path_buf(),
            bundled.path().to_path_buf(),
            None,
        );
        let result = manager
            .save_paths(
                vec![
                    bad.to_string_lossy().into_owned(),
                    good.to_string_lossy().into_owned(),
                ],
                false,
            )
            .unwrap();
        assert!(result.state.paths[0].error.is_some());
        assert!(result.state.paths[1].error.is_none());
        assert!(good.join("proxycrab/SKILL.md").is_file());
    }
}
