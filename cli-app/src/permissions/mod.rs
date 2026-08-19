mod model;
mod store;

use std::{path::Path, sync::Arc};

use async_trait::async_trait;
use proxy_crab_mgr::{
    dto::ManagerError,
    permission::{PermissionAction, PermissionDenied, PermissionManager, PermissionMode},
};

use store::PermissionStore;

/// Headless variant of the desktop app's `HttpPermissionService`: API key
/// authentication and the on-disk permission table behave identically, but
/// actions requiring approval are denied immediately because there is no UI
/// to approve them.
pub struct CliPermissionService {
    store: PermissionStore,
}

impl CliPermissionService {
    pub fn open(workspace: &Path) -> Result<Arc<Self>, ManagerError> {
        Ok(Arc::new(Self {
            store: PermissionStore::open(workspace)?,
        }))
    }
}

#[async_trait]
impl PermissionManager for CliPermissionService {
    async fn check_permission(&self, action: PermissionAction) -> Option<PermissionDenied> {
        let identity = match self.store.authenticate(action.authorization.as_deref()) {
            Ok(identity) => identity,
            Err(error) if error.code == "invalid_api_key" => {
                tracing::warn!(
                    action = action.action.id,
                    "management API credential rejected"
                );
                return Some(PermissionDenied::unauthorized(error.code, error.message));
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
                    "management API action requires approval, denied in headless mode"
                );
                Some(PermissionDenied::forbidden(
                    "approval_unavailable",
                    "management API action requires approval, which is unavailable in headless mode",
                ))
            }
            None => Some(PermissionDenied::internal(
                "permission_check_failed",
                "management API action is missing from the permission table",
            )),
        }
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
            authorization: None,
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
        let service = CliPermissionService::open(directory.path()).unwrap();
        assert!(
            service
                .check_permission(action("GET", "/api/workspace"))
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
        let service = CliPermissionService::open(directory.path()).unwrap();
        let denied = service
            .check_permission(action("POST", "/api/proxy/start"))
            .await
            .unwrap();
        assert_eq!(denied.code, "approval_unavailable");
    }
}
