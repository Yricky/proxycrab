# Session Interceptor Pipeline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the workspace-global interceptor activation model with ordered, per-session request and response chains, add a compact interactive pipeline above the filter bar, and persist the exact interceptor source used by every capture without duplicating identical source text.

**Architecture:** Request and response Lua files remain global libraries, while each session owns an `interceptors.json` file containing up to twelve unique bindings per side. The proxy resolves enabled bindings into immutable name/content/hash snapshots when a request starts, uses those same snapshots through the response phase, and stores each attempted execution in the session database with content deduplicated by SHA-256. The management API exposes global script-library CRUD separately from whole-session chain reads and replacements.

**Tech Stack:** Rust 2024, rusqlite, serde, SHA-256, Axum, Tauri 2, Vue 3, TypeScript, Monaco Editor, vue-icons-plus.

---

## Product and interaction contract

- Request and response interceptor files remain separate global libraries.
- Every session begins with empty request and response chains.
- Each chain contains at most 12 unique script references, including disabled and missing references.
- Chain entries have three UI states:
  - enabled and present: solid green dot;
  - disabled: neutral gray dot;
  - enabled or disabled but missing on disk: red hollow dot with an exclamation mark.
- Disabled and missing entries are skipped and create no execution record. Missing enabled entries write a system warning.
- Clicking a present dot toggles its session-local enabled state.
- Dragging reorders only within the same request or response chain.
- Right-clicking a node provides edit, add before, add after, and remove. Missing nodes additionally provide recreate-with-the-same-name.
- Adding uses a searchable picker that excludes scripts already in the chain.
- Endpoint and empty-chain areas can add the first interceptor.
- The displayed chain follows the session shown by the log table, even when that session is not active for capture.
- Edits to a viewed inactive session are persisted immediately but affect traffic only after that session becomes active.
- A request snapshots enabled, present scripts at its start. In-flight traffic is unaffected by later edits, deletion, reordering, toggles, or session activation changes.
- Global rename rewrites every session reference. Application-driven delete removes all references after confirmation. Files manually removed outside the application remain as missing references.
- Historical execution names never change after a global rename.
- The pipeline is a single horizontally scrollable line above `FilterBar`.
- Static semantic vector icons represent the originating device and upstream server; no emoji are used.

## Visual thesis

A restrained network-topology strip on the existing calm application surface: compact endpoints, fine directional arrows, and status dots whose color has one unambiguous meaning.

## Content plan

1. Device endpoint and request chain.
2. Upstream server endpoint.
3. Response chain and returning device endpoint.
4. Contextual tooltip, searchable picker, and empty-session message only when needed.

## Interaction thesis

- Dots use a quick 120–160 ms color/scale transition to make toggles legible.
- Dragging lifts the active node and reveals a narrow insertion marker without moving it across the server boundary.
- Tooltip and picker use a short opacity/translate reveal and are clamped to the application viewport.

---

### Task 1: Define and persist per-session interceptor chains

**Files:**
- Modify: `crates/proxy-crab-mitm/src/model.rs`
- Modify: `crates/proxy-crab-mitm/src/workspace.rs`
- Test: `crates/proxy-crab-mitm/src/workspace.rs`

- [ ] **Step 1: Write failing workspace tests**

Add tests proving that new sessions get empty chains, a missing file reads as empty, and replacement round-trips disabled and missing references:

```rust
#[test]
fn new_session_has_empty_interceptor_chains() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(temp.path()).unwrap();
    let session = workspace.create_session(Some("one".into()), None).unwrap();
    assert_eq!(
        workspace.session_interceptors(session.id).unwrap(),
        SessionInterceptors::default()
    );
}

#[test]
fn session_interceptors_round_trip_without_resolving_script_files() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(temp.path()).unwrap();
    let session = workspace.create_session(Some("one".into()), None).unwrap();
    let value = SessionInterceptors {
        request: vec![
            SessionInterceptor {
                name: "present".into(),
                enabled: true,
            },
            SessionInterceptor {
                name: "manually-removed".into(),
                enabled: false,
            },
        ],
        response: vec![],
    };
    workspace
        .replace_session_interceptors(session.id, value.clone())
        .unwrap();
    assert_eq!(workspace.session_interceptors(session.id).unwrap(), value);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
cargo test -p proxy-crab-mitm workspace::tests::new_session_has_empty_interceptor_chains workspace::tests::session_interceptors_round_trip_without_resolving_script_files
```

Expected: compilation fails because `SessionInterceptors` and workspace methods do not exist.

- [ ] **Step 3: Add the session chain model**

Remove `active_request_interceptors` and `active_response_interceptors` from `AppConfig`. Add:

```rust
pub const MAX_SESSION_INTERCEPTORS_PER_KIND: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionInterceptor {
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionInterceptors {
    #[serde(default)]
    pub request: Vec<SessionInterceptor>,
    #[serde(default)]
    pub response: Vec<SessionInterceptor>,
}
```

Keep `AppConfig` limited to proxy/API settings, `active_session_id`, and `filter_history`.

- [ ] **Step 4: Add workspace persistence**

In `Workspace::create_session`, write `interceptors.json` with `SessionInterceptors::default()`. Add:

```rust
pub fn session_interceptors(&self, id: u64) -> Result<SessionInterceptors> {
    self.require_session(id)?;
    let path = self.session_dir(id).join("interceptors.json");
    match read_json(&path) {
        Ok(value) => Ok(value),
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            Ok(SessionInterceptors::default())
        }
        Err(error) => Err(error),
    }
}

pub fn replace_session_interceptors(
    &self,
    id: u64,
    value: SessionInterceptors,
) -> Result<SessionInterceptors> {
    self.require_session(id)?;
    write_json_atomic(
        &self.session_dir(id).join("interceptors.json"),
        &value,
    )?;
    Ok(value)
}
```

- [ ] **Step 5: Run workspace tests**

Run:

```bash
cargo test -p proxy-crab-mitm workspace
```

Expected: all workspace tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/proxy-crab-mitm/src/model.rs crates/proxy-crab-mitm/src/workspace.rs
git commit -m "feat: persist interceptor chains per session"
```

---

### Task 2: Implement session chain validation and global reference maintenance

**Files:**
- Modify: `crates/proxy-crab-mitm/src/model.rs`
- Modify: `crates/proxy-crab-mitm/src/runtime.rs`
- Modify: `crates/proxy-crab-mitm/src/workspace.rs`
- Test: `crates/proxy-crab-mitm/src/runtime.rs`

- [ ] **Step 1: Write failing runtime tests**

Cover maximum length, duplicate rejection, missing-reference preservation, rename propagation, delete cleanup, and usage counts:

```rust
#[test]
fn session_interceptor_chains_enforce_limit_and_uniqueness() {
    let runtime = test_runtime();
    let session = runtime.create_session(Some("one".into()), None).unwrap();
    let duplicate = SessionInterceptors {
        request: vec![
            SessionInterceptor { name: "a".into(), enabled: true },
            SessionInterceptor { name: "a".into(), enabled: false },
        ],
        response: vec![],
    };
    assert!(runtime.replace_session_interceptors(session.id, duplicate).is_err());

    let too_many = SessionInterceptors {
        request: (0..=MAX_SESSION_INTERCEPTORS_PER_KIND)
            .map(|index| SessionInterceptor {
                name: format!("script-{index}"),
                enabled: true,
            })
            .collect(),
        response: vec![],
    };
    assert!(runtime.replace_session_interceptors(session.id, too_many).is_err());
}

#[test]
fn interceptor_rename_and_delete_update_every_session() {
    let runtime = test_runtime();
    let first = runtime.create_session(Some("one".into()), None).unwrap();
    let second = runtime.create_session(Some("two".into()), None).unwrap();
    runtime
        .create_script(
            ScriptKind::RequestInterceptor,
            Script { name: "old".into(), content: String::new() },
        )
        .unwrap();
    for id in [first.id, second.id] {
        runtime
            .replace_session_interceptors(
                id,
                SessionInterceptors {
                    request: vec![SessionInterceptor {
                        name: "old".into(),
                        enabled: true,
                    }],
                    response: vec![],
                },
            )
            .unwrap();
    }
    runtime
        .update_script(
            ScriptKind::RequestInterceptor,
            "old",
            Script { name: "new".into(), content: String::new() },
        )
        .unwrap();
    assert!(runtime.session_interceptors(first.id).unwrap().request[0].name == "new");
    runtime
        .delete_script(ScriptKind::RequestInterceptor, "new")
        .unwrap();
    assert!(runtime.session_interceptors(first.id).unwrap().request.is_empty());
    assert!(runtime.session_interceptors(second.id).unwrap().request.is_empty());
}
```

- [ ] **Step 2: Run the targeted tests to verify they fail**

Run:

```bash
cargo test -p proxy-crab-mitm session_interceptor
```

Expected: compilation fails because runtime chain methods do not exist.

- [ ] **Step 3: Add resolved chain and library models**

Add:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResolvedSessionInterceptor {
    pub name: String,
    pub enabled: bool,
    pub valid: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResolvedSessionInterceptors {
    pub session_id: u64,
    pub request: Vec<ResolvedSessionInterceptor>,
    pub response: Vec<ResolvedSessionInterceptor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InterceptorLibraryItem {
    pub name: String,
    pub usage_count: usize,
}
```

- [ ] **Step 4: Implement validation and resolution**

Add runtime methods that:

1. reject more than 12 entries per side;
2. reject duplicate names separately within each side;
3. accept missing names so manually deleted files remain representable;
4. resolve `valid` by checking the relevant global script directory;
5. atomically replace the session JSON only after validation succeeds.

Use a helper with this contract:

```rust
fn validate_session_interceptors(value: &SessionInterceptors) -> Result<()> {
    for (label, entries) in [
        ("request", &value.request),
        ("response", &value.response),
    ] {
        if entries.len() > MAX_SESSION_INTERCEPTORS_PER_KIND {
            bail!("{label} interceptor chain cannot contain more than 12 entries");
        }
        let mut names = std::collections::HashSet::new();
        if entries.iter().any(|entry| !names.insert(entry.name.as_str())) {
            bail!("{label} interceptor chain contains duplicate scripts");
        }
    }
    Ok(())
}
```

- [ ] **Step 5: Replace global config reference updates**

Change interceptor script rename to rewrite every `interceptors.json`, preserving order and enabled state. Change application-driven delete to remove matching references from every session before deleting the file, with rollback if a write or delete fails. Add `interceptor_library(kind)` that counts sessions containing each script name.

- [ ] **Step 6: Run runtime tests**

Run:

```bash
cargo test -p proxy-crab-mitm runtime
```

Expected: all runtime tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/proxy-crab-mitm/src/model.rs crates/proxy-crab-mitm/src/runtime.rs crates/proxy-crab-mitm/src/workspace.rs
git commit -m "feat: manage per-session interceptor references"
```

---

### Task 3: Store immutable interceptor execution history

**Files:**
- Modify: `crates/proxy-crab-mitm/src/model.rs`
- Modify: `crates/proxy-crab-mitm/src/storage.rs`
- Test: `crates/proxy-crab-mitm/src/storage.rs`

- [ ] **Step 1: Write failing storage tests**

Add a test that records the same content for two executions, verifies one source row and two run rows, and reads ordered historical content:

```rust
#[test]
fn interceptor_source_is_deduplicated_and_runs_are_ordered() {
    let temp = tempfile::tempdir().unwrap();
    let store = CaptureStore::open(7, temp.path()).unwrap();
    let request = RequestData {
        method: "GET".into(),
        uri: "http://example.com/".into(),
        version: "HTTP/1.1".into(),
        headers: HeaderValues::new(),
    };
    let id = store.begin("127.0.0.1", &request, "request").unwrap();
    let source = "req.headers:set(\"x-test\", \"1\")";
    let hash = script_content_hash(source);
    for (position, name) in ["first", "second"].into_iter().enumerate() {
        store
            .record_interceptor_run(
                id,
                &InterceptorRun {
                    phase: InterceptorKind::Request,
                    position,
                    name: name.into(),
                    script_hash: hash.clone(),
                    content: source.into(),
                    modifications: vec![],
                    error: None,
                },
            )
            .unwrap();
    }
    let detail = store.get(id).unwrap().unwrap();
    assert_eq!(detail.request_interceptors.len(), 2);
    assert_eq!(detail.request_interceptors[0].name, "first");
    assert_eq!(detail.request_interceptors[0].content, source);
    assert_eq!(store.interceptor_source_count().unwrap(), 1);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run:

```bash
cargo test -p proxy-crab-mitm interceptor_source_is_deduplicated_and_runs_are_ordered
```

Expected: compilation fails because the execution models and store methods do not exist.

- [ ] **Step 3: Add execution models and hashing**

Add:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InterceptorExecution {
    pub phase: InterceptorKind,
    pub position: usize,
    pub name: String,
    pub script_hash: String,
    pub content: String,
    pub modifications: Vec<Modification>,
    pub error: Option<String>,
}

pub struct InterceptorRun {
    pub phase: InterceptorKind,
    pub position: usize,
    pub name: String,
    pub script_hash: String,
    pub content: String,
    pub modifications: Vec<Modification>,
    pub error: Option<String>,
}

pub fn script_content_hash(content: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(content.as_bytes()))
}
```

Change `CaptureDetail` to return:

```rust
pub request_interceptors: Vec<InterceptorExecution>,
pub response_interceptors: Vec<InterceptorExecution>,
```

Remove aggregate request/response modification fields from the public detail model; each execution owns its modifications.

- [ ] **Step 4: Add normalized SQLite tables**

Extend initialization with:

```sql
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS interceptor_script_contents (
    hash TEXT PRIMARY KEY,
    content TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS capture_interceptor_runs (
    capture_id INTEGER NOT NULL,
    phase TEXT NOT NULL CHECK (phase IN ('request', 'response')),
    position INTEGER NOT NULL,
    name TEXT NOT NULL,
    script_hash TEXT NOT NULL,
    modifications TEXT NOT NULL DEFAULT '[]',
    error TEXT,
    PRIMARY KEY (capture_id, phase, position),
    FOREIGN KEY (capture_id) REFERENCES captures(id) ON DELETE CASCADE,
    FOREIGN KEY (script_hash) REFERENCES interceptor_script_contents(hash)
);
CREATE INDEX IF NOT EXISTS capture_interceptor_runs_capture
ON capture_interceptor_runs(capture_id, phase, position);
```

Keep existing aggregate modification columns in `captures` physically so existing databases can be manually adjusted without a runtime migration. Stop writing and reading those columns after this task.

- [ ] **Step 5: Implement atomic run persistence**

`record_interceptor_run` must open one transaction, insert `(hash, content)` with `ON CONFLICT(hash) DO NOTHING`, verify an existing hash maps to the same content, insert the ordered run, bump `captures.updated_at`, and commit. Reading detail joins runs to content and orders by phase then position.

- [ ] **Step 6: Run storage tests**

Run:

```bash
cargo test -p proxy-crab-mitm storage
```

Expected: all storage tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/proxy-crab-mitm/src/model.rs crates/proxy-crab-mitm/src/storage.rs
git commit -m "feat: persist interceptor execution snapshots"
```

---

### Task 4: Snapshot and execute session chains in the proxy

**Files:**
- Modify: `crates/proxy-crab-mitm/src/proxy.rs`
- Modify: `crates/proxy-crab-mitm/src/runtime.rs`
- Modify: `crates/proxy-crab-mitm/tests/proxy_integration.rs`
- Test: `crates/proxy-crab-mitm/tests/proxy_integration.rs`

- [ ] **Step 1: Write failing integration tests**

Add tests proving:

- different sessions run different request chains;
- response uses the content captured when the request began even if the file is edited before the upstream responds;
- disabled and missing entries are skipped;
- a runtime error records name, source hash, source content, partial modifications, and error;
- scripts with no modifications still create execution rows.

Use a delayed local upstream and mutate the response interceptor after the request reaches that upstream. Assert the response contains the header from the old source and the capture detail contains that old source.

- [ ] **Step 2: Run targeted integration tests to verify failure**

Run:

```bash
cargo test -p proxy-crab-mitm --test proxy_integration session_interceptor
```

Expected: tests fail because traffic still reads workspace-global lists and no run history exists.

- [ ] **Step 3: Add immutable runtime snapshots**

Add:

```rust
#[derive(Clone)]
struct InterceptorSnapshot {
    name: String,
    hash: String,
    content: String,
}

#[derive(Clone, Default)]
struct SessionInterceptorSnapshot {
    request: Vec<InterceptorSnapshot>,
    response: Vec<InterceptorSnapshot>,
}
```

Resolve the active session and its enabled entries once at request start. Read the script content immediately, calculate SHA-256, and retain it in the request task. Skip disabled entries. Skip missing files and emit a warning containing session ID, phase, and script name.

- [ ] **Step 4: Record every attempted execution**

For each request and response snapshot:

1. execute the captured source;
2. apply partial effects even when Lua reports a runtime error, preserving current lenient behavior;
3. persist `InterceptorRun` with its per-script modifications and error;
4. continue to the next interceptor;
5. aggregate modifications only in memory when needed to decide whether a modified body file must be written.

Response execution must use the snapshot created at request start.

- [ ] **Step 5: Run integration tests**

Run:

```bash
cargo test -p proxy-crab-mitm --test proxy_integration
```

Expected: all proxy integration tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/proxy-crab-mitm/src/proxy.rs crates/proxy-crab-mitm/src/runtime.rs crates/proxy-crab-mitm/tests/proxy_integration.rs
git commit -m "feat: execute immutable session interceptor snapshots"
```

---

### Task 5: Replace management and Tauri APIs

**Files:**
- Modify: `crates/proxy-crab-mgr/src/dto.rs`
- Modify: `crates/proxy-crab-mgr/src/manager.rs`
- Modify: `crates/proxy-crab-mgr/src/http.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `docs/backend-api.md`
- Test: `crates/proxy-crab-mgr/src/manager.rs`
- Test: `crates/proxy-crab-mgr/src/http.rs`

- [ ] **Step 1: Write failing manager and HTTP tests**

Add tests for:

- `GET /api/interceptors?kind=request` returning only global items with usage counts;
- `GET /api/session-interceptors?session_id=<id>` returning resolved validity;
- `PUT /api/session-interceptors?session_id=<id>` replacing both chains;
- duplicate and 13-item chains returning `bad_request`;
- old `/enable`, `/disable`, and `/order` routes returning 404;
- rename propagating references and delete removing references.

- [ ] **Step 2: Run manager tests to verify failure**

Run:

```bash
cargo test -p proxy-crab-mgr
```

Expected: new route and payload tests fail.

- [ ] **Step 3: Replace interceptor DTOs**

Use these wire contracts:

```rust
pub struct InterceptorLibraryList {
    pub kind: InterceptorKind,
    pub items: Vec<InterceptorLibraryItem>,
}

pub struct SessionInterceptorInput {
    pub name: String,
    pub enabled: bool,
}

pub struct ReplaceSessionInterceptorsRequest {
    pub request: Vec<SessionInterceptorInput>,
    pub response: Vec<SessionInterceptorInput>,
}

pub struct SessionInterceptorItem {
    pub name: String,
    pub enabled: bool,
    pub valid: bool,
}

pub struct SessionInterceptorsPayload {
    pub session_id: u64,
    pub request: Vec<SessionInterceptorItem>,
    pub response: Vec<SessionInterceptorItem>,
}
```

Remove `enabled` from interceptor create/update/detail DTOs and remove `SetInterceptorOrderRequest`.

- [ ] **Step 4: Replace manager trait methods**

Remove `set_interceptor_enabled` and `set_interceptor_order`. Add:

```rust
async fn session_interceptors(
    &self,
    session_id: Option<u64>,
) -> ManagerResult<SessionInterceptorsPayload>;

async fn replace_session_interceptors(
    &self,
    session_id: Option<u64>,
    request: ReplaceSessionInterceptorsRequest,
) -> ManagerResult<SessionInterceptorsPayload>;
```

Map limit and duplicate validation failures to `bad_request`, missing sessions to `not_found`, and filesystem write failures to `internal_error`.

- [ ] **Step 5: Replace HTTP and Tauri routes**

Keep:

- `GET/POST /api/interceptors`
- `GET/PUT/DELETE /api/interceptors/{kind}/{name}`

Add:

- `GET/PUT /api/session-interceptors?session_id=<id>`

Remove:

- `/api/interceptors/{kind}/{name}/enable`
- `/api/interceptors/{kind}/{name}/disable`
- `/api/interceptors/order`

Expose Tauri commands:

- `get_session_interceptors`
- `replace_session_interceptors`

Remove the old enable/order Tauri commands.

- [ ] **Step 6: Return execution history in log detail**

Map each core `InterceptorExecution` to a DTO containing phase, position, historical name, SHA-256 hash, content, modifications, and optional error. Replace aggregate `req_modifications` and `resp_modifications` in `LogDetail` with `request_interceptors` and `response_interceptors`.

- [ ] **Step 7: Update API documentation**

Document the new global library/session chain split, exact JSON payloads, limit and duplicate errors, missing-reference behavior, snapshot timing, and historical execution fields. Remove all old global enable/order documentation.

- [ ] **Step 8: Run manager tests**

Run:

```bash
cargo test -p proxy-crab-mgr
```

Expected: all manager and HTTP tests pass.

- [ ] **Step 9: Commit**

```bash
git add crates/proxy-crab-mgr/src/dto.rs crates/proxy-crab-mgr/src/manager.rs crates/proxy-crab-mgr/src/http.rs src-tauri/src/lib.rs docs/backend-api.md
git commit -m "feat: expose session interceptor APIs"
```

---

### Task 6: Add frontend data types and a session interceptor store

**Files:**
- Modify: `src/api/types.ts`
- Modify: `src/api/backend.ts`
- Modify: `src/api/tauri-backend.ts`
- Create: `src/stores/interceptors.ts`
- Modify: `src/stores/sessions.ts`

- [ ] **Step 1: Replace TypeScript contracts**

Remove global active arrays from `AppConfig`, remove `InterceptorInfo`/`InterceptorList`/`SetInterceptorOrderRequest`, and add:

```ts
export interface InterceptorLibraryItem {
  name: string;
  usage_count: number;
}

export interface InterceptorLibraryList {
  kind: InterceptorKind;
  items: InterceptorLibraryItem[];
}

export interface SessionInterceptorItem {
  name: string;
  enabled: boolean;
  valid: boolean;
}

export interface SessionInterceptorsPayload {
  session_id: number;
  request: SessionInterceptorItem[];
  response: SessionInterceptorItem[];
}

export interface ReplaceSessionInterceptorsRequest {
  request: Array<{ name: string; enabled: boolean }>;
  response: Array<{ name: string; enabled: boolean }>;
}

export interface InterceptorExecution {
  phase: InterceptorKind;
  position: number;
  name: string;
  script_hash: string;
  content: string;
  modifications: Modification[];
  error: string | null;
}
```

- [ ] **Step 2: Replace backend methods**

Expose:

```ts
listInterceptors(kind: InterceptorKind): Promise<InterceptorLibraryList>;
getSessionInterceptors(sessionId: number | null): Promise<SessionInterceptorsPayload>;
replaceSessionInterceptors(
  sessionId: number | null,
  request: ReplaceSessionInterceptorsRequest,
): Promise<SessionInterceptorsPayload>;
```

Delete `setInterceptorEnabled` and `setInterceptorOrder`. Remove `enabled` from interceptor create/update payloads.

- [ ] **Step 3: Add a focused reactive store**

`src/stores/interceptors.ts` owns only:

- the currently loaded `sessionId`;
- request/response arrays;
- loading/saving flags;
- `refresh(sessionId)`;
- atomic `replace(sessionId, request, response)`;
- `toggle`, `move`, `insert`, and `remove` helpers that clone arrays before persisting;
- stale-response protection so a slow load for the previous session cannot overwrite the current one.

Every mutation reports backend errors through `reportError` and restores the last confirmed payload on failure.

- [ ] **Step 4: Refresh on viewed-session changes**

When `sessionsStore.view(id)` changes the viewed session, trigger the interceptor store refresh from the pipeline component watcher rather than coupling the session store to interceptor details. A newly created session naturally loads as an empty chain.

- [ ] **Step 5: Run TypeScript checking**

Run:

```bash
pnpm exec vue-tsc --noEmit
```

Expected: type checking passes.

- [ ] **Step 6: Commit**

```bash
git add src/api/types.ts src/api/backend.ts src/api/tauri-backend.ts src/stores/interceptors.ts src/stores/sessions.ts
git commit -m "feat: add session interceptor frontend state"
```

---

### Task 7: Build the compact pipeline, tooltip, picker, and drag interaction

**Files:**
- Create: `src/components/AppTooltip.vue`
- Create: `src/components/InterceptorPicker.vue`
- Create: `src/components/InterceptorPipeline.vue`
- Modify: `src/App.vue`
- Modify: `src/styles/base.css`
- Modify: `src/windows/launcher.ts`

- [ ] **Step 1: Build the viewport-aware tooltip**

Create `AppTooltip.vue` as a wrapper that teleports a compact tooltip to `body`, opens after a short hover delay, clamps its position to an 8 px viewport inset, and closes on pointer leave, click, scroll, or unmount. It displays name, type, order, and status using existing theme tokens.

- [ ] **Step 2: Build the searchable picker**

Create `InterceptorPicker.vue` with:

- kind-specific library loading;
- case-insensitive search;
- already-used scripts removed;
- keyboard Escape close and Enter select;
- click-outside close;
- a disabled explanatory state at 12 entries;
- a compact empty result state;
- viewport-clamped placement next to the chosen insertion point.

- [ ] **Step 3: Build the pipeline topology**

Use vector icons from `vue-icons-plus/io5`, including directional arrows, a device icon, a server icon, add, edit, and trash. The structure is:

```text
device → request nodes → server → response nodes → device
```

The component:

- follows `sessionsStore.viewingSessionId`;
- shows “请选择或创建会话” when no session exists;
- keeps a single line with horizontal overflow;
- exposes subtle add targets on empty request/response segments and endpoint context menus;
- uses solid green, gray, and red hollow/exclamation node states;
- toggles present nodes on click;
- never toggles a missing node;
- uses HTML drag events and insertion markers for same-side sorting;
- opens the agreed context menu;
- opens the script editor for valid nodes;
- recreates a missing script with the same kind/name and then opens the editor;
- opens the picker before or after a node;
- refreshes on window focus.

- [ ] **Step 4: Add pipeline-level styling and motion**

Add shared tooltip/popover z-index and motion tokens to `base.css`. Keep the strip slightly shorter than a normal table row, use 20 px nodes, fine 12–14 px arrows, a quiet surface separator, and 120–160 ms state/hover transitions. Respect `prefers-reduced-motion`.

- [ ] **Step 5: Mount above the filter bar**

In `App.vue`:

```vue
<section class="app-content">
  <InterceptorPipeline />
  <FilterBar />
  <LogTable />
</section>
```

- [ ] **Step 6: Run frontend build**

Run:

```bash
pnpm build
```

Expected: Vue type checking and Vite build both succeed.

- [ ] **Step 7: Commit**

```bash
git add src/components/AppTooltip.vue src/components/InterceptorPicker.vue src/components/InterceptorPipeline.vue src/App.vue src/styles/base.css src/windows/launcher.ts
git commit -m "feat: add interactive interceptor pipeline"
```

---

### Task 8: Redesign the global manager and historical execution detail

**Files:**
- Modify: `src/windows/InterceptorManagerWindow.vue`
- Modify: `src/windows/ScriptEditorWindow.vue`
- Create: `src/windows/ScriptSnapshotWindow.vue`
- Modify: `src/windows/LogDetailWindow.vue`
- Modify: `src/windows/launcher.ts`
- Modify: `src/components/MonacoEditor.vue`

- [ ] **Step 1: Convert the manager to a global library**

Keep request/response tabs and global create/edit/delete. Remove switches, order badges, and move buttons. Show each script’s session usage count. Add inline rename with validation and Enter/Escape handling.

Delete confirmation must say how many sessions reference the script and explain that confirmation removes those references. After create, rename, or delete, dispatch a local `interceptors-changed` window event so the pipeline refreshes immediately.

- [ ] **Step 2: Make script editor saves refresh validity**

After a successful interceptor save, dispatch `interceptors-changed`. Keep column-script behavior unchanged.

- [ ] **Step 3: Add read-only historical source viewing**

Add a `readOnly` prop to `MonacoEditor.vue` and pass it to Monaco’s `readOnly` option. Create `ScriptSnapshotWindow.vue` with:

- historical script name;
- request/response badge;
- full 64-character SHA-256 display with copy action;
- read-only Lua Monaco editor;
- no save action.

Add `openScriptSnapshot(sessionId, logId, execution)` to the launcher with a stable window ID including phase and position.

- [ ] **Step 4: Group capture detail by execution**

Replace the aggregate modification sections with request and response execution groups ordered by position. Each group shows:

- `#<position + 1>` and historical name;
- short hash;
- execution error when present;
- “已执行，未产生修改” for empty successful runs;
- its own modification list;
- a click action opening the historical source window.

Rename the tab from “修改记录” to “拦截器”，and use total recorded executions for its count.

- [ ] **Step 5: Run frontend build**

Run:

```bash
pnpm build
```

Expected: Vue type checking and Vite build both succeed.

- [ ] **Step 6: Commit**

```bash
git add src/windows/InterceptorManagerWindow.vue src/windows/ScriptEditorWindow.vue src/windows/ScriptSnapshotWindow.vue src/windows/LogDetailWindow.vue src/windows/launcher.ts src/components/MonacoEditor.vue
git commit -m "feat: manage global interceptors and inspect history"
```

---

### Task 9: Full verification, visual QA, and default workspace normalization

**Files:**
- Modify only if verification finds defects.
- Update: default workspace `app_config.json` and session `interceptors.json` files after resolving the configured path.

- [ ] **Step 1: Format and lint source**

Run:

```bash
cargo fmt --all --check
pnpm exec vue-tsc --noEmit
```

Expected: both commands exit 0.

- [ ] **Step 2: Run the full Rust test suite**

Run:

```bash
cargo test --workspace
```

Expected: every unit, manager HTTP, and proxy integration test passes with zero failures.

- [ ] **Step 3: Build the production frontend**

Run:

```bash
pnpm build
```

Expected: Vue type checking and Vite production build exit 0.

- [ ] **Step 4: Run visual QA in the desktop app**

Start the Tauri app and inspect both themes at normal and narrow widths. Verify:

- empty session and empty-chain rendering;
- request/response endpoint direction;
- enabled, disabled, and missing states;
- tooltip viewport clamping;
- picker search and 12-item disabled state;
- click toggling;
- same-side drag reorder;
- context edit/add/remove/recreate;
- horizontal scrolling;
- manager usage counts and rename;
- historical source viewing.

Capture screenshots for light and dark themes and fix any overlap, clipping, weak contrast, or inconsistent spacing before continuing.

- [ ] **Step 5: Normalize the configured default workspace**

Read the configured workspace path first. Back up only the specific JSON files that will change. Remove obsolete `active_request_interceptors` and `active_response_interceptors` keys from its `app_config.json`. Create an empty `interceptors.json` for every existing session that lacks one:

```json
{
  "request": [],
  "response": []
}
```

Do not migrate the old active lists. Opening each session database through the finished application creates the two new history tables; retain existing capture rows and legacy aggregate columns.

- [ ] **Step 6: Re-run verification after workspace normalization**

Run:

```bash
cargo test --workspace
pnpm build
git diff --check
```

Expected: all commands exit 0 and `git diff --check` reports no whitespace errors.

- [ ] **Step 7: Review requirement coverage**

Confirm every product-contract bullet at the top of this plan is represented in code, tests, or visual QA. Inspect `git diff --stat` and `git status --short` to ensure no unrelated files were changed and no generated build artifacts were added.

- [ ] **Step 8: Commit**

```bash
git add .
git commit -m "test: verify session interceptor redesign"
```

