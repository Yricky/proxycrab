# Session Log View API Refactor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the duplicated network-log and visible-column APIs with three session-aware log operations and one whole-session view operation, then migrate the Vue client and the current default workspace.

**Architecture:** Persist each session's table view in `sessions/<id>/view.json` while keeping column scripts global. Query IDs from SQLite using exclusive ID bounds, batch-render at most 200 updated rows with per-log/per-cell exceptions, and keep full-detail reads separate. Expose the same contracts through the manager trait, HTTP, Tauri commands, and TypeScript backend.

**Tech Stack:** Rust 2024, rusqlite, Tokio, Axum, Tauri 2, Vue 3, TypeScript.

---

### Task 1: Persist session views and expose bounded capture queries

**Files:**
- Modify: `crates/proxy-crab-mitm/src/model.rs`
- Modify: `crates/proxy-crab-mitm/src/workspace.rs`
- Modify: `crates/proxy-crab-mitm/src/runtime.rs`
- Modify: `crates/proxy-crab-mitm/src/storage.rs`
- Test: unit tests in the same Rust modules

- [ ] **Step 1: Write failing storage/workspace tests**

Add tests proving that a new session clones the active session view, falls back to:

```rust
vec![
    Column::Method { width: 50.0 },
    Column::Uri { width: 300.0 },
    Column::Code { width: 50.0 },
    Column::Source { width: 100.0 },
]
```

and that capture ID queries apply exclusive `min_id`/`max_id` bounds in the requested direction.

- [ ] **Step 2: Run the focused tests and verify failure**

Run:

```bash
cargo test -p proxy-crab-mitm workspace::tests storage::tests
```

Expected: compilation or assertion failure because session views and bounded queries do not exist yet.

- [ ] **Step 3: Move columns out of `AppConfig` and add session view persistence**

Define:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionView {
    #[serde(default = "default_columns")]
    pub columns: Vec<Column>,
}
```

Make `default_columns` public within the crate. Store each view atomically at `sessions/<id>/view.json`. On session creation, clone the active session view when readable; otherwise use `SessionView::default()`. Existing sessions without `view.json` read the fixed default without automatic migration.

- [ ] **Step 4: Add efficient storage/runtime methods**

Add methods equivalent to:

```rust
pub fn list_ids(
    &self,
    limit: usize,
    min_id: Option<u64>,
    max_id: Option<u64>,
) -> Result<Vec<u64>>;

pub fn get_summaries(&self, ids: &[u64]) -> Result<HashMap<u64, CaptureSummary>>;
```

Use `id > min_id` and `id < max_id`. Query descending when paging older/latest and ascending when only `min_id` is supplied. Batch reads must avoid one SQLite connection/query per ID. Add a body-write update that advances `updated_at` in the same operation sequence.

- [ ] **Step 5: Update script rename/delete behavior**

For a column-script rename, rewrite matching references in every session view. For delete, remove only the script file and deliberately leave stale view references. Reject a newly written view if any referenced script does not exist.

- [ ] **Step 6: Run focused tests**

Run:

```bash
cargo test -p proxy-crab-mitm
```

Expected: all `proxy-crab-mitm` tests pass.

### Task 2: Replace manager DTOs and operations

**Files:**
- Modify: `crates/proxy-crab-mgr/src/dto.rs`
- Modify: `crates/proxy-crab-mgr/src/manager.rs`
- Test: unit tests in `crates/proxy-crab-mgr/src/manager.rs`

- [ ] **Step 1: Add contract tests for validation and rendering**

Cover the 10,000 default/max ID limit, 200-item batch-view limit, strict `server.updated_at > client.updated_at`, missing logs, missing scripts, per-cell Lua failures, and a response whose columns/cells exclude the ID column.

- [ ] **Step 2: Replace legacy DTOs**

Use these shapes:

```rust
pub struct LogIdsRequest {
    pub session_id: Option<u64>,
    pub filter: Option<String>,
    pub min_id: Option<u64>,
    pub max_id: Option<u64>,
    pub limit: Option<usize>,
}

pub struct LogIdsPayload {
    pub ids: Vec<u64>,
}

pub struct LogViewItem {
    pub id: u64,
    pub updated_at: Option<u64>,
}

pub struct LogViewsRequest {
    pub session_id: Option<u64>,
    pub logs: Vec<LogViewItem>,
    pub view: Option<SessionViewInput>,
}

pub struct LogViewRow {
    pub id: u64,
    pub updated_at: u64,
    pub cells: Vec<String>,
}

pub struct LogViewException {
    pub id: u64,
    pub column_index: Option<usize>,
    pub code: String,
    pub message: String,
}

pub struct LogViewsPayload {
    pub columns: Vec<ColumnView>,
    pub rows: Vec<LogViewRow>,
    pub exceptions: Vec<LogViewException>,
}
```

Add `created_at` and `updated_at` to `LogDetail`. Add whole-session view request/response DTOs.

- [ ] **Step 3: Replace manager trait methods**

Replace `logs`, `filter_logs`, and visible-column mutation methods with:

```rust
async fn log_ids(&self, request: LogIdsRequest) -> ManagerResult<LogIdsPayload>;
async fn log_views(&self, request: LogViewsRequest) -> ManagerResult<LogViewsPayload>;
async fn log(&self, session_id: Option<u64>, id: u64) -> ManagerResult<LogDetail>;
async fn session_view(&self, session_id: Option<u64>) -> ManagerResult<SessionViewPayload>;
async fn replace_session_view(
    &self,
    session_id: Option<u64>,
    request: ReplaceSessionViewRequest,
) -> ManagerResult<SessionViewPayload>;
```

- [ ] **Step 4: Implement ID filtering and batch rendering**

Compile/evaluate the optional Lua filter over bounded summaries. Deduplicate requested view IDs using the last supplied timestamp. Return unchanged IDs nowhere. Return `log_not_found` without a row; return rows with empty failing cells plus `column_script_error` carrying the matching zero-based `column_index`.

- [ ] **Step 5: Run manager tests**

Run:

```bash
cargo test -p proxy-crab-mgr manager
```

Expected: manager contract tests pass.

### Task 3: Replace HTTP and Tauri surfaces

**Files:**
- Modify: `crates/proxy-crab-mgr/src/http.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `crates/proxy-crab-mgr/src/http.rs`

- [ ] **Step 1: Write failing route tests**

Assert that these routes exist:

```text
POST /api/logs/ids
POST /api/logs/views
GET  /api/logs/{id}?session_id=<optional>
GET  /api/session-view?session_id=<optional>
PUT  /api/session-view?session_id=<optional>
```

Assert that legacy `/api/sessions/{id}/logs*`, `GET /api/logs`, `POST /api/logs/filter`, and `/api/columns*` return 404.

- [ ] **Step 2: Replace Axum handlers**

Deserialize `session_id` from JSON for both POST routes and from query for detail/view routes. Preserve the standard `{ok,data}` / `{ok,error}` envelope.

- [ ] **Step 3: Replace Tauri commands**

Register:

```rust
get_log_ids
get_log_views
get_log
get_session_view
replace_session_view
```

Remove `list_logs`, `filter_logs`, and all visible-column commands. Keep global column-script CRUD.

- [ ] **Step 4: Run HTTP and workspace Rust tests**

Run:

```bash
cargo test --workspace
```

Expected: all Rust tests pass.

### Task 4: Migrate the Vue backend and log store

**Files:**
- Modify: `src/api/types.ts`
- Modify: `src/api/backend.ts`
- Modify: `src/api/tauri-backend.ts`
- Modify: `src/stores/logs.ts`
- Modify: `src/components/LogTable.vue`

- [ ] **Step 1: Replace TypeScript wire types**

Mirror the Rust DTOs exactly, including:

```ts
interface LogViewException {
  id: number;
  column_index?: number | null;
  code: "column_script_error" | "log_not_found" | string;
  message: string;
}
```

Remove legacy `LogsQuery`, `FilterLogsRequest`, `LogsPayload`, and `ColumnInput`.

- [ ] **Step 2: Replace backend methods**

Expose:

```ts
getLogIds(request: LogIdsRequest): Promise<LogIdsPayload>;
getLogViews(request: LogViewsRequest): Promise<LogViewsPayload>;
getLog(sessionId: number | null, id: number): Promise<LogDetail>;
getSessionView(sessionId: number | null): Promise<SessionViewPayload>;
replaceSessionView(
  sessionId: number | null,
  request: ReplaceSessionViewRequest,
): Promise<SessionViewPayload>;
```

- [ ] **Step 3: Refactor polling into ID discovery plus 200-row hydration**

On session/filter reset, fetch the newest ID page. Poll newer IDs with `min_id`; fetch older IDs with `max_id` when the scroll reaches the old-record edge. Split hydration into chunks of at most 200 and pass known row timestamps so unchanged rows are omitted.

- [ ] **Step 4: Render fixed ID column and cell errors**

Keep ID outside `columns`/`cells`, prepend it only in the table template, and maintain errors keyed by `id:column_index`. Render an empty errored cell with an error class and use the exception message as its `title`.

- [ ] **Step 5: Preserve detail-window user changes**

Only adapt the detail DTO usage required by the new timestamps. Do not overwrite the pre-existing uncommitted edits in `src/windows/LogDetailWindow.vue`.

### Task 5: Convert the column manager to whole-session view editing

**Files:**
- Modify: `src/windows/ColumnManagerWindow.vue`
- Modify: `src/components/LogTable.vue`

- [ ] **Step 1: Load the viewed session's whole view**

Use `sessionsStore.viewingSessionId` and `getSessionView`. Continue loading global scripts through column-script CRUD.

- [ ] **Step 2: Save each UI edit as a whole replacement**

For add, remove, reorder/width changes, build the complete `columns` array and call:

```ts
await backend.replaceSessionView(sessionId, { columns: nextColumns });
```

When resizing in the main table, replace the same session view rather than addressing a global column index.

- [ ] **Step 3: Surface stale-script rendering errors**

Leave deleted script references visible in the view editor. The log table displays the returned `column_script_error`; selecting a valid script or deleting the column repairs the view.

- [ ] **Step 4: Run the frontend build**

Run:

```bash
npm run build
```

Expected: `vue-tsc --noEmit` and Vite build both succeed.

### Task 6: Update public documentation and verify

**Files:**
- Modify: `docs/backend-api.md`
- Modify: `~/.agents/skills/proxycrab/references/api.md` only if explicitly in scope after repository verification

- [ ] **Step 1: Rewrite the repository API reference**

Document the three log endpoints, exclusive bounds, 10,000/200 limits, timestamp semantics, exception codes, optional session fallback, session view replacement, and retained global column scripts.

- [ ] **Step 2: Search for stale contracts**

Run:

```bash
rg -n 'logs/filter|sessions/.*/logs|/api/columns|listLogs|filterLogs|listColumns|appendColumn|replaceColumn|deleteColumn' \
  crates src src-tauri docs
```

Expected: no active code or current documentation references to removed contracts.

- [ ] **Step 3: Run formatting and complete verification**

Run:

```bash
cargo fmt --all -- --check
cargo test --workspace
npm run build
git diff --check
```

Expected: every command exits successfully.

### Task 7: Update the default workspace safely

**Files:**
- Back up and modify the resolved default workspace's `app_config.json`
- Create or modify `sessions/<active-id>/view.json`
- Create missing `view.json` files for other sessions

- [ ] **Step 1: Resolve and inspect the workspace**

Read the Tauri app-data pointer and current workspace files without changing them. Verify the active session exists and record the legacy `columns`.

- [ ] **Step 2: Create a timestamped recoverable backup**

Copy the relevant JSON files into a sibling backup directory with an explicit timestamp. Do not modify capture databases or body blobs.

- [ ] **Step 3: Apply the confirmed one-time data update**

Write the legacy global columns into the active session's `view.json`; write fixed defaults for other sessions; remove the obsolete top-level `columns` field from `app_config.json`.

- [ ] **Step 4: Validate the resulting JSON and application model**

Parse every changed JSON file and, when the app is not locking the workspace, run a read-only application/runtime check against it. Report the backup location.
