# Interceptor Breakpoints and Request Tags Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add pausable Lua request/response interceptors, live breakpoint inspection and control, persistent request tags, and `_crab_skip` short-circuit responses without exposing tags to the upstream server.

**Architecture:** Keep persistent capture state in SQLite and active breakpoint state in a process-local registry owned by `ProxyCrab`. Each interceptor execution gets a shared mutable request/response state plus its own modification journal; temporary scripts reuse the shared state but write an independent historical execution record. Long-running Lua executions move to Tokio blocking workers so `breakpoint(timeoutMs)` can wait on a condition variable without blocking the async proxy runtime.

**Tech Stack:** Rust 2024, Tokio, mlua/Lua 5.4, rusqlite/SQLite, Axum management API, Tauri 2 commands, Vue 3/TypeScript, Monaco Editor.

**Implementation status (2026-08-03):** Implemented. The final design also centralizes all
workspace persistence migrations behind `workspace_schema.json`; schema v1 documents and migrates
request tags plus ID-based multi-execution interceptor history. Verified with the full Rust test
workspace, strict Clippy, frontend production build, formatting/diff checks, focused live
request/response breakpoint regressions, and an independent code review.

---

## Confirmed behavior and implementation assumptions

- `breakpoint(timeoutMs)` is a global Lua function available only to saved request and response interceptor scripts.
- `timeoutMs` must be a non-negative integer. `0` returns immediately. A single breakpoint may block for at most 1,800,000 ms total, including extensions; values above the remaining allowance are silently clipped.
- Timeout automatically releases the request. Manual release resumes the saved script immediately after the `breakpoint()` call.
- A temporary script has exactly the same globals and mutable capabilities as the interceptor phase containing the breakpoint, except `breakpoint` is absent.
- A request temporary script receives mutable `req`. A response temporary script receives mutable `resp` plus a tag-only `req` exposing `setTag/getTag`, matching saved response interceptor capability.
- Running a temporary script never releases the breakpoint. It may be run repeatedly. Runtime and syntax errors use normal interceptor semantics: mutations made before the error remain applied, and the error is recorded.
- Each temporary execution is stored independently with exact source, ordered modifications, hash, and optional error. Its modifications are not duplicated in the parent interceptor record; the resumed parent script sees the resulting latest state.
- Each pause is independent, including multiple `breakpoint()` calls made by the same script. The 30-minute cap applies per pause.
- Tags are `BTreeMap<String, String>`/`Map<string,string>`. `setTag` overwrites a key; `getTag` returns Lua `nil` when absent; an empty string is present. No delete API is added.
- Tags live on the capture request model, are persisted in SQLite, are returned in `GET /api/logs/{id}`, and are available through read-only `entry.req:getTag` in filter/custom-column scripts. There is no standalone tag UI in this release.
- `Modification::TagSet` makes tag mutations visible in the existing log-detail interceptor history. Repeated sets remain ordered like header mutations.
- After the entire request-interceptor chain finishes, `_crab_skip` is checked once by key presence. When present, no upstream request is made; ProxyCrab creates `HTTP/1.1 200`, empty headers, and an empty body, then executes the complete response interceptor chain and returns its result.
- The built-in `proxy.crab/ca.crt` local handler keeps its existing special-case behavior; `_crab_skip` governs normal captured upstream forwarding.
- Breakpoint badges are scoped to the Session currently shown in the UI. Clicking a yellow interceptor always opens a list, even when it contains one request. Counts render as `1` through `9`, then `9+`.
- If a breakpoint expires while a temporary script is running, the timer remains elapsed but the parent script resumes only after that in-flight temporary execution exits, preventing concurrent mutation of one Lua state.
- Proxy shutdown wakes active breakpoint waiters as part of normal request cancellation; editing, disabling, reordering, or deleting saved interceptor definitions does not alter already snapshotted executions.

## File map

**Create**

- `crates/proxy-crab-mitm/src/breakpoint.rs` — active breakpoint registry, deadline/release coordination, live snapshots, and temporary execution entry point.
- `crates/proxy-crab-mitm/src/migration.rs` — workspace-wide schema version and the complete ordered migration chain.
- `src/stores/breakpoints.ts` — current-Session breakpoint polling and per-interceptor counts.
- `src/windows/BreakpointListWindow.vue` — always-list UI for a yellow interceptor.

**Modify**

- `crates/proxy-crab-mitm/src/lib.rs` — register the breakpoint module.
- `crates/proxy-crab-mitm/src/model.rs` — tags, tag modification, execution-origin metadata, and breakpoint DTOs.
- `crates/proxy-crab-mitm/src/storage.rs` — capture-tag schema migration and multi-execution history persistence.
- `crates/proxy-crab-mitm/src/lua.rs` — shared live state, tag methods, breakpoint global, tag-only response request, and temporary execution.
- `crates/proxy-crab-mitm/src/runtime.rs` — own/expose the registry, overlay live details, and release waiters during shutdown.
- `crates/proxy-crab-mitm/src/proxy.rs` — blocking Lua dispatch, shared tag propagation, `_crab_skip`, default response, and response-chain reuse.
- `crates/proxy-crab-mitm/tests/proxy_integration.rs` — upstream-skip, response-chain, breakpoint, and tag persistence tests.
- `crates/proxy-crab-mgr/src/dto.rs` — HTTP/Tauri breakpoint request/response types and `request.tags`.
- `crates/proxy-crab-mgr/src/manager.rs` — manager trait and MITM implementation for breakpoint operations.
- `crates/proxy-crab-mgr/src/http.rs` — management routes, validation, envelopes, and endpoint tests.
- `src-tauri/src/lib.rs` — Tauri breakpoint commands and registration.
- `src/api/types.ts` — breakpoint, tags, and execution-origin TypeScript types.
- `src/api/backend.ts` — backend interface methods.
- `src/api/tauri-backend.ts` — Tauri invoke mappings.
- `src/App.vue` — start and stop breakpoint polling.
- `src/components/InterceptorPipeline.vue` — yellow state, numeric badge, and list launcher behavior.
- `src/windows/LogDetailWindow.vue` — live breakpoint mode, extend/release/temp-script controls, and tag modification rendering.
- `src/windows/ScriptSnapshotWindow.vue` — distinguish saved versus temporary historical executions.
- `src/windows/launcher.ts` — breakpoint list/detail launchers with stable unique window IDs.
- `docs/lua-api.md`, `docs/backend-api.md` — product API documentation.
- `skills/proxycrab/SKILL.md`, `skills/proxycrab/references/lua-api.md`, `skills/proxycrab/references/http-api.md` — bundled agent skill guidance.
- `skills/proxycrab/evals/evals.json` and Lua fixtures — tag/skip/breakpoint evaluation coverage.

## Task 1: Persist tags and multiple execution records

**Files:**

- Modify: `crates/proxy-crab-mitm/src/model.rs`
- Modify: `crates/proxy-crab-mitm/src/storage.rs`

- [ ] **Step 1: Add failing model/storage tests**

Add tests that open an old-format database, reopen it through `CaptureStore::open`, and verify:

```rust
assert_eq!(store.get(capture_id)?.unwrap().summary.request.tags["team"], "checkout");
assert_eq!(detail.request_interceptors.len(), 3);
assert_eq!(detail.request_interceptors[0].origin, InterceptorExecutionOrigin::Saved);
assert_eq!(detail.request_interceptors[1].origin, InterceptorExecutionOrigin::Temporary);
assert_eq!(detail.request_interceptors[2].origin, InterceptorExecutionOrigin::Temporary);
assert!(detail.request_interceptors.iter().all(|run| run.execution_id > 0));
```

The migration test must build the current pre-feature schema explicitly, insert one saved execution, reopen it, and assert its source/modifications survive migration.

- [ ] **Step 2: Run the focused tests and confirm they fail**

Run:

```bash
cargo test -p proxy-crab-mitm storage::tests -- --nocapture
```

Expected: compilation failures for missing `tags`, `origin`, and execution IDs, followed by migration test failures until the schema work exists.

- [ ] **Step 3: Extend the model**

Add the following shapes, preserving `#[serde(default)]` compatibility for pre-feature serialized values:

```rust
pub type RequestTags = BTreeMap<String, String>;

pub struct RequestData {
    pub method: String,
    pub uri: String,
    pub version: String,
    pub headers: HeaderValues,
    #[serde(default)]
    pub tags: RequestTags,
}

pub enum Modification {
    // existing variants
    TagSet { key: String, value: String },
}

#[serde(rename_all = "snake_case")]
pub enum InterceptorExecutionOrigin {
    Saved,
    Temporary,
}

pub struct InterceptorExecution {
    pub execution_id: u64,
    pub origin: InterceptorExecutionOrigin,
    pub completed: bool,
    // existing phase/position/name/hash/content/modifications/error fields
}
```

Mirror those fields in `InterceptorRun`. A saved run uses its configured interceptor name; a temporary run uses `"临时脚本"` and keeps the parent phase/position.

- [x] **Step 4: Migrate SQLite without losing existing captures**

At `Workspace::open`, before any store is used, read the root `workspace_schema.json` label and run
all pending migrations from the centralized `migration.rs`. Version 0 to 1:

1. Add `req_tags TEXT NOT NULL DEFAULT '{}'` to `captures` only when `PRAGMA table_info(captures)` shows it is absent.
2. Detect the legacy composite-primary-key execution table by absence of an `id` column.
3. In one transaction, create `capture_interceptor_runs_v2` with:

```sql
id INTEGER PRIMARY KEY AUTOINCREMENT,
capture_id INTEGER NOT NULL,
phase TEXT NOT NULL CHECK (phase IN ('request', 'response')),
position INTEGER NOT NULL,
origin TEXT NOT NULL CHECK (origin IN ('saved', 'temporary')),
name TEXT NOT NULL,
script_hash TEXT NOT NULL,
modifications TEXT NOT NULL DEFAULT '[]',
error TEXT,
completed INTEGER NOT NULL DEFAULT 1,
created_at INTEGER NOT NULL
```

4. Copy legacy rows as `origin='saved'`, `completed=1`, ordered by capture/phase/position; drop the legacy table; rename v2; recreate the `(capture_id, phase, id)` index and foreign keys.

Only the latest schema remains in `CaptureStore::initialize`; migration/`ALTER TABLE` logic must not
be distributed through individual stores. Atomically advance the workspace label after the version
migration succeeds, reject newer unsupported versions, and comment each version function with its
storage-model changes.

Replace one-shot `record_interceptor_run` with:

```rust
pub fn begin_interceptor_run(&self, capture_id: u64, run: &InterceptorRun) -> Result<u64>;
pub fn update_interceptor_run(
    &self,
    execution_id: u64,
    modifications: &[Modification],
    error: Option<&str>,
    completed: bool,
) -> Result<()>;
pub fn update_tags(&self, capture_id: u64, tags: &RequestTags) -> Result<()>;
```

Select `req_tags` in every capture query and order execution history by `id`, while grouping each phase in its existing response fields.

- [ ] **Step 5: Run storage tests**

```bash
cargo test -p proxy-crab-mitm storage::tests -- --nocapture
```

Expected: all storage tests pass, including legacy migration, tag round-trip, and multiple temporary runs at one parent position.

## Task 2: Add tag APIs and shared interceptor state to Lua

**Files:**

- Modify: `crates/proxy-crab-mitm/src/lua.rs`
- Modify: `crates/proxy-crab-mitm/src/model.rs`

- [ ] **Step 1: Add failing Lua tests**

Cover all agreed contracts:

```lua
assert(req:getTag("missing") == nil)
req:setTag("empty", "")
assert(req:getTag("empty") == "")
req:setTag("team", "one")
req:setTag("team", "two")
```

Verify ordered `tag_set` modifications, overwrite behavior, and that mutations before a runtime error remain. Add response tests proving `req:setTag/getTag` exist while `req.headers`, `req.body`, `req.method`, and `req.uri` are nil. Add filter/column tests proving `entry.req:getTag` reads persisted values but has no setter.

- [ ] **Step 2: Run the Lua tests and confirm failure**

```bash
cargo test -p proxy-crab-mitm lua::tests -- --nocapture
```

Expected: failures for missing tag methods and shared state.

- [ ] **Step 3: Separate shared state from per-execution journals**

Refactor the current `MutationState` into:

```rust
#[derive(Clone)]
pub(crate) struct SharedInterceptorState {
    headers: Arc<Mutex<HeaderValues>>,
    body: Arc<Mutex<Option<BodyReplacement>>>,
    tags: Arc<Mutex<RequestTags>>,
}

#[derive(Clone)]
struct ModificationJournal(Arc<Mutex<Vec<Modification>>>);
```

Every `MutableHeaders`, `MutableBody`, and tag method receives both the shared state and the current journal. Saved and temporary scripts share headers/body/tags but create different journals, so state is visible across executions without duplicating history.

- [ ] **Step 4: Expose tags in each required Lua view**

Implement camelCase userdata methods exactly as requested:

```rust
methods.add_method("getTag", |_, this, key: String| this.state.get_tag(&key));
methods.add_method("setTag", |_, this, (key, value): (String, String)| {
    this.state.set_tag(key.clone(), value.clone());
    this.journal.push(Modification::TagSet { key, value });
    Ok(())
});
```

- Request interceptors: add both methods to mutable `req`.
- Response interceptors: install global `req` as a tag-only userdata and retain mutable `resp`.
- Filter/custom-column `entry.req`: add read-only `getTag` only.
- Routing scripts remain unchanged because tags do not exist before capture/interceptor execution.

- [ ] **Step 5: Add an execution-options boundary for later breakpoint injection**

Replace phase-specific positional arguments with an internal options object:

```rust
pub(crate) struct InterceptorExecutionOptions {
    pub script_name: String,
    pub capture_log_id: Option<u64>,
    pub state: SharedInterceptorState,
    pub allow_breakpoint: bool,
    pub breakpoint_context: Option<BreakpointContext>,
}
```

Saved scripts set `allow_breakpoint=true`; temporary scripts set it to `false`, so calling `breakpoint()` in temporary source produces a normal Lua runtime error and historical error record.

- [ ] **Step 6: Run Lua and crate tests**

```bash
cargo test -p proxy-crab-mitm lua::tests -- --nocapture
cargo test -p proxy-crab-mitm
```

Expected: all tests pass before breakpoint waiting is introduced.

## Task 3: Implement the active breakpoint registry

**Files:**

- Create: `crates/proxy-crab-mitm/src/breakpoint.rs`
- Modify: `crates/proxy-crab-mitm/src/lib.rs`
- Modify: `crates/proxy-crab-mitm/src/model.rs`
- Modify: `crates/proxy-crab-mitm/src/lua.rs`
- Modify: `crates/proxy-crab-mitm/src/runtime.rs`

- [ ] **Step 1: Add registry lifecycle tests**

Use short deterministic durations and channels to verify:

- `0` ms creates no list item and returns immediately.
- `20` ms auto-releases.
- explicit release wakes before timeout.
- extension adds to the existing deadline.
- initial plus extensions never exceed 1,800,000 ms from creation.
- list filtering is exact by Session, phase, and interceptor name.
- two requests paused in the same interceptor produce count two.
- an in-flight temporary execution completes before the saved script resumes.
- temporary scripts may run repeatedly, share state, create independent histories, and cannot call `breakpoint`.

- [ ] **Step 2: Define public runtime DTOs**

Add serializable model types:

```rust
pub struct BreakpointSummary {
    pub id: u64,
    pub session_id: u64,
    pub capture_id: u64,
    pub phase: InterceptorKind,
    pub position: usize,
    pub interceptor_name: String,
    pub method: String,
    pub uri: String,
    pub created_at: u64,
    pub expires_at: u64,
    pub remaining_ms: u64,
}

pub struct BreakpointListFilter {
    pub session_id: u64,
    pub phase: Option<InterceptorKind>,
    pub interceptor_name: Option<String>,
}

pub struct TemporaryExecutionResult {
    pub execution: InterceptorExecution,
    pub breakpoint: BreakpointSummary,
}
```

`BreakpointDetail` contains a `BreakpointSummary` plus a live-overlaid `CaptureDetail`, so the manager can reuse normal log-detail conversion.

- [ ] **Step 3: Implement registry timing and synchronization**

Create `BreakpointRegistry` with an atomic ID, `Mutex<HashMap<u64, Arc<ActiveBreakpoint>>>`, and per-entry `Condvar`. Use `Instant` for deadline comparison and Unix milliseconds only for DTO display.

Expose:

```rust
pub fn wait(&self, context: BreakpointContext, timeout_ms: u64) -> Result<()>;
pub fn list(&self, filter: &BreakpointListFilter) -> Vec<BreakpointSummary>;
pub fn detail(&self, id: u64) -> Result<LiveBreakpointSnapshot>;
pub fn extend(&self, id: u64, timeout_ms: u64) -> Result<BreakpointSummary>;
pub fn release(&self, id: u64) -> Result<()>;
pub fn execute_temporary(&self, id: u64, content: &str) -> Result<TemporaryExecutionResult>;
pub fn release_all(&self);
```

The wait loop recalculates after extension notifications. Removal from the registry happens exactly once on timeout/release. A separate per-breakpoint execution gate serializes temporary runs with final resume.

- [ ] **Step 4: Install `breakpoint(timeoutMs)` into saved Lua executions**

Register a Lua function only when `allow_breakpoint` is true:

```rust
let breakpoint = lua.create_function(move |_, timeout_ms: u64| {
    registry.wait(context.clone(), timeout_ms.min(MAX_BREAKPOINT_MS))
        .map_err(LuaError::external)
})?;
lua.globals().set("breakpoint", breakpoint)?;
```

Before blocking, persist the parent run's current journal and tags, then register the live shared state. On return, leave the shared state intact and continue the same Lua VM after the call.

- [ ] **Step 5: Implement temporary execution against shared state**

Create a fresh safe Lua sandbox, install the same phase globals around the active shared state, omit `breakpoint`, and create a fresh modification journal. Begin and finalize a `Temporary` execution row even for syntax/runtime errors; persist tags after each run; return the completed execution in a success envelope so the UI can display normal Lua errors without treating them as transport failures.

- [ ] **Step 6: Add registry ownership to runtime**

Initialize `BreakpointRegistry` in `ProxyCrab::open`, expose read/action methods, overlay live shared headers/tags/body replacements on stored `CaptureDetail`, and call `release_all()` during proxy shutdown/runtime teardown so blocking workers cannot outlive the application indefinitely.

- [ ] **Step 7: Run registry and Lua tests**

```bash
cargo test -p proxy-crab-mitm breakpoint::tests -- --nocapture
cargo test -p proxy-crab-mitm lua::tests -- --nocapture
```

Expected: timing, cap, release, temp execution, and resume tests pass without flaky wall-clock assertions.

## Task 4: Integrate breakpoints, tags, and `_crab_skip` into proxy flow

**Files:**

- Modify: `crates/proxy-crab-mitm/src/proxy.rs`
- Modify: `crates/proxy-crab-mitm/tests/proxy_integration.rs`

- [ ] **Step 1: Add failing integration tests**

Add tests with a local upstream hit counter:

1. Request script sets `_crab_skip` to an empty string; upstream count stays zero; response interceptor adds a header/body; client receives status 200 and the modified response.
2. `_crab_skip` set and later overwritten remains present and still skips.
3. A non-`_crab_skip` tag reaches response `req:getTag`, persists into log detail, and never appears in upstream headers.
4. A request script pauses, appears in registry, accepts a temporary header/tag mutation, remains paused, releases, and the resumed script reads the new values.
5. A response script pauses after upstream response, accepts repeated temporary response mutations, and returns the final state.
6. Timeout resumes automatically; extension delays resume; total allowance is clipped.
7. Concurrent requests paused on one interceptor produce independent entries and release independently.

- [ ] **Step 2: Move saved Lua execution to blocking workers**

Wrap each saved request/response execution in `tokio::task::spawn_blocking`, passing owned source, metadata, store, registry, and shared state. Map `JoinError` into the existing interceptor error path. Do not hold async locks or semaphore guards inside registry mutex sections.

- [ ] **Step 3: Begin/update execution rows around each saved script**

Before dispatch, create a `Saved` incomplete run containing the initial snapshot. At each breakpoint, update the same row with current modifications. At script exit, apply body replacement, persist tags, and mark the run completed with its final error. Temporary modifications remain only in their independent rows.

- [ ] **Step 4: Refactor response-chain execution into one helper**

Extract one async helper used by both real upstream responses and skip responses:

```rust
async fn run_response_interceptors(
    runtime: Arc<ProxyCrab>,
    store: CaptureStore,
    capture_id: u64,
    request_data: &RequestData,
    response_data: ResponseData,
    response_body: Vec<u8>,
    scripts: &[InterceptorScriptSnapshot],
) -> Result<(ResponseData, Vec<u8>)>;
```

It supplies response scripts with mutable `resp` and tag-only `req`, persists response bodies/history, and preserves current lenient error behavior.

- [ ] **Step 5: Add `_crab_skip` immediately before upstream forwarding**

After all request scripts and capture updates:

```rust
let skipped = request_data.tags.contains_key("_crab_skip");
let (response_data, response_body) = if skipped {
    (
        ResponseData {
            status: 200,
            version: "HTTP/1.1".into(),
            headers: HeaderValues::new(),
        },
        Vec::new(),
    )
} else {
    send_upstream(...).await?
};
```

Pass both branches through the same response helper. Do not translate tags into HTTP headers or otherwise expose them upstream.

- [ ] **Step 6: Run proxy integration tests**

```bash
cargo test -p proxy-crab-mitm --test proxy_integration -- --nocapture
```

Expected: upstream counters, response mutations, persistence, pause controls, and concurrency assertions all pass.

## Task 5: Expose breakpoint management over manager, HTTP, and Tauri

**Files:**

- Modify: `crates/proxy-crab-mgr/src/dto.rs`
- Modify: `crates/proxy-crab-mgr/src/manager.rs`
- Modify: `crates/proxy-crab-mgr/src/http.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/api/types.ts`
- Modify: `src/api/backend.ts`
- Modify: `src/api/tauri-backend.ts`

- [ ] **Step 1: Add failing manager/HTTP tests**

Test standard envelopes, filtering, not-found behavior after timeout/release, clipping, and temporary runtime errors for:

```text
GET  /api/breakpoints?session_id=3&phase=request&interceptor_name=hold
GET  /api/breakpoints/{id}
POST /api/breakpoints/{id}/extend   {"timeout_ms":5000}
POST /api/breakpoints/{id}/release  {}
POST /api/breakpoints/{id}/execute  {"content":"req.headers:set('x-debug','1')"}
```

`session_id` is required by the runtime filter but follows existing manager convention: omission selects the active Session and returns `409 conflict` when none is active. `phase` and `interceptor_name` are optional exact-match filters.

- [ ] **Step 2: Add DTOs and log tags**

Add `tags: BTreeMap<String, String>` to `RequestDetail`, plus:

```rust
pub struct BreakpointQuery {
    pub session_id: Option<u64>,
    pub phase: Option<InterceptorKind>,
    pub interceptor_name: Option<String>,
}

pub struct ExtendBreakpointRequest { pub timeout_ms: u64 }
pub struct ExecuteTemporaryScriptRequest { pub content: String }
pub struct BreakpointDetailPayload {
    pub breakpoint: BreakpointSummary,
    pub log: LogDetail,
}
```

Map missing/expired IDs to `not_found`, invalid phase/query/body to `bad_request`, and keep temporary Lua errors inside `TemporaryExecutionResult.execution.error` with HTTP 200.

- [ ] **Step 3: Extend `Manager` and `MitmManager`**

Add async methods:

```rust
async fn breakpoints(&self, query: BreakpointQuery) -> ManagerResult<Vec<BreakpointSummary>>;
async fn breakpoint(&self, id: u64) -> ManagerResult<BreakpointDetailPayload>;
async fn extend_breakpoint(&self, id: u64, request: ExtendBreakpointRequest)
    -> ManagerResult<BreakpointSummary>;
async fn release_breakpoint(&self, id: u64) -> ManagerResult<()>;
async fn execute_breakpoint_script(
    &self,
    id: u64,
    request: ExecuteTemporaryScriptRequest,
) -> ManagerResult<TemporaryExecutionResult>;
```

- [ ] **Step 4: Add Axum routes and Tauri commands**

Register the five HTTP routes above and matching commands:

```text
list_breakpoints
get_breakpoint
extend_breakpoint
release_breakpoint
execute_breakpoint_script
```

These live-state operations do not emit normal persisted-resource change events; the UI polls them explicitly. Register all commands in `tauri::generate_handler!`.

- [ ] **Step 5: Add TypeScript contracts and backend mappings**

Mirror Rust DTOs using `number`, `Record<string,string>`, and the existing `InterceptorKind`. Add `Backend` methods and invoke mappings with Tauri camelCase argument keys. Extend `Modification` with `tag_set`, `InterceptorExecution` with execution metadata, and `RequestDetail` with tags.

- [ ] **Step 6: Run manager and TypeScript checks**

```bash
cargo test -p proxy-crab-mgr -- --nocapture
pnpm build
```

Expected: all endpoint tests pass and Vue/TypeScript compiles before UI components consume the new methods.

## Task 6: Add breakpoint polling, badges, list, and live controls

**Files:**

- Create: `src/stores/breakpoints.ts`
- Create: `src/windows/BreakpointListWindow.vue`
- Modify: `src/App.vue`
- Modify: `src/components/InterceptorPipeline.vue`
- Modify: `src/windows/LogDetailWindow.vue`
- Modify: `src/windows/ScriptSnapshotWindow.vue`
- Modify: `src/windows/launcher.ts`

- [ ] **Step 1: Implement a current-Session polling store**

Poll every 500 ms while a Session is viewed. Discard stale responses after Session switches and make transient polling failures silent, matching `logsStore` behavior.

Expose:

```ts
count(kind: InterceptorKind, name: string): number;
items(kind: InterceptorKind, name: string): BreakpointSummary[];
refresh(): Promise<void>;
startPolling(): void;
stopPolling(): void;
```

Start/stop it in `App.vue`. Remove expired/released items on every successful snapshot.

- [ ] **Step 2: Render yellow nodes and bounded numeric badges**

In `InterceptorPipeline.vue`, derive the count by phase/name. When positive:

- add a `breakpoint-active` class using `var(--warning)`;
- render centered text `String(Math.min(count, 9))` or `9+`;
- include the active count in tooltip status;
- change primary click from enable/disable toggle to `openBreakpointList`;
- preserve context-menu, drag, and saved-chain behavior.

When the count returns to zero, the node immediately reverts to its normal enabled/disabled/invalid style and click behavior.

- [ ] **Step 3: Build the always-list window**

`BreakpointListWindow.vue` receives `{ sessionId, phase, interceptorName }`, reads the shared store, and lists newest pauses first. Each row displays capture ID, method, URI, phase position, and a one-second-updating remaining countdown. Clicking a row calls `openBreakpointDetail(sessionId, captureId, breakpointId)`.

If all entries expire while the list is open, render `当前没有等待中的请求` rather than closing the window.

- [ ] **Step 4: Add live breakpoint mode to log details**

Extend `LogDetailWindow.vue` with optional `breakpointId`. In live mode, load `getBreakpoint()` instead of `getLog()`, assign `payload.log` to the existing detail renderer, and refresh every 500 ms while active.

Add a fixed breakpoint control area containing:

- remaining/maximum time;
- an extension input in seconds and `延长` button, converted to integer milliseconds;
- `放行` button;
- Monaco Lua editor and `执行临时脚本` button;
- last execution success/error feedback.

Executing a temporary script refreshes the live detail and remains paused. Releasing switches the window to read-only normal-log polling. If timeout/release wins a race and the endpoint returns `not_found`, disable actions, show `断点已结束`, and fall back to `getLog(sessionId, captureId)`.

- [ ] **Step 5: Render tag changes and temporary history**

Add `tag_set: "设置 Tag"` to the existing modification label/detail functions, rendering `key = value` including empty values. Mark temporary execution rows and snapshot titles as `临时脚本`; use `execution_id` in Vue keys and launcher window IDs so repeated temp scripts at the same interceptor position do not collide.

- [ ] **Step 6: Verify frontend build and manual states**

```bash
pnpm build
```

Then run the desktop app against a local delayed request and verify:

1. one paused request shows yellow `1`;
2. ten paused requests show `9+`;
3. clicking yellow always opens the list;
4. list-to-detail navigation shows state only through the current interceptor;
5. extend, repeated temporary execution, release, and timeout update without reopening windows;
6. tag modifications and temporary errors appear in historical interceptor details after completion.

## Task 7: Synchronize docs, skill, fixtures, and evaluations

**Files:**

- Modify: `docs/lua-api.md`
- Modify: `docs/backend-api.md`
- Modify: `skills/proxycrab/SKILL.md`
- Modify: `skills/proxycrab/references/lua-api.md`
- Modify: `skills/proxycrab/references/http-api.md`
- Modify: `skills/proxycrab/evals/evals.json`
- Modify/Create: `skills/proxycrab/evals/fixtures/*.lua`

- [ ] **Step 1: Document Lua contracts and examples**

Document exact signatures and phase availability:

```lua
req:setTag("team", "checkout")
local team = req:getTag("team")
breakpoint(30000)
```

Explain nil/empty/overwrite semantics, persistent but upstream-invisible tags, read-only `entry.req:getTag`, tag-only response `req`, 30-minute clipping, automatic/manual release, temporary-script behavior, normal partial-mutation error behavior, and `_crab_skip` default response.

- [ ] **Step 2: Document all management endpoints**

Add breakpoint query/detail/extend/release/execute request and response examples, the standard error mappings, tags in `request`, `tag_set` history, execution origin/completion fields, Session scoping, and expiration races.

- [ ] **Step 3: Update agent workflow and eval coverage**

Teach the skill to:

- inspect `request.tags` and `tag_set` evidence;
- understand that `_crab_skip` intentionally avoids upstream;
- list a paused interceptor before acting;
- never release, extend, or execute a temporary script unless explicitly requested, because those operations mutate a live request.

Add evaluations for creating/verifying a tag-setting skip interceptor and for inspecting a paused request without accidentally releasing it. Add Lua fixtures that use the exact camelCase method names.

- [ ] **Step 4: Check duplicate references remain synchronized**

```bash
diff -u docs/lua-api.md skills/proxycrab/references/lua-api.md
rg -n "breakpoint|setTag|getTag|_crab_skip|/api/breakpoints|tag_set" \
  docs skills/proxycrab README.md
```

The two Lua references may differ in heading context but must describe identical contracts. Every new endpoint and behavior must be discoverable from both product docs and bundled skill docs.

## Task 8: Full verification and review checkpoint

**Files:** All files above.

- [ ] **Step 1: Format and run complete automated checks**

```bash
cargo fmt --all -- --check
cargo test --workspace -- --nocapture
cargo clippy --workspace --all-targets -- -D warnings
pnpm build
```

Expected: every command exits zero.

- [ ] **Step 2: Run focused regression scenarios**

Verify normal traffic without tags/breakpoints is byte-for-byte behaviorally unchanged, existing interceptor history loads from a migrated workspace, filters/custom columns without tags remain valid, Session switching hides other Sessions' breakpoint badges, and saved-script edits during a pause do not alter the snapshotted execution.

- [ ] **Step 3: Inspect the final diff for scope and safety**

```bash
git status --short
git diff --check
git diff --stat
```

Confirm every changed product file maps to a requirement, no tag is added to upstream headers, no breakpoint action is invoked merely by opening UI, and no unrelated user changes are modified.

- [ ] **Step 4: Prepare the handoff**

Report automated results, manual scenarios, schema migration behavior, management API additions, and any remaining platform-specific UI verification. Do not commit, stage, push, or create a PR unless the user separately requests it.

## Plan self-review checklist

- Requirement coverage: breakpoint timeout/release/extend/temp execution, yellow count badge/list/detail, persistent tags, tag history, filter/column reads, `_crab_skip`, empty default response, response interceptors, Session scoping, and docs/evals each map to an explicit task.
- Simplicity: one registry, one shared-state representation, one response-chain helper, one existing detail renderer; no nested breakpoint support, tag deletion, tag UI, or speculative query language.
- Consistency: API uses `phase`, Lua uses requested camelCase `setTag/getTag`, temporary history uses the same `InterceptorExecution`, and all timeout values cross process boundaries in milliseconds.
- Data safety: migrations preserve existing execution rows; tags default to `{}`; runtime state never enters upstream headers; proxy shutdown releases waiters.
- Race safety: temporary execution is serialized with resume; stale UI responses are Session/version guarded; expired IDs return `not_found` and fall back to normal log detail.
