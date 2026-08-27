use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use proxy_crab_mgr::{
    dto::ManagerError,
    permission::{ManagementCredential, OBSOLETE_API_ACTION_IDS, PermissionMode, api_actions},
};
use proxy_crab_mitm::workspace::now_millis;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use super::model::{
    ApiKeyRecord, CreatedApiKey, IdentityPermissions, LOCAL_IDENTITY_ID, LocalPermissionRecord,
    PERMISSION_FILE_VERSION, PermissionEntry, PermissionFile, PermissionIdentityKind,
    PermissionIdentitySummary,
};

const PERMISSION_FILE_NAME: &str = "http_api_permissions.json";
const LAST_USED_WRITE_INTERVAL_MS: u64 = 60_000;

pub struct PermissionStore {
    path: PathBuf,
    state: Mutex<PermissionFile>,
}

#[derive(Clone)]
pub struct AuthenticatedIdentity {
    pub summary: PermissionIdentitySummary,
    pub permissions: BTreeMap<String, PermissionMode>,
}

impl PermissionStore {
    pub fn open(workspace: &Path) -> Result<Self, ManagerError> {
        let path = workspace.join(PERMISSION_FILE_NAME);
        let (mut state, existed) = if path.exists() {
            let bytes = fs::read(&path).map_err(|error| permission_file_error(&path, error))?;
            let state = serde_json::from_slice(&bytes).map_err(|error| {
                ManagerError::internal(format!(
                    "failed to parse permission file {}: {error}",
                    path.display()
                ))
            })?;
            (state, true)
        } else {
            (PermissionFile::default(), false)
        };
        let normalized = validate_and_normalize(&mut state)?;
        let store = Self {
            path,
            state: Mutex::new(state),
        };
        if !existed || normalized {
            let state = store.state.lock().map_err(lock_error)?;
            store.save_locked(&state)?;
        }
        Ok(store)
    }

    pub fn authenticate(
        &self,
        credential: &ManagementCredential,
    ) -> Result<AuthenticatedIdentity, ManagerError> {
        let ManagementCredential::Bearer(api_key) = credential else {
            let state = self.state.lock().map_err(lock_error)?;
            return Ok(AuthenticatedIdentity {
                summary: local_summary(),
                permissions: state.local.permissions.clone(),
            });
        };
        let prefix = api_key
            .strip_prefix("pcrab_")
            .and_then(|value| value.split_once('_'))
            .map(|(prefix, _)| prefix)
            .ok_or_else(invalid_api_key)?;
        let mut state = self.state.lock().map_err(lock_error)?;
        let record = state
            .api_keys
            .iter_mut()
            .find(|record| record.prefix == prefix)
            .ok_or_else(invalid_api_key)?;
        let salt = URL_SAFE_NO_PAD
            .decode(&record.salt)
            .map_err(|_| ManagerError::internal("stored API key salt is invalid"))?;
        let expected = URL_SAFE_NO_PAD
            .decode(&record.hash)
            .map_err(|_| ManagerError::internal("stored API key hash is invalid"))?;
        let actual = key_hash(&salt, api_key);
        if expected.len() != actual.len() || expected.ct_eq(actual.as_slice()).unwrap_u8() != 1 {
            return Err(invalid_api_key());
        }
        let now = now_millis();
        let should_save = record
            .last_used_at
            .is_none_or(|last| now.saturating_sub(last) >= LAST_USED_WRITE_INTERVAL_MS);
        let identity = AuthenticatedIdentity {
            summary: api_key_summary(record),
            permissions: record.permissions.clone(),
        };
        if should_save {
            let mut next = state.clone();
            if let Some(record) = next
                .api_keys
                .iter_mut()
                .find(|candidate| candidate.id == identity.summary.id)
            {
                record.last_used_at = Some(now);
            }
            match self.save_locked(&next) {
                Ok(()) => *state = next,
                Err(error) => {
                    tracing::error!("failed to persist API key last-used time: {error}");
                }
            }
        }
        Ok(identity)
    }

    pub fn identities(&self) -> Result<Vec<PermissionIdentitySummary>, ManagerError> {
        let state = self.state.lock().map_err(lock_error)?;
        let mut identities = Vec::with_capacity(state.api_keys.len() + 1);
        identities.push(local_summary());
        identities.extend(state.api_keys.iter().map(api_key_summary));
        Ok(identities)
    }

    pub fn identity_permissions(&self, id: &str) -> Result<IdentityPermissions, ManagerError> {
        let state = self.state.lock().map_err(lock_error)?;
        let (identity, permissions) = identity_record(&state, id)?;
        Ok(IdentityPermissions {
            identity,
            permissions: permission_entries(permissions),
        })
    }

    pub fn replace_permissions(
        &self,
        id: &str,
        entries: Vec<PermissionEntry>,
    ) -> Result<IdentityPermissions, ManagerError> {
        let permissions = validate_complete_permissions(entries)?;
        let mut state = self.state.lock().map_err(lock_error)?;
        let mut next = state.clone();
        if id == LOCAL_IDENTITY_ID {
            next.local.permissions = permissions;
        } else {
            let record = next
                .api_keys
                .iter_mut()
                .find(|record| record.id == id)
                .ok_or_else(|| ManagerError::not_found("API key not found"))?;
            record.permissions = permissions;
        }
        self.save_locked(&next)?;
        *state = next;
        let (identity, permissions) = identity_record(&state, id)?;
        Ok(IdentityPermissions {
            identity,
            permissions: permission_entries(permissions),
        })
    }

    pub fn create_api_key(&self, name: String) -> Result<CreatedApiKey, ManagerError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 64 {
            return Err(ManagerError::bad_request(
                "API key name must contain 1 to 64 characters",
            ));
        }
        let mut state = self.state.lock().map_err(lock_error)?;
        if state.api_keys.iter().any(|record| record.name == name) {
            return Err(ManagerError::conflict("API key name already exists"));
        }
        let id = random_text(12)?;
        let prefix = random_text(6)?;
        let secret = random_text(32)?;
        let api_key = format!("pcrab_{prefix}_{secret}");
        let salt = random_bytes(16)?;
        let hash = key_hash(&salt, &api_key);
        let record = ApiKeyRecord {
            id,
            name: name.to_owned(),
            prefix,
            salt: URL_SAFE_NO_PAD.encode(salt),
            hash: URL_SAFE_NO_PAD.encode(hash),
            created_at: now_millis(),
            last_used_at: None,
            permissions: cli_default_permissions(),
        };
        let identity = api_key_summary(&record);
        let mut next = state.clone();
        next.api_keys.push(record);
        self.save_locked(&next)?;
        *state = next;
        Ok(CreatedApiKey { identity, api_key })
    }

    pub fn delete_api_key(&self, id: &str) -> Result<(), ManagerError> {
        let mut state = self.state.lock().map_err(lock_error)?;
        let index = state
            .api_keys
            .iter()
            .position(|record| record.id == id)
            .ok_or_else(|| ManagerError::not_found("API key not found"))?;
        let mut next = state.clone();
        next.api_keys.remove(index);
        self.save_locked(&next)?;
        *state = next;
        Ok(())
    }

    fn save_locked(&self, state: &PermissionFile) -> Result<(), ManagerError> {
        write_permission_file(&self.path, state)
    }
}

impl Default for PermissionFile {
    fn default() -> Self {
        Self {
            version: PERMISSION_FILE_VERSION,
            local: LocalPermissionRecord {
                permissions: api_actions()
                    .iter()
                    .map(|action| (action.id.to_owned(), PermissionMode::Allow))
                    .collect(),
            },
            api_keys: Vec::new(),
        }
    }
}

fn validate_and_normalize(state: &mut PermissionFile) -> Result<bool, ManagerError> {
    if state.version != PERMISSION_FILE_VERSION {
        return Err(ManagerError::internal(format!(
            "unsupported permission file version {}",
            state.version
        )));
    }
    let known = known_action_ids();
    let mut changed = remove_obsolete_permissions(&mut state.local.permissions);
    validate_permission_keys(&state.local.permissions, &known)?;
    changed |= add_missing_defaults(&mut state.local.permissions);
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut prefixes = BTreeSet::new();
    for record in &mut state.api_keys {
        if record.id.is_empty() || !ids.insert(record.id.clone()) {
            return Err(ManagerError::internal(
                "permission file contains duplicate API key IDs",
            ));
        }
        if record.name.trim().is_empty()
            || record.name != record.name.trim()
            || record.name.chars().count() > 64
            || !names.insert(record.name.clone())
        {
            return Err(ManagerError::internal(
                "permission file contains invalid API key names",
            ));
        }
        if record.prefix.is_empty() || !prefixes.insert(record.prefix.clone()) {
            return Err(ManagerError::internal(
                "permission file contains duplicate API key prefixes",
            ));
        }
        let salt = URL_SAFE_NO_PAD
            .decode(&record.salt)
            .map_err(|_| ManagerError::internal("permission file contains an invalid salt"))?;
        let hash = URL_SAFE_NO_PAD
            .decode(&record.hash)
            .map_err(|_| ManagerError::internal("permission file contains an invalid hash"))?;
        if salt.len() != 16 || hash.len() != 32 {
            return Err(ManagerError::internal(
                "permission file contains invalid API key credentials",
            ));
        }
        changed |= remove_obsolete_permissions(&mut record.permissions);
        validate_permission_keys(&record.permissions, &known)?;
        changed |= add_missing_defaults(&mut record.permissions);
    }
    Ok(changed)
}

fn remove_obsolete_permissions(permissions: &mut BTreeMap<String, PermissionMode>) -> bool {
    let before = permissions.len();
    for id in OBSOLETE_API_ACTION_IDS {
        permissions.remove(*id);
    }
    permissions.len() != before
}

fn validate_permission_keys(
    permissions: &BTreeMap<String, PermissionMode>,
    known: &BTreeSet<String>,
) -> Result<(), ManagerError> {
    if let Some(unknown) = permissions.keys().find(|id| !known.contains(*id)) {
        return Err(ManagerError::internal(format!(
            "permission file contains unknown action {unknown}"
        )));
    }
    Ok(())
}

fn add_missing_defaults(permissions: &mut BTreeMap<String, PermissionMode>) -> bool {
    let before = permissions.len();
    for action in api_actions() {
        permissions
            .entry(action.id.to_owned())
            .or_insert(action.default_mode);
    }
    permissions.len() != before
}

fn validate_complete_permissions(
    entries: Vec<PermissionEntry>,
) -> Result<BTreeMap<String, PermissionMode>, ManagerError> {
    let known = known_action_ids();
    let mut permissions = BTreeMap::new();
    for entry in entries {
        if entry.mode == PermissionMode::Approval {
            return Err(ManagerError::bad_request(
                "CLI permissions only support allow and deny",
            ));
        }
        if !known.contains(&entry.action_id)
            || permissions.insert(entry.action_id, entry.mode).is_some()
        {
            return Err(ManagerError::bad_request(
                "permission table contains unknown or duplicate actions",
            ));
        }
    }
    if permissions.len() != known.len() {
        return Err(ManagerError::bad_request(
            "permission table must contain every API action",
        ));
    }
    Ok(permissions)
}

fn identity_record<'a>(
    state: &'a PermissionFile,
    id: &str,
) -> Result<
    (
        PermissionIdentitySummary,
        &'a BTreeMap<String, PermissionMode>,
    ),
    ManagerError,
> {
    if id == LOCAL_IDENTITY_ID {
        return Ok((local_summary(), &state.local.permissions));
    }
    let record = state
        .api_keys
        .iter()
        .find(|record| record.id == id)
        .ok_or_else(|| ManagerError::not_found("API key not found"))?;
    Ok((api_key_summary(record), &record.permissions))
}

fn permission_entries(permissions: &BTreeMap<String, PermissionMode>) -> Vec<PermissionEntry> {
    api_actions()
        .iter()
        .map(|action| PermissionEntry {
            action_id: action.id.to_owned(),
            mode: match permissions[action.id] {
                PermissionMode::Approval => PermissionMode::Deny,
                mode => mode,
            },
        })
        .collect()
}

fn cli_default_permissions() -> BTreeMap<String, PermissionMode> {
    api_actions()
        .iter()
        .map(|action| {
            let mode = match action.default_mode {
                PermissionMode::Approval => PermissionMode::Deny,
                mode => mode,
            };
            (action.id.to_owned(), mode)
        })
        .collect()
}

fn local_summary() -> PermissionIdentitySummary {
    PermissionIdentitySummary {
        id: LOCAL_IDENTITY_ID.into(),
        kind: PermissionIdentityKind::Local,
        name: "本机无 API Key".into(),
        prefix: None,
        created_at: None,
        last_used_at: None,
    }
}

fn api_key_summary(record: &ApiKeyRecord) -> PermissionIdentitySummary {
    PermissionIdentitySummary {
        id: record.id.clone(),
        kind: PermissionIdentityKind::ApiKey,
        name: record.name.clone(),
        prefix: Some(record.prefix.clone()),
        created_at: Some(record.created_at),
        last_used_at: record.last_used_at,
    }
}

fn known_action_ids() -> BTreeSet<String> {
    api_actions()
        .iter()
        .map(|action| action.id.to_owned())
        .collect()
}

fn invalid_api_key() -> ManagerError {
    ManagerError::new(
        "invalid_api_key",
        "Authorization must contain a valid Bearer API key",
    )
}

fn random_text(bytes: usize) -> Result<String, ManagerError> {
    Ok(URL_SAFE_NO_PAD.encode(random_bytes(bytes)?))
}

fn random_bytes(length: usize) -> Result<Vec<u8>, ManagerError> {
    let mut bytes = vec![0; length];
    getrandom::fill(&mut bytes).map_err(|error| {
        ManagerError::internal(format!("secure random generation failed: {error}"))
    })?;
    Ok(bytes)
}

fn key_hash(salt: &[u8], key: &str) -> Vec<u8> {
    let mut digest = Sha256::new();
    digest.update(salt);
    digest.update(key.as_bytes());
    digest.finalize().to_vec()
}

fn write_permission_file(path: &Path, state: &PermissionFile) -> Result<(), ManagerError> {
    let parent = path
        .parent()
        .ok_or_else(|| ManagerError::internal("permission file has no parent directory"))?;
    fs::create_dir_all(parent).map_err(|error| permission_file_error(path, error))?;
    let temporary = parent.join(format!(".{PERMISSION_FILE_NAME}.{}.tmp", random_text(8)?));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|error| permission_file_error(&temporary, error))?;
        serde_json::to_writer_pretty(&mut file, state).map_err(|error| {
            ManagerError::internal(format!("serialize permission file: {error}"))
        })?;
        file.write_all(b"\n")
            .map_err(|error| permission_file_error(&temporary, error))?;
        file.sync_all()
            .map_err(|error| permission_file_error(&temporary, error))?;
        fs::rename(&temporary, path).map_err(|error| permission_file_error(path, error))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))
                .map_err(|error| permission_file_error(path, error))?;
            fs::File::open(parent)
                .and_then(|directory| directory.sync_all())
                .map_err(|error| permission_file_error(parent, error))?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn permission_file_error(path: &Path, error: std::io::Error) -> ManagerError {
    ManagerError::internal(format!(
        "permission file {} could not be read or written: {error}",
        path.display()
    ))
}

fn lock_error<T>(_: std::sync::PoisonError<T>) -> ManagerError {
    ManagerError::internal("permission state lock poisoned")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn missing_file_creates_local_allow_all() {
        let directory = tempdir().unwrap();
        let store = PermissionStore::open(directory.path()).unwrap();
        let identity = store
            .authenticate(&ManagementCredential::LocalLoopback)
            .unwrap();
        assert_eq!(identity.summary.id, LOCAL_IDENTITY_ID);
        assert!(
            identity
                .permissions
                .values()
                .all(|mode| *mode == PermissionMode::Allow)
        );
    }

    #[test]
    fn corrupt_file_fails_without_replacing_it() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(PERMISSION_FILE_NAME);
        fs::write(&path, b"not json").unwrap();
        let error = PermissionStore::open(directory.path()).err().unwrap();
        assert!(error.message.contains("failed to parse permission file"));
        assert_eq!(fs::read(&path).unwrap(), b"not json");
    }

    #[test]
    fn cli_permission_views_and_updates_never_expose_approval() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(PERMISSION_FILE_NAME);
        fs::write(
            &path,
            r#"{"version":1,"local":{"permissions":{"POST /api/proxy/start":"approval"}},"api_keys":[]}"#,
        )
        .unwrap();
        let store = PermissionStore::open(directory.path()).unwrap();

        let mut view = store.identity_permissions(LOCAL_IDENTITY_ID).unwrap();
        assert!(
            view.permissions
                .iter()
                .all(|entry| entry.mode != PermissionMode::Approval)
        );
        let start = view
            .permissions
            .iter()
            .find(|entry| entry.action_id == "POST /api/proxy/start")
            .unwrap();
        assert_eq!(start.mode, PermissionMode::Deny);

        view.permissions[0].mode = PermissionMode::Approval;
        let error = store
            .replace_permissions(LOCAL_IDENTITY_ID, view.permissions)
            .unwrap_err();
        assert_eq!(error.code, "bad_request");
    }

    #[test]
    fn new_cli_api_keys_default_approval_actions_to_deny() {
        let directory = tempdir().unwrap();
        let store = PermissionStore::open(directory.path()).unwrap();
        let created = store.create_api_key("browser test".into()).unwrap();
        let view = store.identity_permissions(&created.identity.id).unwrap();

        assert!(
            view.permissions
                .iter()
                .all(|entry| entry.mode != PermissionMode::Approval)
        );
        for action in api_actions() {
            let entry = view
                .permissions
                .iter()
                .find(|entry| entry.action_id == action.id)
                .unwrap();
            let expected = match action.default_mode {
                PermissionMode::Approval => PermissionMode::Deny,
                mode => mode,
            };
            assert_eq!(entry.mode, expected);
        }
    }

    #[test]
    fn existing_permission_file_migrates_removed_and_added_actions() {
        let directory = tempdir().unwrap();
        let store = PermissionStore::open(directory.path()).unwrap();
        let created = store.create_api_key("agent".into()).unwrap();
        drop(store);

        let path = directory.path().join(PERMISSION_FILE_NAME);
        let mut state: PermissionFile = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        for permissions in std::iter::once(&mut state.local.permissions).chain(
            state
                .api_keys
                .iter_mut()
                .map(|record| &mut record.permissions),
        ) {
            permissions.remove("GET /api/session-shares/{id}");
            permissions.remove("POST /api/session-shares");
            permissions.remove("DELETE /api/session-shares/{id}");
            for action_id in OBSOLETE_API_ACTION_IDS {
                permissions.insert((*action_id).to_owned(), PermissionMode::Deny);
            }
        }
        fs::write(&path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();

        let store = PermissionStore::open(directory.path()).unwrap();
        for identity_id in [LOCAL_IDENTITY_ID, created.identity.id.as_str()] {
            let modes = store
                .identity_permissions(identity_id)
                .unwrap()
                .permissions
                .into_iter()
                .map(|entry| (entry.action_id, entry.mode))
                .collect::<BTreeMap<_, _>>();
            assert_eq!(modes["GET /api/session-shares/{id}"], PermissionMode::Allow);
            assert_eq!(modes["POST /api/session-shares"], PermissionMode::Deny);
            assert_eq!(
                modes["DELETE /api/session-shares/{id}"],
                PermissionMode::Deny
            );
            for action_id in OBSOLETE_API_ACTION_IDS {
                assert!(!modes.contains_key(*action_id));
            }
        }
    }
}
