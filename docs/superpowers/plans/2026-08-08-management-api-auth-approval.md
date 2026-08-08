# Management API Authentication and Approval Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add workspace-scoped API keys, per-route allow/approval/deny permissions, synchronous desktop approval, and a sidebar-based settings UI without applying HTTP authentication to trusted Tauri commands.

**Architecture:** `proxy-crab-mgr` owns only the stable HTTP action catalog, the asynchronous permission-check contract, and Router middleware; every HTTP Router must receive a permission manager. The Tauri crate implements workspace persistence, API-key verification, temporary decisions, and the pending-approval queue, while desktop-only commands manage that state and the Vue UI renders settings and approvals. Existing `ProxyCrabManager` business methods and direct Tauri calls remain outside authentication.

**Tech Stack:** Rust 2024, Axum 0.8 middleware, Tokio synchronization, SHA-256 with constant-time comparison, Tauri 2 commands/events, Vue 3 Composition API, TypeScript, bundled Node.js Skill scripts.

---

## Confirmed contract

- The management server continues to bind only to loopback addresses. Host and Origin validation remains mandatory.
- A request without `Authorization` uses the independently configurable local/no-key identity. A request with `Authorization` must contain exactly one `Bearer <api-key>` value. `X-API-Key` and query-string credentials are unsupported.
- Authentication and permission checks wrap only HTTP routes. Direct Tauri commands continue to call `ProxyCrabManager` without authentication.
- Permission granularity is one HTTP method plus one Axum route template from `router_with_changes`. The UI groups routes only for presentation and bulk editing.
- Each route is `allow`, `approval`, or `deny`. Approval holds the original HTTP request for at most 30 seconds. Explicit denial is `403 permission_denied`, timeout is `403 approval_timeout`, and malformed/unknown/deleted credentials are `401 invalid_api_key`. Permission infrastructure failure is fail-closed with `500 permission_check_failed`.
- Approval offers allow once, deny once, or allow/deny for 5 minutes, 30 minutes, or 1 hour. Temporary decisions are keyed by identity plus route action, override persisted permission until expiry, and disappear on restart. A duration decision immediately settles all currently pending requests with the same identity and action.
- Pending requests are not recomputed when persisted permission changes or an API key is deleted. A request already waiting may still be approved and execute. Deletion affects subsequent requests only. Disconnecting the original HTTP client removes its pending item.
- API keys are workspace-scoped, have immutable unique names, show their full secret exactly once, and can only have permissions changed or be deleted. Stored records contain a public prefix, salt, hash, creation time, and throttled last-used time, never the full secret.
- A missing permission file creates a local identity with all current routes allowed. A new API key uses the default matrix below. Existing files store a complete permission table; newly introduced routes are appended using their code-defined default, while later changes to defaults do not rewrite existing route values. Corrupt/unreadable files prevent the HTTP service from starting rather than silently widening access.
- The top toolbar shows a continuously oscillating `IoHandRight` icon and count to the left of the AI/scripts/tools/system menus while approvals are pending. Clicking it opens or focuses one approval window.
- Settings has exactly two sidebar pages: General and Management API. Management API contains service status, a custom identity picker, API-key creation/deletion, and grouped single/bulk permission editing with explicit save and dirty-state guards.

## Default permission matrix for newly created API keys

### Allow

```text
GET    /api/agents.md
GET    /api/workspace
GET    /api/config
GET    /api/proxy/status
GET    /api/sessions
GET    /api/archived-sessions
GET    /api/active-session
POST   /api/logs/ids
POST   /api/logs/views
POST   /api/logs/export
GET    /api/logs/{id}
GET    /api/logs/{id}/body
GET    /api/breakpoints
GET    /api/breakpoints/{id}
GET    /api/breakpoints/{id}/body
GET    /api/session-view
GET    /api/session-interceptors
GET    /api/column-scripts
GET    /api/column-scripts/{name}
GET    /api/filter-scripts
GET    /api/filter-scripts/{name}
POST   /api/filter-scripts/{name}/debug
GET    /api/routing-scripts
GET    /api/routing-scripts/{name}
GET    /api/routing-script-selection
GET    /api/interceptors
GET    /api/interceptors/{kind}/{name}
GET    /api/bypass
GET    /api/system-logs
```

### Approval

```text
PUT    /api/workspace
PUT    /api/config
POST   /api/proxy/start
POST   /api/proxy/stop
POST   /api/sessions
PUT    /api/sessions/{id}
POST   /api/archived-sessions/{id}/restore
PUT    /api/active-session
PUT    /api/session-view
PUT    /api/session-interceptors
POST   /api/column-scripts
PUT    /api/column-scripts/{name}
POST   /api/filter-scripts
PUT    /api/filter-scripts/{name}
POST   /api/routing-scripts
PUT    /api/routing-scripts/{name}
PUT    /api/routing-script-selection
POST   /api/interceptors
PUT    /api/interceptors/{kind}/{name}
POST   /api/breakpoints/{id}/extend
POST   /api/breakpoints/{id}/execute
POST   /api/breakpoints/{id}/release
```

### Deny

```text
POST   /api/sessions/{id}/archive
DELETE /api/archived-sessions/{id}
DELETE /api/column-scripts/{name}
DELETE /api/filter-scripts/{name}
DELETE /api/routing-scripts/{name}
DELETE /api/interceptors/{kind}/{name}
GET    /api/ca
POST   /api/ca
DELETE /api/system-logs
DELETE /api/bypass
POST   /api/bypass/delete
DELETE /api/bypass/{id}
```

## File responsibilities

- `crates/proxy-crab-mgr/src/permission.rs`: transport-facing permission modes, action descriptors, request context, denial result, route catalog, and the async `PermissionManager` trait.
- `crates/proxy-crab-mgr/src/http.rs`: mandatory permission-manager injection, matched-route middleware, request preview preservation, CORS preflight, denial envelopes, and HTTP tests.
- `crates/proxy-crab-mgr/src/lib.rs`: public exports for the permission contract and catalog.
- `src-tauri/src/http_permissions/model.rs`: persisted and frontend-facing API-key, identity, permission, and approval DTOs.
- `src-tauri/src/http_permissions/store.rs`: strict file loading, normalization, atomic `0600` writes, secure key creation/verification, permission replacement, deletion, and last-used throttling.
- `src-tauri/src/http_permissions/approval.rs`: pending queue, 30-second waiters, duration decisions, cancellation cleanup, and change notifications.
- `src-tauri/src/http_permissions/mod.rs`: `HttpPermissionService`, `PermissionManager` implementation, audit logging, and desktop methods.
- `src-tauri/src/lib.rs`: lifecycle wiring, mandatory HTTP injection, Tauri commands/events, and graceful shutdown.
- `src/api/types.ts`, `src/api/backend.ts`, `src/api/tauri-backend.ts`: typed desktop-only permission/key/approval APIs.
- `src/stores/approvals.ts`: global pending count/list state and Tauri event subscription.
- `src/components/CustomSelect.vue`: keyboard-accessible reusable custom select used for identity and group permission choices.
- `src/components/PermissionEditor.vue`: grouped route matrix, mixed group state, and bulk updates.
- `src/components/ApiKeySecretDialog.vue`: create form and one-time secret acknowledgement/copy flow.
- `src/windows/SettingsWindow.vue`: two-page settings shell, management identity selection, dirty guards, key deletion, and save orchestration.
- `src/windows/ApprovalWindow.vue`: earliest-deadline-first approval list and once/duration decisions.
- `src/windows/launcher.ts`, `src/components/AppToolbar.vue`, `src/App.vue`: approval window launcher, animated indicator, and approval-store lifecycle.
- `skills/proxycrab/scripts/lib/common.mjs`, `skills/proxycrab/scripts/body-get.mjs`: `PROXYCRAB_API_KEY` Authorization support and a timeout longer than the 30-second approval window.
- `skills/proxycrab/SKILL.md`, `skills/proxycrab/references/http-api.md`, `skills/proxycrab/references/best-practices.md`, `skills/proxycrab/evals/evals.json`: authentication, approval, least-privilege, and evaluation guidance.
- `docs/backend-api.md`, `README.md`: public architecture, configuration, response codes, CORS, and desktop behavior.

### Task 1: Define the permission contract and complete route catalog

**Files:**
- Create: `crates/proxy-crab-mgr/src/permission.rs`
- Modify: `crates/proxy-crab-mgr/src/lib.rs`
- Test: `crates/proxy-crab-mgr/src/permission.rs`

- [ ] **Step 1: Write catalog tests before the model**

Add tests that assert all 63 method/template actions are unique (29 allow, 22 approval, 12 deny), group labels are non-empty, the three default sets exactly match the confirmed matrix above, and lookup uses both method and template. Also assert both CA actions are denied, filter debug and logs export are allowed, and breakpoint release requires approval.

- [ ] **Step 2: Run the focused test and verify it fails**

Run: `cargo test -p proxy-crab-mgr permission -- --nocapture`

Expected: compilation fails because the permission module and catalog do not exist.

- [ ] **Step 3: Add the public permission types**

Implement serializable `PermissionMode::{Allow, Approval, Deny}` and a static `ApiAction` descriptor containing a stable ID (`"METHOD /template"`), method, route template, Chinese UI label, group key, Chinese group label, and default mode. Define owned request context without exposing Axum request types:

```rust
pub struct PermissionAction {
    pub action: &'static ApiAction,
    pub authorization: Option<String>,
    pub actual_path: String,
    pub query: Option<String>,
    pub source: Option<SocketAddr>,
    pub content_type: Option<String>,
    pub content_length: Option<u64>,
    pub body_preview: Option<String>,
    pub body_preview_truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PermissionDenied {
    pub status: PermissionDeniedStatus,
    pub code: String,
    pub message: String,
}

#[async_trait]
pub trait PermissionManager: Send + Sync {
    async fn check_permission(&self, action: PermissionAction) -> Option<PermissionDenied>;
}
```

Use `PermissionDeniedStatus::{Unauthorized, Forbidden, Internal}` so the contract does not let implementations return arbitrary HTTP status codes. Never derive `Debug` for `PermissionAction`, because it contains the raw Authorization value.

- [ ] **Step 4: Encode the route catalog once**

Create one `API_ACTIONS` static slice in the same order as the UI groups: basics/workspace, proxy, sessions, capture logs, session views, scripts/interceptors, breakpoints, bypass, CA, system logs. Provide `api_actions()` and `find_api_action(method, route_template)`. Store full IDs in persisted permission maps so route parameters never create per-resource permissions.

- [ ] **Step 5: Export the contract and run tests**

Expose the module from `lib.rs`, then run:

```bash
cargo test -p proxy-crab-mgr permission -- --nocapture
```

Expected: all catalog/default tests pass.

- [ ] **Step 6: Commit the contract**

```bash
git add crates/proxy-crab-mgr/src/permission.rs crates/proxy-crab-mgr/src/lib.rs
git commit -m "feat: define management API permission catalog"
```

### Task 2: Enforce permissions in the HTTP Router and support local CORS

**Files:**
- Modify: `crates/proxy-crab-mgr/src/http.rs`
- Modify: `crates/proxy-crab-mgr/Cargo.toml` only if `http-body-util` is needed directly
- Test: `crates/proxy-crab-mgr/src/http.rs`

- [ ] **Step 1: Add failing middleware tests**

Create recording, allow-all, and deny permission-manager fakes. Cover: missing Authorization reaches the checker as `None`; exactly one Bearer header is forwarded unchanged; method and `MatchedPath` resolve to the expected catalog item; denial maps to 401/403/500 envelopes; denied handlers are not called; JSON bodies remain readable after a 16 KiB preview; oversized previews are truncated without truncating the handler body; query and loopback peer address are included; and permission audit structures never format the Authorization value.

Add preflight tests verifying a local `OPTIONS` request bypasses business permission checks, returns 204, echoes the allowed local Origin, permits `Authorization` and `Content-Type`, advertises the route methods, and still rejects non-local Host or Origin.

- [ ] **Step 2: Run focused HTTP tests and verify failure**

Run: `cargo test -p proxy-crab-mgr http::tests -- --nocapture`

Expected: compile failures at Router construction and missing middleware helpers.

- [ ] **Step 3: Require permission injection**

Change the constructors to require `Arc<dyn PermissionManager>`:

```rust
pub async fn start_http_server(
    manager: Arc<dyn ProxyCrabManager>,
    permissions: Arc<dyn PermissionManager>,
) -> Result<HttpServerHandle, ManagerError>;

pub fn router(
    manager: Arc<dyn ProxyCrabManager>,
    permissions: Arc<dyn PermissionManager>,
) -> Router;
```

Keep `ProxyCrabManager` unchanged. Pass connect information from `axum::serve` so permission actions can contain the loopback peer address. Update every HTTP unit test to inject an explicit allow-all or purpose-built fake; there must be no implicit production fallback.

- [ ] **Step 4: Add route-aware permission middleware**

Apply authentication with `route_layer` after all business routes are registered so `MatchedPath` is available. Parse neither the key nor its hash in `proxy-crab-mgr`; copy at most one Authorization value into `PermissionAction` and let the injected implementation interpret it. Reject duplicate/non-UTF-8 Authorization values through the permission implementation's `invalid_api_key` result.

For request preview, preserve the original body stream: read data frames only until 16 KiB is available, retain those frames, then reconstruct a body that yields retained frames followed by the untouched remainder. Use `Content-Length` as total size when present, mark previews truncated when the stream or declared size exceeds 16 KiB, decode only UTF-8/JSON/text content for display, and emit no binary preview. Do not log headers or preview text.

- [ ] **Step 5: Extend local request protection for CORS**

Keep Host/Origin validation as the outer middleware. Answer valid local `OPTIONS` preflight before route handlers and permission checks, adding `Access-Control-Allow-Origin`, `Access-Control-Allow-Methods`, `Access-Control-Allow-Headers: Authorization, Content-Type`, and `Vary: Origin`. Continue adding the matching Origin header to ordinary responses. Do not make the CORS policy permissive for remote origins.

- [ ] **Step 6: Map stable denial errors**

Return the existing JSON error envelope. Extend `ApiError` mapping for `invalid_api_key` (401), `permission_denied` and `approval_timeout` (403), and `permission_check_failed` (500). Ensure HTTP mutation change events publish only after the authorized handler succeeds.

- [ ] **Step 7: Run the crate tests**

```bash
cargo test -p proxy-crab-mgr
```

Expected: all existing manager/HTTP tests and new permission/CORS/body-preservation tests pass.

- [ ] **Step 8: Commit HTTP enforcement**

```bash
git add crates/proxy-crab-mgr/src/http.rs crates/proxy-crab-mgr/Cargo.toml Cargo.lock
git commit -m "feat: enforce permissions on management HTTP routes"
```

### Task 3: Persist workspace-scoped API keys and complete permission tables

**Files:**
- Create: `src-tauri/src/http_permissions/model.rs`
- Create: `src-tauri/src/http_permissions/store.rs`
- Create: `src-tauri/src/http_permissions/mod.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: workspace `Cargo.toml` for reusable dependency versions
- Test: `src-tauri/src/http_permissions/store.rs`

- [ ] **Step 1: Write persistence and key tests**

Use temporary workspaces to cover: missing file creates local/all-allow state; new keys receive the exact catalog defaults; names are trimmed, 1–64 characters, unique, and immutable; full keys are returned once and never serialized; valid keys verify in constant time; malformed, duplicated, unknown, and deleted Bearer credentials fail; permissions can be replaced only with a complete known action table; key deletion leaves no credential record; file mode is `0600` on Unix; corrupt JSON and unsupported versions fail closed; atomic writes preserve the previous file on failure; new catalog entries are appended with their current default; existing entries are not rewritten after a default changes; and `last_used_at` writes at most once per minute per key.

- [ ] **Step 2: Run the Tauri library test and verify failure**

Run: `cargo test -p proxy-crab-t http_permissions -- --nocapture`

Expected: module/dependency compile failures.

- [ ] **Step 3: Add cryptographic dependencies and persisted schema**

Add workspace-pinned `getrandom`, `sha2`, `base64`, and `subtle`; consume them from `src-tauri`. Store `http_api_permissions.json` in the active workspace with this versioned shape:

```json
{
  "version": 1,
  "local": { "permissions": { "GET /api/config": "allow" } },
  "api_keys": [
    {
      "id": "random-public-id",
      "name": "agent",
      "prefix": "Ab3x9Q",
      "salt": "base64url",
      "hash": "base64url-sha256",
      "created_at": 0,
      "last_used_at": null,
      "permissions": {}
    }
  ]
}
```

Use `BTreeMap<String, PermissionMode>` for deterministic files. Do not store temporary decisions or pending approvals.

- [ ] **Step 4: Implement secure immutable keys**

Generate 32 random secret bytes, a random public ID, a six-or-more-character public prefix, and 16 random salt bytes. Return `pcrab_<prefix>_<base64url-secret>` once. Persist `SHA-256(salt || full_key)` and compare digests with `subtle::ConstantTimeEq`. Parse one case-insensitive Bearer scheme with a non-empty single token; reject all other Authorization forms as `invalid_api_key`. Do not impose an API-key count limit.

- [ ] **Step 5: Implement strict load/normalize/write behavior**

On first creation, set every local route to allow and save the file. On every valid load, add catalog actions absent from each identity using that action's current default and write the normalized complete table. Reject unknown/missing versions, malformed records, duplicate IDs/names/prefixes, invalid hashes, or unreadable files. Write to a sibling temporary file, sync it, set owner-only permissions on Unix, rename atomically, and sync the parent directory where supported. Never replace a corrupt file with defaults.

- [ ] **Step 6: Implement desktop store operations**

Expose list identities, create key, delete key, get one identity's complete permissions, and atomically replace one complete permission table. Names use trimmed case-sensitive uniqueness. Update a valid key's in-memory `last_used_at` for every recognized request outcome and persist at most once per 60 seconds; a timestamp write failure is logged and does not change the permission result.

- [ ] **Step 7: Run persistence tests**

```bash
cargo test -p proxy-crab-t http_permissions::store -- --nocapture
```

Expected: all schema, normalization, hashing, permissions, atomicity, and throttling tests pass.

- [ ] **Step 8: Commit the workspace permission store**

```bash
git add Cargo.toml Cargo.lock src-tauri/Cargo.toml src-tauri/src/http_permissions
git commit -m "feat: persist workspace API keys and route permissions"
```

### Task 4: Implement synchronous approval and the Tauri permission service

**Files:**
- Create: `src-tauri/src/http_permissions/approval.rs`
- Modify: `src-tauri/src/http_permissions/mod.rs`
- Modify: `src-tauri/src/http_permissions/model.rs`
- Test: `src-tauri/src/http_permissions/approval.rs`
- Test: `src-tauri/src/http_permissions/mod.rs`

- [ ] **Step 1: Write approval state-machine tests**

Use Tokio paused time and an injected notifier callback. Cover direct allow/deny; 30-second timeout; one-shot allow/deny; each 5/30/60-minute decision; temporary precedence over persisted permission; restart clearing temporary state; one duration decision settling all same-identity/action waiters; different identity/action isolation; earliest-deadline ordering; dropped checker future removing a pending item; deleted key not canceling an existing waiter but rejecting subsequent checks; persisted changes not recomputing waiters; notification on add/remove/settle; and notifier failure not changing security decisions.

- [ ] **Step 2: Run focused tests and verify failure**

Run: `cargo test -p proxy-crab-t http_permissions::approval -- --nocapture`

Expected: compile failure because the queue and decisions do not exist.

- [ ] **Step 3: Add approval DTOs and queue ownership**

Define stable IDs, identity snapshots, actual path, route template, query, source, content type/length, optional 16 KiB preview, truncation flag, creation/deadline timestamps, and remaining time. Never include Authorization, hash, salt, or full key in pending DTOs. Sort list results by deadline then creation ID.

Use a cancellation guard owned by each `check_permission` future. Dropping it removes the item unless already settled. Use one-shot channels for per-request decisions and `tokio::time::timeout(Duration::from_secs(30), receiver)` for automatic denial.

- [ ] **Step 4: Implement temporary decisions**

Key temporary entries by immutable identity (`Local` or API-key ID) and action ID. Check unexpired temporary entries before persisted permissions. A duration resolution writes an expiry and settles all matching current waiters. One-shot decisions settle only the selected request. Prune expired entries opportunistically on checks and list operations.

- [ ] **Step 5: Implement `PermissionManager` fail-closed**

`HttpPermissionService::check_permission` performs these steps in order: parse/verify Authorization or select local identity; update recognized-key last use; resolve the catalog action's persisted mode; apply temporary override; return `None` for allow; return `permission_denied` for deny; enqueue and await for approval. Convert lock/state errors to `permission_check_failed`, but log and ignore last-used persistence failures. Audit with `tracing`: invalid credentials, direct denial, approval created/allowed/denied/timed out/client-cancelled, key create/delete, and permission save. Do not log direct allows, Authorization, request bodies, salts, or hashes.

- [ ] **Step 6: Run the complete permission-service tests**

```bash
cargo test -p proxy-crab-t http_permissions -- --nocapture
```

Expected: persistence and approval tests pass, including timeout and cancellation.

- [ ] **Step 7: Commit the approval engine**

```bash
git add src-tauri/src/http_permissions
git commit -m "feat: add synchronous management API approvals"
```

### Task 5: Wire desktop-only commands, events, and HTTP startup

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add command-contract tests**

Test conversion between internal records and frontend DTOs, one-time secret responses, deletion, complete permission replacement, pending list ordering, decision validation (once has no duration; duration must be exactly 300/1800/3600 seconds), and HTTP status reporting when the permission file cannot initialize.

- [ ] **Step 2: Extend backend state without changing business manager calls**

Keep `manager: Arc<dyn ProxyCrabManager>`. Add an optional/initialized `Arc<HttpPermissionService>` and retain its initialization error separately. Construct the service from `runtime.workspace_paths().current_path` during Tauri setup, inject it into `start_http_server(manager.clone(), permissions.clone())`, and do not start HTTP when permission initialization fails. Direct Tauri business commands remain unchanged.

- [ ] **Step 3: Add desktop-only commands**

Register commands for:

```text
get_http_permission_catalog
list_http_permission_identities
get_http_identity_permissions
replace_http_identity_permissions
create_http_api_key
delete_http_api_key
list_http_approvals
resolve_http_approval
```

No equivalent HTTP routes are added. Creating returns the full key once; normal identity listing returns only ID, immutable name, prefix, created time, and last-used time. Replace requires one entry for every catalog action.

- [ ] **Step 4: Emit approval state changes**

Give the permission service a notifier that emits `proxycrab://approval-change` with the current pending count whenever the queue changes. The frontend will still fetch the authoritative list after events, so event loss does not corrupt state. Preserve `proxycrab://http-api-change` for successful business mutations.

- [ ] **Step 5: Report service failures accurately**

Use `get_http_service_status` as the settings source of truth. If permission loading failed, return `running: false` plus the strict load error. Do not overwrite the broken file or silently install allow-all. Shutdown first stops the proxy, then HTTP, and lets dropping the permission service reject/remove outstanding waiters.

- [ ] **Step 6: Run all Rust tests and checks**

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: formatting, all workspace tests, and Clippy pass.

- [ ] **Step 7: Commit lifecycle wiring**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat: expose desktop API key and approval commands"
```

### Task 6: Add typed frontend permission and approval state

**Files:**
- Modify: `src/api/types.ts`
- Modify: `src/api/backend.ts`
- Modify: `src/api/tauri-backend.ts`
- Create: `src/stores/approvals.ts`
- Modify: `src/App.vue`

- [ ] **Step 1: Add exact wire types**

Define `PermissionMode`, catalog/group/action DTOs, local/API-key identity unions, immutable API-key summary, created-key response, permission-table response, approval summary, and resolution request. Use snake_case fields exactly as serialized by Rust. Add `HttpServiceStatus` to the shared Backend contract and replace the settings page's error-only call with the full status call.

- [ ] **Step 2: Add Backend methods and Tauri mappings**

Add a method for every command in Task 5. Keep full key values scoped to the create response and do not store them in a global reactive store. Continue wrapping command failures in `BackendError`.

- [ ] **Step 3: Implement the global approval store**

The store owns `items`, `count`, `loading`, an event unlisten handle, and methods `start`, `stop`, `refresh`, and `resolve`. On start, subscribe to `proxycrab://approval-change` and immediately list approvals to recover events emitted before Vue mounted. Coalesce bursts with a short timer. Use the server-provided deadline for countdown display; refresh after every decision.

- [ ] **Step 4: Start and stop approval synchronization with the app**

Call `approvalsStore.start()` in `App.vue` mounting and `stop()` during unmount, alongside existing HTTP UI synchronization. A failure shows the standard toast but must not affect traffic or other stores.

- [ ] **Step 5: Run frontend type/build verification**

Run: `pnpm build`

Expected: Vue TypeScript checking and Vite production build pass.

- [ ] **Step 6: Commit frontend data flow**

```bash
git add src/api src/stores/approvals.ts src/App.vue
git commit -m "feat: synchronize API approval state in the desktop UI"
```

### Task 7: Build the approval window and animated toolbar indicator

**Files:**
- Create: `src/windows/ApprovalWindow.vue`
- Modify: `src/windows/launcher.ts`
- Modify: `src/components/AppToolbar.vue`

- [ ] **Step 1: Add the unique approval window launcher**

Open ID `http-approvals`, title `接口审批`, with a practical initial size around 760×520. Repeated clicks focus the same window.

- [ ] **Step 2: Render complete, non-secret approval context**

List earliest deadline first. Each card shows API-key name/prefix or `本机无 API Key`, method, actual path, route template, query, loopback source, content type/declared size, UTF-8 preview with an explicit truncation marker, and a live 30-second countdown. Do not render headers or Authorization. Closing the window leaves requests waiting. Empty state explains that the toolbar icon disappears when no requests remain.

- [ ] **Step 3: Add once and duration actions**

Provide `允许一次` and `拒绝一次`, plus a custom duration selector for 5 minutes, 30 minutes, and 1 hour with adjacent allow/deny buttons. Disable only the card being submitted. A duration result may remove multiple cards after the authoritative refresh. Expired or concurrently settled IDs should refresh silently after a not-found/conflict response.

- [ ] **Step 4: Add the toolbar hand indicator**

Immediately left of the full menu group, render a button only when `approvalsStore.count > 0`. Use `IoHandRight`, a count badge, accessible label/title, and a continuous keyframe oscillating between `rotate(-30deg)` and `rotate(30deg)`. Respect `prefers-reduced-motion` by disabling rotation while keeping the icon visible. Clicking opens/focuses the approval window.

- [ ] **Step 5: Build and visually inspect static states**

Run: `pnpm build`

Expected: build passes. During later Browser QA, inspect no-pending, one-pending, multiple-pending, truncated-preview, expiring, light, and dark states.

- [ ] **Step 6: Commit the approval UI**

```bash
git add src/windows/ApprovalWindow.vue src/windows/launcher.ts src/components/AppToolbar.vue
git commit -m "feat: add desktop management API approval UI"
```

### Task 8: Refactor Settings into General and Management API pages

**Files:**
- Create: `src/components/CustomSelect.vue`
- Create: `src/components/PermissionEditor.vue`
- Create: `src/components/ApiKeySecretDialog.vue`
- Modify: `src/windows/SettingsWindow.vue`
- Modify: `src/windows/launcher.ts`

- [ ] **Step 1: Build a reusable accessible custom select**

Support controlled value/options, disabled state, placeholder, keyboard open/close, arrows, Enter/Space selection, Escape, outside click, current checkmark, and `role=combobox/listbox/option`. It must support local identity, API-key identity, group bulk mode, and approval-duration options without native `<select>`.

- [ ] **Step 2: Build the grouped permission editor**

Render the ten confirmed groups in catalog order. Each group header contains a bulk custom select showing Allow/Approval/Deny or Mixed; choosing a value updates every row locally. Each route row shows Chinese label, method badge, route template, and its own tri-state custom select. Visually distinguish allow/success, approval/warning, and deny/danger without relying on color alone. Emit immutable permission-map updates.

- [ ] **Step 3: Build API-key creation and one-time reveal**

The dialog first validates a trimmed unique 1–64-character name. After creation it replaces the form with the full key, copy button, warning that it cannot be recovered, and a required `我已保存此 API Key` checkbox before close. On close, emit the created key ID so Settings selects it. Never place the full key in local storage, logs, URL, toast, or the identity list.

- [ ] **Step 4: Replace Settings with a sidebar shell**

Use only `通用` and `管理接口` sidebar entries. General retains current/current-next-start workspace behavior. Management API shows running/error status and actual loopback URL, then a custom identity picker containing `本机无 API Key` and immutable API-key names/prefixes. Put `新建` beside it. For a selected API key show created/last-used times and `删除`; deletion requires confirmation and then selects local identity. There is no rename/edit-key action. Increase the default settings window size to fit the route editor, approximately 900×640.

- [ ] **Step 5: Add explicit saves and dirty-state guards**

Load a complete table for the selected identity and save only through `replaceHttpIdentityPermissions`. New-key creation and deletion are immediate. Permission changes remain local until `保存`. Switching identity/page or closing the floating window with unsaved changes asks whether to discard. External workspace/config refresh preserves dirty edits and shows the existing reload warning pattern. Successful save refreshes authoritative data and clears dirty state.

- [ ] **Step 6: Handle initialization errors safely**

When permission configuration is corrupt/unreadable, show the HTTP service error prominently and disable identity/key/permission controls without creating or overwriting a file. General workspace controls remain usable.

- [ ] **Step 7: Run frontend verification**

```bash
pnpm build
```

Expected: TypeScript and Vite pass with no native select regression or unresolved icons.

- [ ] **Step 8: Commit the settings refactor**

```bash
git add src/components/CustomSelect.vue src/components/PermissionEditor.vue src/components/ApiKeySecretDialog.vue src/windows/SettingsWindow.vue src/windows/launcher.ts
git commit -m "feat: manage API key permissions from settings"
```

### Task 9: Make the bundled ProxyCrab Skill authentication-aware

**Files:**
- Modify: `skills/proxycrab/scripts/lib/common.mjs`
- Modify: `skills/proxycrab/scripts/body-get.mjs`
- Modify: `skills/proxycrab/SKILL.md`
- Modify: `skills/proxycrab/references/http-api.md`
- Modify: `skills/proxycrab/references/best-practices.md`
- Modify: `skills/proxycrab/evals/evals.json`

- [ ] **Step 1: Centralize safe Authorization headers**

Read only `PROXYCRAB_API_KEY`; when non-empty, attach `Authorization: Bearer <value>` to JSON, text, download, and direct body requests. Merge it with `Content-Type` without printing it in errors. Do not add a CLI `--api-key` option, because process arguments are easier to leak. Reject newline-containing environment values locally. Refactor `body-get.mjs` to reuse the shared header helper.

- [ ] **Step 2: Cover the approval wait in client timeout behavior**

Raise the common request timeout from 10 seconds to 40 seconds so a 30-second approval can complete and the handler still has time to respond. Keep a clear timeout error. Apply the same timeout policy to text/body/download helpers rather than leaving inconsistent unbounded calls.

- [ ] **Step 3: Update Skill operating guidance**

Document: loopback-only transport; optional `PROXYCRAB_API_KEY`; full key shown once; 401 meaning malformed/unknown key; 403 direct denial/approval timeout; requests may wait 30 seconds; agents must not retry an approval/denial loop aggressively; API keys cannot be created over HTTP; and least privilege should be requested from the user through Settings. Update all previous claims that the API has no authentication.

- [ ] **Step 4: Add an authentication evaluation case**

Add one eval whose environment supplies `PROXYCRAB_API_KEY` and whose expected behavior requires using the existing script helper/Authorization header without echoing the key, tolerating an approval wait, and reporting stable error codes without bypass attempts. Keep JSON valid and do not embed a real secret in fixtures.

- [ ] **Step 5: Validate scripts, docs, and eval JSON**

```bash
node --check skills/proxycrab/scripts/lib/common.mjs
node --check skills/proxycrab/scripts/body-get.mjs
for file in skills/proxycrab/scripts/*.mjs; do node --check "$file"; done
node -e 'JSON.parse(require("node:fs").readFileSync("skills/proxycrab/evals/evals.json", "utf8"))'
rg -n "no authentication|PROXYCRAB_API_KEY|approval_timeout|invalid_api_key" skills/proxycrab
```

Expected: every script parses, eval JSON parses, obsolete no-auth wording is gone, and new auth guidance is present.

- [ ] **Step 6: Commit the Skill update**

```bash
git add skills/proxycrab
git commit -m "docs: teach ProxyCrab skill API key authentication"
```

### Task 10: Update project documentation and verify end to end

**Files:**
- Modify: `docs/backend-api.md`
- Modify: `README.md`
- Inspect only: `docs/socks5-support.md` (untracked user file; do not edit or add)
- Test: full Rust, frontend, HTTP, and visual flows

- [ ] **Step 1: Document the final public contract**

In `docs/backend-api.md`, add the permission contract, Bearer syntax, local/no-key identity, full default matrix, CORS preflight, error/status codes, 30-second synchronous approval, temporary decision durations, desktop-only key management, workspace file behavior, one-time key reveal, hashing/no-plaintext guarantee, audit log scope, and the fact that Tauri calls bypass HTTP authentication. Update README setup with where to create a key and how CLI/Skill users set `PROXYCRAB_API_KEY`.

- [ ] **Step 2: Run targeted HTTP behavior checks**

With a temporary workspace/application data directory, verify:

```text
no key + local allow -> 200
new valid key + GET /api/config -> 200
new valid key + GET /api/ca -> 403 permission_denied
malformed/unknown/deleted Bearer -> 401 invalid_api_key
approval allowed once -> original request completes
approval denied once -> 403 permission_denied
unanswered approval -> 403 approval_timeout after 30 seconds
five-minute allow -> same identity/action bypasses approval
different key/action -> still follows its own permission
local OPTIONS with Authorization request headers -> 204 and correct CORS headers
remote Host/Origin -> rejected
```

Capture only synthetic keys in test output and remove temporary state afterward.

- [ ] **Step 3: Run full automated verification**

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
pnpm build
git diff --check
```

Expected: all commands exit successfully.

- [ ] **Step 4: Perform Browser visual and interaction QA**

Start the desktop/frontend target using the normal project command, then use the in-app Browser capability when the local target is available. Verify: Settings sidebar at narrow and default size; identity custom select keyboard behavior; group mixed state and bulk edit; dirty guards on identity/page/window changes; create/copy/acknowledge secret; immutable key metadata and deletion; light/dark mode; permission-file error state; hand icon position, count, ±30-degree animation and reduced-motion behavior; approval list ordering/countdown; once and duration decisions; and closing/reopening the approval window without cancelling requests.

- [ ] **Step 5: Review repository-specific synchronization**

Inspect `skills/proxycrab/` scripts, references, and evals against every changed HTTP behavior. Confirm no API-key management HTTP endpoint was added, every route in `router_with_changes` appears in `API_ACTIONS`, every existing API script uses the shared Authorization helper, and the untracked `docs/socks5-support.md` remains untouched.

- [ ] **Step 6: Inspect the final diff and commit docs/verification fixes**

```bash
git status --short
git diff -- crates/proxy-crab-mgr src-tauri src skills/proxycrab docs/backend-api.md README.md Cargo.toml Cargo.lock
git diff --check
git add docs/backend-api.md README.md
git commit -m "docs: describe management API authentication and approvals"
```

Expected: only task-related files are staged/committed; `docs/socks5-support.md` remains an unrelated untracked user file.

## Self-review checklist

- The Router cannot start without an explicitly injected permission manager.
- `ProxyCrabManager` contains no API-key, permission, or approval methods.
- Tauri commands are trusted and never pass through HTTP authentication.
- There are no API-key management HTTP routes.
- All current method/template routes have exactly one catalog entry and the confirmed default.
- Local/no-key and every API key have independent complete permission tables.
- Full secrets never reach disk, logs, events, reactive global state, URLs, or process arguments.
- Approval holds the original request, times out at 30 seconds, and cleans up on cancellation.
- Persisted changes and deletion do not mutate requests already waiting.
- Temporary decisions are identity/action-scoped, higher priority, in-memory only, and settle matching waiters.
- Settings contains only General and Management API sidebar pages.
- Skill scripts send Authorization from `PROXYCRAB_API_KEY` and wait longer than the approval timeout.
- Project docs, bundled Skill docs/scripts/evals, tests, and visual QA all reflect the new contract.
