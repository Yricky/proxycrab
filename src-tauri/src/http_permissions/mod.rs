mod approval;
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

use approval::{ApprovalOutcome, ApprovalQueue, outcome_denial};
pub use model::{
    ApiActionView, CreatedApiKey, IdentityPermissions, PendingApproval, PermissionEntry,
    PermissionIdentitySummary, ResolveApprovalRequest,
};
use store::PermissionStore;

pub struct HttpPermissionService {
    store: PermissionStore,
    approvals: ApprovalQueue,
}

impl HttpPermissionService {
    pub fn open(
        workspace: &Path,
        notify: Arc<dyn Fn(usize) + Send + Sync>,
    ) -> Result<Arc<Self>, ManagerError> {
        Ok(Arc::new(Self {
            store: PermissionStore::open(workspace)?,
            approvals: ApprovalQueue::new(notify),
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

    pub fn set_redaction(
        &self,
        id: &str,
        script: Option<String>,
    ) -> Result<IdentityPermissions, ManagerError> {
        self.store.set_redaction(id, script)
    }

    pub fn replace_permissions(
        &self,
        id: &str,
        entries: Vec<PermissionEntry>,
    ) -> Result<IdentityPermissions, ManagerError> {
        let value = self.store.replace_permissions(id, entries)?;
        tracing::info!(identity_id = id, "management API permissions saved");
        Ok(value)
    }

    pub fn create_api_key(&self, name: String) -> Result<CreatedApiKey, ManagerError> {
        let value = self.store.create_api_key(name)?;
        tracing::info!(
            identity_id = value.identity.id,
            "management API key created"
        );
        Ok(value)
    }

    pub fn delete_api_key(&self, id: &str) -> Result<(), ManagerError> {
        self.store.delete_api_key(id)?;
        tracing::info!(identity_id = id, "management API key deleted");
        Ok(())
    }

    pub fn approvals(&self) -> Result<Vec<PendingApproval>, ManagerError> {
        self.approvals.list()
    }

    pub fn resolve_approval(
        &self,
        id: u64,
        request: ResolveApprovalRequest,
    ) -> Result<(), ManagerError> {
        self.approvals.resolve(id, request)
    }
}

#[async_trait]
impl PermissionManager for HttpPermissionService {
    async fn redaction_script(
        &self,
        credential: &ManagementCredential,
    ) -> Result<Option<String>, PermissionDenied> {
        let identity = self
            .store
            .authenticate(credential)
            .map_err(|_| PermissionDenied::unauthorized("invalid_api_key", "invalid API key"))?;
        self.store
            .identity_permissions(&identity.summary.id)
            .map(|view| view.redaction_script)
            .map_err(|_| {
                PermissionDenied::internal(
                    "permission_check_failed",
                    "cannot load redaction binding",
                )
            })
    }
    async fn check_permission(&self, action: PermissionAction) -> Option<PermissionDenied> {
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
        let temporary = match self
            .approvals
            .temporary_outcome(&identity.key, action.action.id)
        {
            Ok(value) => value,
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
        if let Some(outcome) = temporary {
            if outcome == ApprovalOutcome::Deny {
                tracing::warn!(
                    identity_id = identity.summary.id,
                    action = action.action.id,
                    "management API action denied by temporary decision"
                );
            }
            return outcome_denial(outcome);
        }
        match identity.permissions.get(action.action.id).copied() {
            Some(PermissionMode::Allow) => None,
            Some(PermissionMode::Deny) => {
                tracing::warn!(
                    identity_id = identity.summary.id,
                    action = action.action.id,
                    "management API action denied"
                );
                outcome_denial(ApprovalOutcome::Deny)
            }
            Some(PermissionMode::Approval) => {
                match self
                    .approvals
                    .wait(identity.key, identity.summary, &action)
                    .await
                {
                    Ok(outcome) => outcome_denial(outcome),
                    Err(error) => {
                        tracing::error!(
                            action = action.action.id,
                            "approval check failed: {error}"
                        );
                        Some(PermissionDenied::internal(
                            "permission_check_failed",
                            "management API permission check failed",
                        ))
                    }
                }
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
        default_mode: action.default_mode,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::tempdir;

    use super::HttpPermissionService;

    #[tokio::test]
    async fn redaction_binding_resolves_per_authenticated_identity() {
        use proxy_crab_mgr::permission::{ManagementCredential, PermissionManager};
        let directory = tempdir().unwrap();
        let service = HttpPermissionService::open(directory.path(), Arc::new(|_| {})).unwrap();
        let key = service.create_api_key("bound".into()).unwrap();
        service
            .set_redaction("local", Some("local-mask".into()))
            .unwrap();
        service
            .set_redaction(&key.identity.id, Some("token-mask".into()))
            .unwrap();
        assert_eq!(
            service
                .redaction_script(&ManagementCredential::LocalLoopback)
                .await
                .unwrap()
                .as_deref(),
            Some("local-mask")
        );
        assert_eq!(
            service
                .redaction_script(&ManagementCredential::Bearer(key.api_key))
                .await
                .unwrap()
                .as_deref(),
            Some("token-mask")
        );
    }

    #[test]
    fn desktop_catalog_exposes_asset_and_share_management_actions() {
        let directory = tempdir().unwrap();
        let service = HttpPermissionService::open(directory.path(), Arc::new(|_| {})).unwrap();
        let catalog = service.catalog();

        assert_eq!(catalog.len(), 81);
        assert!(
            catalog
                .iter()
                .any(|action| action.id == "GET /api/session-shares")
        );
        assert!(catalog.iter().any(|action| {
            action.id == "GET /api/assets" && action.route_template == "/api/assets"
        }));
        assert!(catalog.iter().any(|action| {
            action.id == "GET /api/clash-config" && action.route_template == "/api/clash-config"
        }));
        assert!(catalog.iter().any(|action| {
            action.id == "POST /api/replay" && action.route_template == "/api/replay"
        }));
        assert!(catalog.iter().any(|action| {
            action.id == "GET /api/assets/{*asset_id}"
                && action.route_template == "/api/assets/{*asset_id}"
        }));
        assert!(catalog.iter().any(|action| {
            action.id == "POST /api/assets/{*asset_id}"
                && action.route_template == "/api/assets/{*asset_id}"
        }));
        assert!(
            !catalog
                .iter()
                .any(|action| action.route_template.starts_with("/api/session-har-shares"))
        );
        assert!(catalog.iter().any(|action| {
            action.id == "POST /api/session-shares"
                && action.route_template == "/api/session-shares"
        }));
        assert!(catalog.iter().any(|action| {
            action.id == "GET /api/session-shares/{id}"
                && action.route_template == "/api/session-shares/{id}"
        }));
        assert!(catalog.iter().any(|action| {
            action.id == "DELETE /api/session-shares/{id}"
                && action.route_template == "/api/session-shares/{id}"
        }));
    }
}
