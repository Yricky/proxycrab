mod model;
mod store;

use std::{path::Path, sync::Arc};

use async_trait::async_trait;
use proxy_crab_mgr::{
    dto::{ErrorCode, ManagerError},
    permission::{
        ApiAction, ManagementCredential, PermissionAction, PermissionDenied, PermissionManager,
        PermissionMode, api_actions,
    },
};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

pub use model::{
    ApiActionView, CreatedApiKey, IdentityPermissions, PermissionEntry, PermissionIdentitySummary,
};
use store::PermissionStore;

pub struct UiAccess {
    digest: [u8; 32],
}

impl UiAccess {
    pub fn new(token: &str) -> Self {
        Self {
            digest: Sha256::digest(token.as_bytes()).into(),
        }
    }

    pub fn allows(&self, credential: &ManagementCredential) -> bool {
        let ManagementCredential::Bearer(token) = credential else {
            return false;
        };
        self.allows_token(token)
    }

    pub fn allows_authorization(&self, authorization: Option<&str>) -> bool {
        let Some(value) = authorization else {
            return false;
        };
        let mut parts = value.split_whitespace();
        let (Some(scheme), Some(token), None) = (parts.next(), parts.next(), parts.next()) else {
            return false;
        };
        scheme.eq_ignore_ascii_case("bearer") && self.allows_token(token)
    }

    fn allows_token(&self, token: &str) -> bool {
        let digest: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        self.digest.ct_eq(&digest).into()
    }
}

/// CLI variant of the desktop app's `HttpPermissionService`: API key
/// authentication and the on-disk permission table behave identically, but
/// actions requiring approval are denied immediately because this target does
/// not implement approval decisions.
pub struct CliPermissionService {
    store: PermissionStore,
    ui_access: Option<Arc<UiAccess>>,
}

impl CliPermissionService {
    pub fn open(
        workspace: &Path,
        ui_access: Option<Arc<UiAccess>>,
    ) -> Result<Arc<Self>, ManagerError> {
        Ok(Arc::new(Self {
            store: PermissionStore::open(workspace)?,
            ui_access,
        }))
    }

    pub fn catalog(&self) -> Vec<ApiActionView> {
        api_actions().iter().map(action_view).collect()
    }

    pub fn identities(&self) -> Result<Vec<PermissionIdentitySummary>, ManagerError> {
        self.store.identities()
    }

    pub fn identity_permissions(&self, id: &str) -> Result<IdentityPermissions, ManagerError> {
        self.store.identity_permissions(id)
    }

    pub fn replace_permissions(
        &self,
        id: &str,
        entries: Vec<PermissionEntry>,
    ) -> Result<IdentityPermissions, ManagerError> {
        self.store.replace_permissions(id, entries)
    }

    pub fn create_api_key(&self, name: String) -> Result<CreatedApiKey, ManagerError> {
        self.store.create_api_key(name)
    }

    pub fn delete_api_key(&self, id: &str) -> Result<(), ManagerError> {
        self.store.delete_api_key(id)
    }
}

#[async_trait]
impl PermissionManager for CliPermissionService {
    async fn check_permission(&self, action: PermissionAction) -> Option<PermissionDenied> {
        if self
            .ui_access
            .as_ref()
            .is_some_and(|access| access.allows(&action.credential))
        {
            return None;
        }
        let identity = match self.store.authenticate(&action.credential) {
            Ok(identity) => identity,
            Err(error) if error.code == ErrorCode::InvalidApiKey => {
                tracing::warn!(
                    action = action.action.id,
                    "management API credential rejected"
                );
                return Some(PermissionDenied::unauthorized(
                    error.code.as_str(),
                    error.message,
                ));
            }
            Err(error) => {
                tracing::error!(
                    action = action.action.id,
                    "permission check failed: {error}"
                );
                return Some(PermissionDenied::internal(
                    "permission_check_failed",
                    "management API permission check failed",
                ));
            }
        };
        match identity.permissions.get(action.action.id).copied() {
            Some(PermissionMode::Allow) => None,
            Some(PermissionMode::Deny) => {
                tracing::warn!(
                    identity_id = identity.summary.id,
                    action = action.action.id,
                    "management API action denied"
                );
                Some(PermissionDenied::forbidden(
                    "permission_denied",
                    "management API action was denied",
                ))
            }
            Some(PermissionMode::Approval) => {
                tracing::warn!(
                    identity_id = identity.summary.id,
                    action = action.action.id,
                    "management API approval mode is treated as deny by the CLI"
                );
                Some(PermissionDenied::forbidden(
                    "permission_denied",
                    "management API action was denied",
                ))
            }
            None => Some(PermissionDenied::internal(
                "permission_check_failed",
                "management API action is missing from the permission table",
            )),
        }
    }
}

fn action_view(action: &ApiAction) -> ApiActionView {
    ApiActionView {
        id: action.id.into(),
        method: action.method.into(),
        route_template: action.route_template.into(),
        default_mode: match action.default_mode {
            PermissionMode::Approval => PermissionMode::Deny,
            mode => mode,
        },
    }
}

#[cfg(test)]
mod tests {
    use proxy_crab_mgr::permission::find_api_action;
    use tempfile::tempdir;

    use super::*;

    fn action(method: &'static str, route: &str) -> PermissionAction {
        PermissionAction {
            action: find_api_action(method, route).unwrap(),
            credential: ManagementCredential::LocalLoopback,
            actual_path: route.into(),
            query: None,
            source: None,
            content_type: None,
            content_length: None,
            body_preview: None,
            body_preview_truncated: false,
        }
    }

    #[tokio::test]
    async fn default_local_identity_allows_allow_actions() {
        let directory = tempdir().unwrap();
        let service = CliPermissionService::open(directory.path(), None).unwrap();
        assert!(
            service
                .check_permission(action("GET", "/api/config"))
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn approval_actions_are_denied_in_headless_mode() {
        let directory = tempdir().unwrap();
        std::fs::write(
            directory.path().join("http_api_permissions.json"),
            r#"{"version":1,"local":{"permissions":{"POST /api/proxy/start":"approval"}},"api_keys":[]}"#,
        )
        .unwrap();
        let service = CliPermissionService::open(directory.path(), None).unwrap();
        let denied = service
            .check_permission(action("POST", "/api/proxy/start"))
            .await
            .unwrap();
        assert_eq!(denied.code, "permission_denied");
    }

    #[tokio::test]
    async fn ui_token_bypasses_the_cli_permission_table() {
        let directory = tempdir().unwrap();
        std::fs::write(
            directory.path().join("http_api_permissions.json"),
            r#"{"version":1,"local":{"permissions":{"POST /api/proxy/start":"deny"}},"api_keys":[]}"#,
        )
        .unwrap();
        let access = Arc::new(UiAccess::new("pcrab_ui_test"));
        let service = CliPermissionService::open(directory.path(), Some(access)).unwrap();
        let mut request = action("POST", "/api/proxy/start");
        request.credential = ManagementCredential::Bearer("pcrab_ui_test".into());
        assert!(service.check_permission(request).await.is_none());
    }
}
