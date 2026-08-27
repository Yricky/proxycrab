# Session Share Toggle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace expiring, repeatable Session share-link generation with one process-local enable/disable share per Session and present its reachable URLs in the shared desktop/CLI application UI.

**Architecture:** `SessionShareService` owns at most one cleartext token and digest per Session for the lifetime of the process. Trusted Tauri commands and permission-controlled management routes expose status, idempotent enable, and idempotent disable operations; the existing read-only `/share-api/*` surface continues to authenticate the token digest. A managed application window loads state plus current IPv4 interface addresses, renders current URLs, and reserves the “导出和分享” surface for a later HAR section without implementing export now.

**Tech Stack:** Rust 2024, Axum, Tauri 2, Vue 3, TypeScript, Node test runner.

---

### Task 1: Make Session sharing a process-local toggle

**Files:**
- Modify: `crates/proxy-crab-mgr/src/session_share.rs`
- Modify: `crates/proxy-crab-mgr/src/http.rs`

- [x] **Step 1: Replace expiry tests with state-transition tests**

Add service tests that enable a Session twice and compare tokens, query the enabled state, disable it, verify authentication fails, and verify a new `SessionShareService` starts disabled. The public state shape is:

```rust
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct SessionShareState {
    pub session_id: u64,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}
```

- [x] **Step 2: Run the focused test and observe failure**

Run: `cargo test -p proxy-crab-mgr session_share::tests -- --nocapture`

Expected: compilation or assertions fail because status/disable and idempotent enable do not exist.

- [x] **Step 3: Implement one entry per Session**

Replace the expiring vector with a map keyed by Session ID. Retain the cleartext token only because reopening the owner dialog must reproduce the same links; continue authenticating with SHA-256 and constant-time digest comparison.

```rust
#[derive(Clone)]
struct ShareEntry {
    token: String,
    digest: [u8; 32],
}

#[derive(Default)]
pub struct SessionShareService {
    entries: Mutex<BTreeMap<u64, ShareEntry>>,
}

pub async fn enable(
    &self,
    manager: &Arc<dyn ProxyCrabManager>,
    session_id: u64,
) -> Result<SessionShareState, ManagerError>;

pub async fn status(
    &self,
    manager: &Arc<dyn ProxyCrabManager>,
    session_id: u64,
) -> Result<SessionShareState, ManagerError>;

pub async fn disable(
    &self,
    manager: &Arc<dyn ProxyCrabManager>,
    session_id: u64,
) -> Result<SessionShareState, ManagerError>;
```

`enable` returns an existing token when present, `status` returns it only to the management owner, and `disable` removes the entry immediately. All three reject unknown or archived Sessions consistently.

- [x] **Step 4: Add management routes**

Keep `POST /api/session-shares` as the enable operation with `{ "session_id": 3 }`. Add `GET /api/session-shares/{id}` for status and `DELETE /api/session-shares/{id}` for disable. Each returns `SessionShareState`.

- [x] **Step 5: Run focused Rust tests**

Run: `cargo test -p proxy-crab-mgr session_share::tests http::tests::session_share -- --nocapture`

Expected: all selected tests pass.

### Task 2: Expose status/enable/disable through both application backends

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/api/types.ts`
- Modify: `src/api/backend.ts`
- Modify: `src/api/tauri-backend.ts`
- Modify: `src/api/http-backend.ts`
- Modify: `crates/proxy-crab-mgr/src/permission.rs`
- Modify: `crates/proxy-crab-mgr/src/dto.rs`
- Modify: `crates/proxy-crab-mgr/tests/permission_contract.rs`
- Modify: `src-tauri/src/http_permissions/mod.rs`
- Modify: `src-tauri/src/http_permissions/store.rs`
- Modify: `cli-app/src/permissions/store.rs`
- Modify: `src/utils/permission-meta.ts`
- Modify: `src/stores/http-api-sync.ts`

- [x] **Step 1: Update transport types and methods**

Use one shared DTO and three backend methods:

```ts
export interface SessionShareState {
  session_id: number;
  enabled: boolean;
  token?: string;
}

getSessionShare(sessionId: number): Promise<SessionShareState>;
enableSessionShare(sessionId: number): Promise<SessionShareState>;
disableSessionShare(sessionId: number): Promise<SessionShareState>;
```

Tauri maps these to `get_session_share`, `enable_session_share`, and `disable_session_share`; the CLI HTTP backend maps them to `GET /api/session-shares/{id}`, `POST /api/session-shares`, and `DELETE /api/session-shares/{id}`.

- [x] **Step 2: Update permission catalog and migration tests**

Keep enable at `Approval`, add status as `Allow`, and add disable as `Approval`:

```rust
action!("GET", "/api/session-shares/{id}", Allow),
action!("POST", "/api/session-shares", Approval),
action!("DELETE", "/api/session-shares/{id}", Approval),
```

Update exact catalog assertions, UI labels, and desktop/CLI permission-file migration tests so new actions receive host-appropriate defaults.

- [x] **Step 3: Run transport and permission tests**

Run: `cargo test -p proxy-crab-mgr --test permission_contract && cargo test -p proxycrab-cli permissions::store && cargo test -p proxy-crab-t http_permissions`

Expected: all commands exit zero.

### Task 3: Build reachable share URLs and the “导出和分享” application window

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `cli-app/src/ui.rs`
- Modify: `src/utils/session-share.ts`
- Modify: `src/utils/session-share.test.mts`
- Create: `src/windows/SessionExportShareWindow.vue`
- Modify: `src/windows/launcher.ts`
- Modify: `src/components/SessionSidebar.vue`

- [x] **Step 1: Write URL-selection tests**

Cover duplicate removal, wildcard removal, normal loopback hiding, and IPv4 loopback fallback:

```ts
assert.deepEqual(
  buildSessionShareLinks(
    ["127.0.0.1", "192.168.1.5"],
    18089,
    "pcrab_share_a+b",
  ),
  [
    "http://192.168.1.5:18089/session?token=pcrab_share_a%2Bb",
  ],
);
assert.deepEqual(
  buildSessionShareLinks(["127.0.0.1"], 18089, "token"),
  [
    "http://127.0.0.1:18089/session?token=token",
  ],
);
```

- [x] **Step 2: Run URL tests and observe failure**

Run: `pnpm test:unit`

Expected: the loopback-fallback assertion fails with the old implementation.

- [x] **Step 3: Enumerate supported addresses and build URLs**

Return IPv4 addresses from both Tauri and CLI. In TypeScript, remove the wildcard address, prefer non-loopback addresses, fall back to loopback only when necessary, deduplicate, and preserve deterministic backend ordering.

- [x] **Step 4: Replace generation UI with stateful sharing UI**

Rename the context item to “导出和分享” and open a per-Session window through `windowsStore`, focusing and refreshing it when reopened. Load share status, management port, and current addresses. Render only a “分享” section; do not render a disabled HAR control. When enabled, show full URL rows where the row opens through `backend.openExternal` and a separate copy button copies that row. Use one dynamic action button, confirm before disable, retain disable access when the list has only loopback addresses, and show loading/error states through existing app conventions.

- [x] **Step 5: Run frontend tests and build**

Run: `pnpm test:unit && pnpm build`

Expected: unit tests and Vue/TypeScript production build exit zero.

### Task 4: Synchronize API, project, and ProxyCrab Skill documentation

**Files:**
- Modify: `README.md`
- Modify: `docs/backend-api.md`
- Modify: `skills/proxycrab/SKILL.md`
- Modify: `skills/proxycrab/references/http-api.md`
- Modify: `skills/proxycrab/evals/evals.json`

- [x] **Step 1: Document the lifecycle and security change**

State that each process owns at most one share per Session, enable is idempotent, status returns the current owner token, disable revokes it immediately, and restart/archive/disable invalidates it. Remove expiry/hour language. Document status and disable routes plus their permission defaults.

- [x] **Step 2: Update the Skill evaluation contract**

Change the sharing evaluation to expect `{session_id:3}`, prohibit printing the token, and recognize status/disable as permission-controlled owner operations. Preserve the rule that share tokens are never Agent bearer credentials.

- [x] **Step 3: Check stale terminology**

Run: `rg -n "DEFAULT_SHARE_HOURS|MAX_SHARE_HOURS|hours.*session-share|expires_at|1 through 720|链接分享" src crates/proxy-crab-mgr src-tauri cli-app README.md docs/backend-api.md skills/proxycrab`

Expected: no live documentation or implementation retains the removed expiry workflow; historical plan documents may still contain it.

### Task 5: Full verification and diff audit

**Files:**
- Review: all files changed above

- [x] **Step 1: Format code**

Run: `cargo fmt --all -- --check`

Expected: exit zero. If it reports differences, run `cargo fmt --all` and repeat the check.

- [x] **Step 2: Run complete relevant verification**

Run: `pnpm test:unit && pnpm build && cargo test -p proxy-crab-mgr && cargo test -p proxycrab-cli && cargo test -p proxy-crab-t`

Expected: every command exits zero with no failing tests.

- [x] **Step 3: Audit the patch against requirements**

Run: `git diff --check && git status --short && git diff --stat`

Expected: no whitespace errors; only Session sharing, address enumeration, permissions, tests, and required documentation are changed.
