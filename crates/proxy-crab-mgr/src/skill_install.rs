use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::dto::ManagerError;
use serde::Serialize;

const SKILL_DIRECTORY_NAME: &str = "proxycrab";
static INSTALL_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize)]
pub struct SkillInstallInfo {
    pub parent_path: String,
    pub target_path: String,
    pub exists: bool,
}

pub fn install(
    source: &Path,
    parent: &str,
    overwrite: bool,
) -> Result<SkillInstallInfo, ManagerError> {
    validate_existing_directory(source, "bundled skill source")?;
    if !source.is_dir() {
        return Err(ManagerError::internal(
            "bundled ProxyCrab skill directory is missing",
        ));
    }

    let parent = normalize_parent(parent)?;
    validate_existing_directory(&parent, "skill parent path")?;
    fs::create_dir_all(&parent)
        .map_err(|error| io_error("create skill parent directory", &parent, error))?;
    let target = parent.join(SKILL_DIRECTORY_NAME);
    validate_existing_directory(&target, "skill target path")?;
    let existed = path_exists(&target);
    if existed && !overwrite {
        return Err(ManagerError::conflict(format!(
            "ProxyCrab skill already exists at {}",
            target.display()
        )));
    }

    let suffix = unique_suffix();
    let temporary = parent.join(format!(".proxycrab.install-{suffix}"));
    let backup = parent.join(format!(".proxycrab.backup-{suffix}"));
    fs::create_dir(&temporary)
        .map_err(|error| io_error("create temporary skill directory", &temporary, error))?;
    if let Err(error) = copy_directory_contents(source, &temporary) {
        let _ = fs::remove_dir_all(&temporary);
        return Err(error);
    }

    if existed && let Err(error) = fs::rename(&target, &backup) {
        let _ = fs::remove_dir_all(&temporary);
        return Err(io_error(
            "move the previous ProxyCrab skill",
            &target,
            error,
        ));
    }

    if let Err(error) = fs::rename(&temporary, &target) {
        if existed {
            let _ = fs::rename(&backup, &target);
        }
        let _ = fs::remove_dir_all(&temporary);
        return Err(io_error("activate the new ProxyCrab skill", &target, error));
    }

    if existed && let Err(error) = fs::remove_dir_all(&backup) {
        tracing::warn!(
            "installed ProxyCrab skill but failed to remove backup {}: {error}",
            backup.display()
        );
    }

    Ok(SkillInstallInfo {
        parent_path: parent.to_string_lossy().into_owned(),
        target_path: target.to_string_lossy().into_owned(),
        exists: true,
    })
}

pub fn normalize_parent(value: &str) -> Result<PathBuf, ManagerError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(ManagerError::bad_request(
            "skill installation parent path cannot be empty",
        ));
    }
    let expanded = if value == "~" {
        home_directory()?
    } else if let Some(relative) = value.strip_prefix("~/") {
        home_directory()?.join(relative)
    } else {
        PathBuf::from(value)
    };
    if !expanded.is_absolute() {
        return Err(ManagerError::bad_request(
            "skill installation parent path must be absolute or start with ~/",
        ));
    }
    let mut normalized = PathBuf::new();
    for component in expanded.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    Ok(normalized)
}

/// Removes the fixed ProxyCrab Skill child without touching any sibling entry.
/// Returns whether an installed target existed.
pub fn uninstall(parent: &str) -> Result<bool, ManagerError> {
    let parent = normalize_parent(parent)?;
    validate_existing_directory(&parent, "skill parent path")?;
    let target = parent.join(SKILL_DIRECTORY_NAME);
    validate_existing_directory(&target, "skill target path")?;
    if !path_exists(&target) {
        return Ok(false);
    }
    fs::remove_dir_all(&target)
        .map_err(|error| io_error("remove ProxyCrab skill", &target, error))?;
    Ok(true)
}

fn home_directory() -> Result<PathBuf, ManagerError> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| ManagerError::internal("HOME is unavailable"))
}

fn validate_existing_directory(path: &Path, label: &str) -> Result<(), ManagerError> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(ManagerError::bad_request(format!(
            "{label} is not a directory: {}",
            path.display()
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(&format!("inspect {label}"), path, error)),
    }
}

fn path_exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn copy_directory_contents(source: &Path, target: &Path) -> Result<(), ManagerError> {
    let entries = fs::read_dir(source)
        .map_err(|error| io_error("read bundled skill directory", source, error))?;
    for entry in entries {
        let entry = entry.map_err(|error| io_error("read bundled skill entry", source, error))?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|error| io_error("inspect bundled skill entry", &source_path, error))?;
        if file_type.is_symlink() {
            return Err(ManagerError::internal(format!(
                "bundled skill contains an unsupported symlink: {}",
                source_path.display()
            )));
        }
        if file_type.is_dir() {
            fs::create_dir(&target_path)
                .map_err(|error| io_error("create skill directory", &target_path, error))?;
            copy_directory_contents(&source_path, &target_path)?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &target_path)
                .map_err(|error| io_error("copy bundled skill file", &source_path, error))?;
        }
    }
    Ok(())
}

fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = INSTALL_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{}-{nanos}-{sequence}", std::process::id())
}

fn io_error(action: &str, path: &Path, error: std::io::Error) -> ManagerError {
    ManagerError::internal(format!("{action} {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{install, normalize_parent, uninstall};

    #[test]
    fn installs_and_completely_overwrites_the_target_directory() {
        let source = tempdir().unwrap();
        std::fs::write(source.path().join("SKILL.md"), "first").unwrap();
        std::fs::create_dir(source.path().join("scripts")).unwrap();
        std::fs::write(source.path().join("scripts/tool.mjs"), "first").unwrap();
        let parent = tempdir().unwrap();
        let parent_path = parent.path().to_string_lossy();

        let installed = install(source.path(), &parent_path, false).unwrap();
        let target = parent.path().join("proxycrab");
        assert_eq!(installed.target_path, target.to_string_lossy());
        assert_eq!(
            std::fs::read_to_string(target.join("scripts/tool.mjs")).unwrap(),
            "first"
        );

        std::fs::write(target.join("user-extra.txt"), "remove me").unwrap();
        std::fs::write(source.path().join("SKILL.md"), "second").unwrap();
        assert_eq!(
            install(source.path(), &parent_path, false)
                .unwrap_err()
                .code,
            "conflict"
        );
        install(source.path(), &parent_path, true).unwrap();

        assert_eq!(
            std::fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "second"
        );
        assert!(!target.join("user-extra.txt").exists());
    }

    #[test]
    fn reports_the_fixed_proxycrab_target() {
        let source = tempdir().unwrap();
        std::fs::write(source.path().join("SKILL.md"), "skill").unwrap();
        let parent = tempdir().unwrap();
        let info = install(source.path(), &parent.path().to_string_lossy(), false).unwrap();
        assert_eq!(
            info.target_path,
            parent.path().join("proxycrab").to_string_lossy()
        );
        assert!(info.exists);
    }

    #[test]
    fn rejects_a_file_as_the_parent_or_target() {
        let source = tempdir().unwrap();
        std::fs::write(source.path().join("SKILL.md"), "skill").unwrap();
        let parent = tempdir().unwrap();
        let file = parent.path().join("not-a-directory");
        std::fs::write(&file, "file").unwrap();
        assert_eq!(
            install(source.path(), &file.to_string_lossy(), false)
                .unwrap_err()
                .code,
            "bad_request"
        );

        let target = parent.path().join("proxycrab");
        std::fs::write(&target, "file").unwrap();
        assert_eq!(
            install(source.path(), &parent.path().to_string_lossy(), false)
                .unwrap_err()
                .code,
            "bad_request"
        );
    }

    #[test]
    fn normalizes_absolute_parent_paths() {
        let parent = tempdir().unwrap();
        assert_eq!(
            normalize_parent(&parent.path().to_string_lossy()).unwrap(),
            parent.path()
        );
    }

    #[test]
    fn uninstall_removes_only_the_proxycrab_child() {
        let parent = tempdir().unwrap();
        let target = parent.path().join("proxycrab");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("SKILL.md"), "skill").unwrap();
        std::fs::write(parent.path().join("keep.txt"), "keep").unwrap();

        assert!(uninstall(&parent.path().to_string_lossy()).unwrap());
        assert!(!target.exists());
        assert!(parent.path().join("keep.txt").exists());
        assert!(!uninstall(&parent.path().to_string_lossy()).unwrap());
    }
}
