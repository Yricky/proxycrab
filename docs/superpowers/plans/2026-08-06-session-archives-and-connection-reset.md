# Session Archives and Connection Reset Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Disconnect every established proxy connection when effective Session/routing configuration changes, and replace direct Session deletion with archive, restore, and archived-only deletion workflows.

**Architecture:** Keep the proxy listener alive while rotating a connection generation containing its own cancellation token and task tracker; reset the upstream client at the same boundary so pooled connections are dropped. Store active Sessions under `sessions/` and archived Sessions under `sessions_archived/`, with Workspace owning atomic directory moves and MGR exposing archive-specific APIs that remain invisible to existing Session/log APIs.

**Tech Stack:** Rust, Tokio, Hyper, Axum, Tauri, Vue 3, TypeScript, Vitest/Vite build tooling.

---

### Task 1: Add connection-generation reset semantics

**Files:**
- Modify: `crates/proxy-crab-mitm/src/proxy.rs`
- Modify: `crates/proxy-crab-mitm/src/proxy/upstream.rs`
- Modify: `crates/proxy-crab-mitm/src/runtime.rs`
- Test: `crates/proxy-crab-mitm/tests/proxy_integration.rs`

- [ ] **Step 1: Write failing integration tests**

Add Tokio tests that hold an HTTP keep-alive or CONNECT stream open, switch the active Session, and assert the old stream reaches EOF while a new connection is captured by the new Session. Add routing tests proving selected-content changes disconnect while identical content and unselected script edits do not.

- [ ] **Step 2: Verify the tests fail**

Run: `cargo test -p proxy-crab-mitm connection -- --nocapture`

Expected: the old downstream connection remains usable or the new async mutation API is absent.

- [ ] **Step 3: Implement generation rotation**

Introduce a connection registry with this responsibility:

```rust
#[derive(Clone)]
struct ConnectionGeneration {
    cancellation: CancellationToken,
    tasks: TaskGroup,
}

impl ConnectionRegistry {
    fn rotate(&self) -> ConnectionGeneration;
    async fn shutdown(self, timeout: Duration);
}
```

Track capture and bypass IDs in each generation so forced shutdown changes only that generation's unfinished rows to the existing `proxy_shutdown` outcome. Rebuild `UpstreamClient` during rotation to discard verified and insecure pools.

- [ ] **Step 4: Route effective mutations through the reset boundary**

Make `ProxyCrab::replace_config`, `replace_active_session`, routing-script update/delete, and routing selection async. Compare old and new active Session ID, selected routing name, and selected routing content; call the serialized proxy mutation boundary only when the effective value changes.

- [ ] **Step 5: Run MITM tests**

Run: `cargo test -p proxy-crab-mitm`

Expected: all unit and integration tests pass.

### Task 2: Add Workspace Session archives

**Files:**
- Modify: `crates/proxy-crab-mitm/src/workspace.rs`
- Modify: `crates/proxy-crab-mitm/src/migration.rs`
- Modify: `crates/proxy-crab-mitm/src/runtime.rs`
- Test: `crates/proxy-crab-mitm/src/workspace.rs`
- Test: `crates/proxy-crab-mitm/tests/proxy_integration.rs`

- [ ] **Step 1: Write failing archive tests**

Cover directory movement, restart persistence, active-Session rejection, in-progress pin rejection, restore, archived-only deletion, and exclusion from existing Session/log/view/interceptor lookups.

- [ ] **Step 2: Implement archive storage**

Add `archived_sessions: RwLock<Vec<SessionMetadata>>` and these operations:

```rust
pub fn archived_sessions(&self) -> Vec<SessionMetadata>;
pub fn archive_session(&self, id: u64) -> Result<SessionMetadata>;
pub fn restore_session(&self, id: u64) -> Result<SessionMetadata>;
pub fn delete_archived_session(&self, id: u64) -> Result<()>;
```

Move whole directories between `sessions/<id>` and `sessions_archived/<id>`. Reject archiving the active Session and preserve IDs and metadata. Ensure new IDs do not collide with archived IDs and apply workspace migrations to both roots.

- [ ] **Step 3: Add runtime safety**

Serialize archive mutations with proxy lifecycle changes. Hold the Session pin registry while checking that no request is in progress, evict the capture-store cache before moving, and permit archive/restore/delete while the proxy is running.

- [ ] **Step 4: Run Workspace and runtime tests**

Run: `cargo test -p proxy-crab-mitm workspace`

Expected: archive tests pass and active Session behavior remains unchanged.

### Task 3: Replace MGR deletion with archive APIs

**Files:**
- Modify: `crates/proxy-crab-mgr/src/manager.rs`
- Modify: `crates/proxy-crab-mgr/src/http.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `crates/proxy-crab-mgr/src/http.rs`

- [ ] **Step 1: Write route contract tests**

Assert `DELETE /api/sessions/{id}` returns 404, and verify:

```text
GET    /api/archived-sessions
POST   /api/sessions/{id}/archive
POST   /api/archived-sessions/{id}/restore
DELETE /api/archived-sessions/{id}
```

Also assert archived IDs return `not_found` through existing log and Session-specific routes.

- [ ] **Step 2: Extend manager and HTTP synchronization resources**

Replace `delete_session` in `ProxyCrabManager` with list/archive/restore/delete-archived methods. Map active Session and in-progress request failures to stable `conflict` responses. Publish both `sessions` and `archived_sessions` changes for moves, and only `archived_sessions` for permanent deletion.

- [ ] **Step 3: Replace Tauri commands**

Remove `delete_session` and register `list_archived_sessions`, `archive_session`, `restore_session`, and `delete_archived_session`.

- [ ] **Step 4: Run manager tests**

Run: `cargo test -p proxy-crab-mgr`

Expected: HTTP contracts and manager behavior pass.

### Task 4: Add archived Sessions UI

**Files:**
- Create: `src/windows/ArchivedSessionsWindow.vue`
- Modify: `src/windows/launcher.ts`
- Modify: `src/components/SessionSidebar.vue`
- Modify: `src/stores/sessions.ts`
- Modify: `src/stores/http-api-sync.ts`
- Modify: `src/api/backend.ts`
- Modify: `src/api/tauri-backend.ts`
- Modify: `src/api/types.ts`

- [ ] **Step 1: Add typed backend methods and reactive state**

Expose archived list/archive/restore/delete operations. Refresh active and archived collections after local commands and HTTP synchronization events; if the viewed Session is archived, fall back to the first remaining active Session.

- [ ] **Step 2: Replace the regular Session delete action**

Show “归档” for inactive Sessions and disable it for the active Session with an explanatory title. Confirm that archiving preserves data.

- [ ] **Step 3: Build the archived window**

Render metadata-only rows with restore and dangerous permanent-delete actions. Add a fixed “已归档 Session” button below the scrollable sidebar list and launch one reusable floating window.

- [ ] **Step 4: Run frontend verification**

Run: `pnpm build`

Expected: Vue type checking and production build pass.

### Task 5: Synchronize public documentation and bundled Skill

**Files:**
- Modify: `docs/backend-api.md`
- Modify: `skills/proxycrab/references/http-api.md`
- Modify: `skills/proxycrab/SKILL.md`
- Modify: `README.md`
- Modify: `skills/proxycrab/evals/evals.json` when API expectations are asserted there

- [ ] **Step 1: Document connection reset behavior**

State that effective active Session or selected routing changes close downstream connections, tunnels/upgrades, and upstream pools while the listener remains running; no-op updates do not reset connections.

- [ ] **Step 2: Document archive APIs and deletion restriction**

Describe `sessions_archived/`, archived Session invisibility, active/in-use archive conflicts, restore, and permanent deletion. Remove every documented reference to direct Session deletion.

- [ ] **Step 3: Run final verification**

Run: `cargo fmt --check`, `cargo test --workspace`, and `pnpm build`.

Expected: all commands exit successfully and the working tree contains only requested feature changes.
