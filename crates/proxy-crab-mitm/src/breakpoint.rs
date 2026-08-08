use std::{
    collections::HashMap,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Result, anyhow};

use crate::{
    lua::{
        ModificationJournal, ResponseScriptContext, SharedInterceptorState,
        execute_request_with_state, execute_response_with_state,
    },
    model::{
        BreakpointListFilter, BreakpointSummary, CaptureError, ErrorStage, InterceptorExecution,
        InterceptorExecutionOrigin, InterceptorKind, InterceptorRun, RequestData, ResponseData,
        TemporaryExecutionResult, script_content_hash,
    },
    storage::CaptureStore,
    workspace::now_millis,
};

pub const MAX_BREAKPOINT_MS: u64 = 1_800_000;

#[derive(Clone)]
pub(crate) struct BreakpointContext {
    pub session_id: u64,
    pub capture_id: u64,
    pub phase: InterceptorKind,
    pub position: usize,
    pub interceptor_name: String,
    pub parent_execution_id: u64,
    pub request: RequestData,
    pub response: Option<ResponseData>,
    pub state: SharedInterceptorState,
    pub parent_journal: ModificationJournal,
    pub store: CaptureStore,
}

#[derive(Clone)]
pub(crate) struct LiveBreakpointSnapshot {
    pub summary: BreakpointSummary,
    pub context: BreakpointContext,
}

struct DeadlineState {
    expires_at: Instant,
    maximum_expires_at: Instant,
    released: bool,
}

struct ActiveBreakpoint {
    id: u64,
    created_at: u64,
    created_instant: Instant,
    context: BreakpointContext,
    deadline: Mutex<DeadlineState>,
    wake: Condvar,
    execution_gate: Mutex<()>,
}

impl ActiveBreakpoint {
    fn summary(&self, now: Instant) -> BreakpointSummary {
        let deadline = self
            .deadline
            .lock()
            .expect("breakpoint deadline lock poisoned");
        let remaining_ms = deadline
            .expires_at
            .checked_duration_since(now)
            .unwrap_or_default()
            .as_millis() as u64;
        let expires_at = self.created_at
            + deadline
                .expires_at
                .duration_since(self.created_instant)
                .as_millis() as u64;
        BreakpointSummary {
            id: self.id,
            session_id: self.context.session_id,
            capture_id: self.context.capture_id,
            phase: self.context.phase,
            position: self.context.position,
            interceptor_name: self.context.interceptor_name.clone(),
            method: self.context.request.method.clone(),
            uri: self.context.request.uri.clone(),
            created_at: self.created_at,
            expires_at,
            remaining_ms,
        }
    }

    fn is_available(&self, now: Instant) -> bool {
        let deadline = self
            .deadline
            .lock()
            .expect("breakpoint deadline lock poisoned");
        !deadline.released && now < deadline.expires_at
    }
}

#[derive(Default)]
pub(crate) struct BreakpointRegistry {
    next_id: AtomicU64,
    active: Mutex<HashMap<u64, Arc<ActiveBreakpoint>>>,
}

impl BreakpointRegistry {
    pub(crate) fn wait(&self, context: BreakpointContext, timeout_ms: u64) -> Result<()> {
        let timeout_ms = timeout_ms.min(MAX_BREAKPOINT_MS);
        if timeout_ms == 0 {
            return Ok(());
        }

        context.store.update_interceptor_run(
            context.parent_execution_id,
            &context.parent_journal.snapshot(),
            None,
            false,
        )?;
        context
            .store
            .update_tags(context.capture_id, &context.state.tags())?;

        let created_instant = Instant::now();
        let active = Arc::new(ActiveBreakpoint {
            id: self.next_id.fetch_add(1, Ordering::Relaxed) + 1,
            created_at: now_millis(),
            created_instant,
            context,
            deadline: Mutex::new(DeadlineState {
                expires_at: created_instant + Duration::from_millis(timeout_ms),
                maximum_expires_at: created_instant + Duration::from_millis(MAX_BREAKPOINT_MS),
                released: false,
            }),
            wake: Condvar::new(),
            execution_gate: Mutex::new(()),
        });
        self.active
            .lock()
            .expect("breakpoint registry lock poisoned")
            .insert(active.id, active.clone());

        let mut deadline = active
            .deadline
            .lock()
            .expect("breakpoint deadline lock poisoned");
        loop {
            if deadline.released {
                break;
            }
            let now = Instant::now();
            let Some(remaining) = deadline.expires_at.checked_duration_since(now) else {
                break;
            };
            if remaining.is_zero() {
                break;
            }
            let (next, _) = active
                .wake
                .wait_timeout(deadline, remaining)
                .expect("breakpoint deadline lock poisoned while waiting");
            deadline = next;
        }
        drop(deadline);

        let _execution = active
            .execution_gate
            .lock()
            .expect("breakpoint execution gate poisoned");
        self.remove_if_same(active.id, &active);
        Ok(())
    }

    pub(crate) fn list(&self, filter: &BreakpointListFilter) -> Vec<BreakpointSummary> {
        let now = Instant::now();
        let mut items = self
            .active
            .lock()
            .expect("breakpoint registry lock poisoned")
            .values()
            .filter(|active| active.context.session_id == filter.session_id)
            .filter(|active| {
                filter
                    .phase
                    .is_none_or(|phase| active.context.phase == phase)
            })
            .filter(|active| {
                filter
                    .interceptor_name
                    .as_deref()
                    .is_none_or(|name| active.context.interceptor_name == name)
            })
            .filter(|active| active.is_available(now))
            .map(|active| active.summary(now))
            .collect::<Vec<_>>();
        items.sort_by_key(|item| std::cmp::Reverse(item.id));
        items
    }

    pub(crate) fn detail(&self, id: u64) -> Result<LiveBreakpointSnapshot> {
        let active = self.active(id)?;
        if !active.is_available(Instant::now()) {
            return Err(not_found(id));
        }
        Ok(LiveBreakpointSnapshot {
            summary: active.summary(Instant::now()),
            context: active.context.clone(),
        })
    }

    pub(crate) fn extend(&self, id: u64, timeout_ms: u64) -> Result<BreakpointSummary> {
        let active = self.active(id)?;
        let now = Instant::now();
        let mut deadline = active
            .deadline
            .lock()
            .expect("breakpoint deadline lock poisoned");
        if deadline.released || now >= deadline.expires_at {
            return Err(not_found(id));
        }
        let extension = Duration::from_millis(timeout_ms.min(MAX_BREAKPOINT_MS));
        deadline.expires_at = deadline
            .expires_at
            .checked_add(extension)
            .unwrap_or(deadline.maximum_expires_at)
            .min(deadline.maximum_expires_at);
        drop(deadline);
        active.wake.notify_all();
        Ok(active.summary(now))
    }

    pub(crate) fn release(&self, id: u64) -> Result<()> {
        let active = self.active(id)?;
        let mut deadline = active
            .deadline
            .lock()
            .expect("breakpoint deadline lock poisoned");
        if deadline.released || Instant::now() >= deadline.expires_at {
            return Err(not_found(id));
        }
        deadline.released = true;
        drop(deadline);
        active.wake.notify_all();
        Ok(())
    }

    pub(crate) fn execute_temporary(
        &self,
        id: u64,
        content: &str,
    ) -> Result<TemporaryExecutionResult> {
        let active = self.active(id)?;
        let _execution = active
            .execution_gate
            .lock()
            .expect("breakpoint execution gate poisoned");
        if !active.is_available(Instant::now()) {
            return Err(not_found(id));
        }

        let context = &active.context;
        let journal = ModificationJournal::new(context.state.headers());
        let (_, error) = match context.phase {
            InterceptorKind::Request => execute_request_with_state(
                content,
                &context.request,
                context.state.clone(),
                journal.clone(),
                "临时脚本",
                Some(context.capture_id),
                None,
            )?,
            InterceptorKind::Response => execute_response_with_state(
                content,
                ResponseScriptContext {
                    request: &context.request,
                    response: context
                        .response
                        .as_ref()
                        .ok_or_else(|| anyhow!("response breakpoint is missing response state"))?,
                },
                context.state.clone(),
                journal.clone(),
                "临时脚本",
                Some(context.capture_id),
                None,
            )?,
        };
        let modifications = journal.snapshot();
        let script_hash = script_content_hash(content);
        let run = InterceptorRun {
            origin: InterceptorExecutionOrigin::Temporary,
            completed: true,
            phase: context.phase,
            position: context.position,
            name: "临时脚本".into(),
            script_hash: script_hash.clone(),
            content: content.into(),
            modifications: modifications.clone(),
            error: error.clone(),
        };
        let execution_id = context
            .store
            .begin_interceptor_run(context.capture_id, &run)?;
        if let Some(message) = error.as_deref() {
            context.store.note_error(
                context.capture_id,
                &CaptureError {
                    stage: ErrorStage::Interceptor,
                    kind: "interceptor_runtime_error".into(),
                    message: format!("临时脚本: {message}"),
                },
            )?;
            tracing::warn!("temporary interceptor script failed: {message}");
        }
        context
            .store
            .update_tags(context.capture_id, &context.state.tags())?;
        Ok(TemporaryExecutionResult {
            execution: InterceptorExecution {
                execution_id,
                origin: InterceptorExecutionOrigin::Temporary,
                completed: true,
                phase: context.phase,
                position: context.position,
                name: run.name,
                script_hash,
                content: content.into(),
                modifications,
                error,
            },
            breakpoint: active.summary(Instant::now()),
        })
    }

    pub(crate) fn release_all(&self) {
        let active = self
            .active
            .lock()
            .expect("breakpoint registry lock poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for item in active {
            item.deadline
                .lock()
                .expect("breakpoint deadline lock poisoned")
                .released = true;
            item.wake.notify_all();
        }
    }

    fn active(&self, id: u64) -> Result<Arc<ActiveBreakpoint>> {
        self.active
            .lock()
            .expect("breakpoint registry lock poisoned")
            .get(&id)
            .cloned()
            .ok_or_else(|| not_found(id))
    }

    fn remove_if_same(&self, id: u64, expected: &Arc<ActiveBreakpoint>) {
        let mut active = self
            .active
            .lock()
            .expect("breakpoint registry lock poisoned");
        if active
            .get(&id)
            .is_some_and(|current| Arc::ptr_eq(current, expected))
        {
            active.remove(&id);
        }
    }
}

fn not_found(id: u64) -> anyhow::Error {
    anyhow!("breakpoint {id} not found")
}

#[cfg(test)]
mod tests {
    use std::{sync::mpsc, thread, time::Duration};

    use tempfile::tempdir;

    use crate::{
        lua::{ModificationJournal, SharedInterceptorState},
        model::{
            BreakpointListFilter, HeaderValues, InterceptorExecutionOrigin, InterceptorKind,
            InterceptorRun, RequestData, script_content_hash,
        },
        storage::CaptureStore,
    };

    use super::{BreakpointContext, BreakpointRegistry, MAX_BREAKPOINT_MS};

    fn context(root: &std::path::Path) -> BreakpointContext {
        let store = CaptureStore::open(1, root).unwrap();
        let request = RequestData {
            method: "GET".into(),
            uri: "http://example.com/test".into(),
            version: "HTTP/1.1".into(),
            headers: HeaderValues::new(),
            tags: Default::default(),
        };
        let capture_id = store.begin("127.0.0.1", &request, "request").unwrap();
        let state = SharedInterceptorState::new_request(
            request.method.clone(),
            request.uri.clone(),
            request.headers.clone(),
            request.tags.clone(),
        );
        let journal = ModificationJournal::new(request.headers.clone());
        let source = "breakpoint(1000)";
        let parent_execution_id = store
            .begin_interceptor_run(
                capture_id,
                &InterceptorRun {
                    origin: InterceptorExecutionOrigin::Saved,
                    completed: false,
                    phase: InterceptorKind::Request,
                    position: 0,
                    name: "hold".into(),
                    script_hash: script_content_hash(source),
                    content: source.into(),
                    modifications: journal.snapshot(),
                    error: None,
                },
            )
            .unwrap();
        BreakpointContext {
            session_id: 1,
            capture_id,
            phase: InterceptorKind::Request,
            position: 0,
            interceptor_name: "hold".into(),
            parent_execution_id,
            request,
            response: None,
            state,
            parent_journal: journal,
            store,
        }
    }

    fn filter() -> BreakpointListFilter {
        BreakpointListFilter {
            session_id: 1,
            phase: Some(InterceptorKind::Request),
            interceptor_name: Some("hold".into()),
        }
    }

    #[test]
    fn zero_timeout_returns_without_registering() {
        let root = tempdir().unwrap();
        let registry = BreakpointRegistry::default();
        registry.wait(context(root.path()), 0).unwrap();
        assert!(registry.list(&filter()).is_empty());
    }

    #[test]
    fn release_and_temporary_scripts_share_state_and_keep_separate_history() {
        let root = tempdir().unwrap();
        let registry = std::sync::Arc::new(BreakpointRegistry::default());
        let (done_tx, done_rx) = mpsc::channel();
        let worker_registry = registry.clone();
        let context = context(root.path());
        let capture_id = context.capture_id;
        thread::spawn(move || {
            worker_registry.wait(context, 5_000).unwrap();
            done_tx.send(()).unwrap();
        });

        for _ in 0..100 {
            if !registry.list(&filter()).is_empty() {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        let id = registry.list(&filter())[0].id;
        let first = registry
            .execute_temporary(id, "req:set_tag('team', 'one')")
            .unwrap();
        let second = registry
            .execute_temporary(id, "req:set_tag('team', 'two')")
            .unwrap();
        let rejected_breakpoint = registry
            .execute_temporary(id, "req:set_tag('before-error', 'kept'); breakpoint(1000)")
            .unwrap();
        assert_eq!(
            first.execution.origin,
            InterceptorExecutionOrigin::Temporary
        );
        assert_ne!(first.execution.execution_id, second.execution.execution_id);
        assert!(
            rejected_breakpoint
                .execution
                .error
                .as_deref()
                .is_some_and(|error| error.contains("breakpoint"))
        );
        assert!(done_rx.recv_timeout(Duration::from_millis(20)).is_err());
        registry.release(id).unwrap();
        done_rx.recv_timeout(Duration::from_secs(1)).unwrap();

        let store = CaptureStore::open(1, root.path()).unwrap();
        let detail = store.get(capture_id).unwrap().unwrap();
        assert_eq!(detail.summary.request.tags["team"], "two");
        assert_eq!(detail.summary.request.tags["before-error"], "kept");
        assert_eq!(detail.request_interceptors.len(), 4);
        assert_eq!(
            detail.request_interceptors[3].execution_id,
            rejected_breakpoint.execution.execution_id
        );
        assert!(detail.request_interceptors[3].error.is_some());
    }

    #[test]
    fn timeout_and_extensions_update_deadline_with_a_hard_cap() {
        let root = tempdir().unwrap();
        let registry = std::sync::Arc::new(BreakpointRegistry::default());
        let worker_registry = registry.clone();
        let context = context(root.path());
        let worker = thread::spawn(move || worker_registry.wait(context, 40).unwrap());
        for _ in 0..100 {
            if !registry.list(&filter()).is_empty() {
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        let item = registry.list(&filter())[0].clone();
        let extended = registry.extend(item.id, MAX_BREAKPOINT_MS).unwrap();
        assert!(extended.expires_at - extended.created_at <= MAX_BREAKPOINT_MS);
        registry.release(item.id).unwrap();
        worker.join().unwrap();
    }
}
