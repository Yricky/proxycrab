use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use proxy_crab_mgr::{
    dto::ManagerError,
    permission::{PermissionAction, PermissionDenied},
};
use proxy_crab_mitm::workspace::now_millis;
use tokio::sync::oneshot;

use super::model::{
    ApprovalDecision, IdentityKey, PendingApproval, PermissionIdentitySummary,
    ResolveApprovalRequest,
};

const APPROVAL_TIMEOUT: Duration = Duration::from_secs(30);
const ALLOWED_DURATIONS: [u64; 3] = [300, 1_800, 3_600];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalOutcome {
    Allow,
    Deny,
    Timeout,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct DecisionKey {
    identity: IdentityKey,
    action_id: String,
}

struct TemporaryDecision {
    outcome: ApprovalOutcome,
    expires_at: u64,
}

struct PendingEntry {
    summary: PendingApproval,
    key: DecisionKey,
    sender: oneshot::Sender<ApprovalOutcome>,
}

struct ApprovalState {
    next_id: u64,
    pending: HashMap<u64, PendingEntry>,
    temporary: HashMap<DecisionKey, TemporaryDecision>,
}

type ChangeNotifier = Arc<dyn Fn(usize) + Send + Sync>;

pub struct ApprovalQueue {
    state: Arc<Mutex<ApprovalState>>,
    notify: ChangeNotifier,
}

impl ApprovalQueue {
    pub fn new(notify: ChangeNotifier) -> Self {
        Self {
            state: Arc::new(Mutex::new(ApprovalState {
                next_id: 1,
                pending: HashMap::new(),
                temporary: HashMap::new(),
            })),
            notify,
        }
    }

    pub fn temporary_outcome(
        &self,
        identity: &IdentityKey,
        action_id: &str,
    ) -> Result<Option<ApprovalOutcome>, ManagerError> {
        let now = now_millis();
        let mut state = self.state.lock().map_err(lock_error)?;
        state.temporary.retain(|_, value| value.expires_at > now);
        Ok(state
            .temporary
            .get(&DecisionKey {
                identity: identity.clone(),
                action_id: action_id.to_owned(),
            })
            .map(|value| value.outcome))
    }

    pub async fn wait(
        &self,
        identity_key: IdentityKey,
        identity: PermissionIdentitySummary,
        action: &PermissionAction,
    ) -> Result<ApprovalOutcome, ManagerError> {
        let created_at = now_millis();
        let deadline_at = created_at + APPROVAL_TIMEOUT.as_millis() as u64;
        let key = DecisionKey {
            identity: identity_key,
            action_id: action.action.id.to_owned(),
        };
        let (sender, receiver) = oneshot::channel();
        let (id, count) = {
            let mut state = self.state.lock().map_err(lock_error)?;
            state
                .temporary
                .retain(|_, value| value.expires_at > created_at);
            if let Some(decision) = state.temporary.get(&key) {
                return Ok(decision.outcome);
            }
            let id = state.next_id;
            state.next_id = state.next_id.saturating_add(1);
            state.pending.insert(
                id,
                PendingEntry {
                    summary: PendingApproval {
                        id,
                        identity,
                        action_id: action.action.id.to_owned(),
                        method: action.action.method.to_owned(),
                        route_template: action.action.route_template.to_owned(),
                        actual_path: action.actual_path.clone(),
                        query: action.query.clone(),
                        source: action.source.map(|value| value.to_string()),
                        content_type: action.content_type.clone(),
                        content_length: action.content_length,
                        body_preview: action.body_preview.clone(),
                        body_preview_truncated: action.body_preview_truncated,
                        created_at,
                        deadline_at,
                    },
                    key,
                    sender,
                },
            );
            (id, state.pending.len())
        };
        (self.notify)(count);
        tracing::info!(
            approval_id = id,
            action = action.action.id,
            "management API approval created"
        );
        let mut guard = PendingGuard {
            id,
            state: self.state.clone(),
            notify: self.notify.clone(),
            armed: true,
        };
        let outcome = match tokio::time::timeout(APPROVAL_TIMEOUT, receiver).await {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(_)) => ApprovalOutcome::Deny,
            Err(_) => ApprovalOutcome::Timeout,
        };
        guard.remove();
        tracing::info!(
            approval_id = id,
            ?outcome,
            "management API approval completed"
        );
        Ok(outcome)
    }

    pub fn list(&self) -> Result<Vec<PendingApproval>, ManagerError> {
        let state = self.state.lock().map_err(lock_error)?;
        let mut values = state
            .pending
            .values()
            .map(|entry| entry.summary.clone())
            .collect::<Vec<_>>();
        values.sort_by_key(|value| (value.deadline_at, value.id));
        Ok(values)
    }

    pub fn resolve(&self, id: u64, request: ResolveApprovalRequest) -> Result<(), ManagerError> {
        if let Some(seconds) = request.duration_seconds
            && !ALLOWED_DURATIONS.contains(&seconds)
        {
            return Err(ManagerError::bad_request(
                "approval duration must be 300, 1800, or 3600 seconds",
            ));
        }
        let outcome = match request.decision {
            ApprovalDecision::Allow => ApprovalOutcome::Allow,
            ApprovalDecision::Deny => ApprovalOutcome::Deny,
        };
        let (senders, count) = {
            let mut state = self.state.lock().map_err(lock_error)?;
            let entry = state
                .pending
                .remove(&id)
                .ok_or_else(|| ManagerError::not_found("approval request not found"))?;
            let mut senders = vec![entry.sender];
            if let Some(seconds) = request.duration_seconds {
                state.temporary.insert(
                    entry.key.clone(),
                    TemporaryDecision {
                        outcome,
                        expires_at: now_millis() + seconds * 1_000,
                    },
                );
                let matching = state
                    .pending
                    .iter()
                    .filter_map(|(id, pending)| (pending.key == entry.key).then_some(*id))
                    .collect::<Vec<_>>();
                for id in matching {
                    if let Some(entry) = state.pending.remove(&id) {
                        senders.push(entry.sender);
                    }
                }
            }
            (senders, state.pending.len())
        };
        for sender in senders {
            let _ = sender.send(outcome);
        }
        (self.notify)(count);
        tracing::info!(
            approval_id = id,
            ?outcome,
            duration = request.duration_seconds,
            "management API approval resolved"
        );
        Ok(())
    }
}

struct PendingGuard {
    id: u64,
    state: Arc<Mutex<ApprovalState>>,
    notify: ChangeNotifier,
    armed: bool,
}

impl PendingGuard {
    fn remove(&mut self) {
        if !self.armed {
            return;
        }
        self.armed = false;
        let count = match self.state.lock() {
            Ok(mut state) => {
                state.pending.remove(&self.id);
                state.pending.len()
            }
            Err(_) => return,
        };
        (self.notify)(count);
    }
}

impl Drop for PendingGuard {
    fn drop(&mut self) {
        if self.armed {
            tracing::info!(approval_id = self.id, "management API approval cancelled");
        }
        self.remove();
    }
}

pub fn outcome_denial(outcome: ApprovalOutcome) -> Option<PermissionDenied> {
    match outcome {
        ApprovalOutcome::Allow => None,
        ApprovalOutcome::Deny => Some(PermissionDenied::forbidden(
            "permission_denied",
            "management API action was denied",
        )),
        ApprovalOutcome::Timeout => Some(PermissionDenied::forbidden(
            "approval_timeout",
            "management API approval timed out",
        )),
    }
}

fn lock_error<T>(_: std::sync::PoisonError<T>) -> ManagerError {
    ManagerError::internal("approval state lock poisoned")
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use proxy_crab_mgr::permission::{api_actions, find_api_action};

    use super::*;
    use crate::http_permissions::model::{PermissionIdentityKind, PermissionIdentitySummary};

    fn identity(id: &str) -> PermissionIdentitySummary {
        PermissionIdentitySummary {
            id: id.into(),
            kind: PermissionIdentityKind::ApiKey,
            name: id.into(),
            prefix: Some("prefix".into()),
            created_at: Some(1),
            last_used_at: None,
        }
    }

    fn action() -> PermissionAction {
        PermissionAction {
            action: find_api_action("POST", "/api/proxy/start").unwrap(),
            authorization: None,
            actual_path: "/api/proxy/start".into(),
            query: None,
            source: None,
            content_type: None,
            content_length: None,
            body_preview: None,
            body_preview_truncated: false,
        }
    }

    async fn wait_until_pending(queue: &ApprovalQueue, expected: usize) {
        for _ in 0..50 {
            if queue.list().unwrap().len() == expected {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("pending approval count did not reach {expected}");
    }

    #[tokio::test]
    async fn one_shot_resolution_settles_only_selected_request() {
        let notifications = Arc::new(AtomicUsize::new(0));
        let observed = notifications.clone();
        let queue = Arc::new(ApprovalQueue::new(Arc::new(move |count| {
            observed.store(count, Ordering::SeqCst);
        })));
        let waiting = {
            let queue = queue.clone();
            tokio::spawn(async move {
                queue
                    .wait(IdentityKey::ApiKey("a".into()), identity("a"), &action())
                    .await
                    .unwrap()
            })
        };
        wait_until_pending(&queue, 1).await;
        let id = queue.list().unwrap()[0].id;
        queue
            .resolve(
                id,
                ResolveApprovalRequest {
                    decision: ApprovalDecision::Allow,
                    duration_seconds: None,
                },
            )
            .unwrap();

        assert_eq!(waiting.await.unwrap(), ApprovalOutcome::Allow);
        assert!(queue.list().unwrap().is_empty());
        assert_eq!(notifications.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn duration_resolution_settles_matching_waiters_and_sets_override() {
        let queue = Arc::new(ApprovalQueue::new(Arc::new(|_| {})));
        let first = {
            let queue = queue.clone();
            tokio::spawn(async move {
                queue
                    .wait(IdentityKey::ApiKey("a".into()), identity("a"), &action())
                    .await
                    .unwrap()
            })
        };
        let second = {
            let queue = queue.clone();
            tokio::spawn(async move {
                queue
                    .wait(IdentityKey::ApiKey("a".into()), identity("a"), &action())
                    .await
                    .unwrap()
            })
        };
        wait_until_pending(&queue, 2).await;
        let id = queue.list().unwrap()[0].id;
        queue
            .resolve(
                id,
                ResolveApprovalRequest {
                    decision: ApprovalDecision::Deny,
                    duration_seconds: Some(300),
                },
            )
            .unwrap();

        assert_eq!(first.await.unwrap(), ApprovalOutcome::Deny);
        assert_eq!(second.await.unwrap(), ApprovalOutcome::Deny);
        assert_eq!(
            queue
                .temporary_outcome(&IdentityKey::ApiKey("a".into()), "POST /api/proxy/start")
                .unwrap(),
            Some(ApprovalOutcome::Deny)
        );
        assert_eq!(api_actions().len(), 63);
    }

    #[tokio::test(start_paused = true)]
    async fn unanswered_request_times_out_after_thirty_seconds() {
        let queue = ApprovalQueue::new(Arc::new(|_| {}));
        let action = action();
        let waiting = queue.wait(IdentityKey::ApiKey("a".into()), identity("a"), &action);

        assert_eq!(waiting.await.unwrap(), ApprovalOutcome::Timeout);
        assert!(queue.list().unwrap().is_empty());
    }
}
