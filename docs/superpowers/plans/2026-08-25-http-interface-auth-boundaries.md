# HTTP Interface and Authentication Boundaries Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Separate trusted host operations, authenticated management operations, CLI-private UI operations, and query-token Session sharing while closing the forged-localhost authentication bypass.

**Architecture:** Trusted host mutations remain Tauri commands or local CLI commands and are removed from the public management router. `/api/*` resolves every request to an explicit `LocalLoopback` or `Bearer` credential before the permission manager runs. `/share-api/*` bypasses the management permission system and validates exactly one `token` query parameter scoped to one active Session.

**Tech Stack:** Rust 2024, Axum 0.8, Tauri 2, Vue 3/TypeScript, Tokio, Node test runner.

---

## File map

- Create `crates/proxy-crab-mgr/src/http/auth.rs`: management ingress credential resolution, loopback/Host/Origin policy, CORS preflight handling, and focused tests.
- Modify `crates/proxy-crab-mgr/src/http.rs`: compose authenticated management routes, remove trusted-host writes, add share creation, and stop growing authentication logic in this file.
- Modify `crates/proxy-crab-mgr/src/permission.rs`: replace optional Authorization with explicit `ManagementCredential` and update the route catalog.
- Modify `crates/proxy-crab-mgr/src/session_share.rs`: authenticate strict query tokens and add no-store responses.
- Modify `src-tauri/src/http_permissions/{mod.rs,store.rs}` and `cli-app/src/permissions/{mod.rs,store.rs}`: consume explicit credentials and normalize removed/new action IDs.
- Modify `cli-app/src/ui.rs`: remove the duplicate private Session-share creation route.
- Modify `src/api/{backend.ts,http-backend.ts,tauri-backend.ts,share-backend.ts,body.ts}`: separate trusted host operations, route share creation through `/api`, and propagate query tokens.
- Modify `src/{components/AppToolbar.vue,stores/proxy.ts,windows/SettingsWindow.vue,windows/CertManagerWindow.vue}`: make host mutations available only through the Tauri host capability.
- Modify permission metadata, Rust/TypeScript tests, README, backend API docs, ProxyCrab Skill references, and evals to lock the new contract.

### Task 1: Explicit management credentials

**Files:**
- Modify: `crates/proxy-crab-mgr/src/permission.rs`
- Create: `crates/proxy-crab-mgr/src/http/auth.rs`
- Modify: `crates/proxy-crab-mgr/src/http.rs`
- Test: `crates/proxy-crab-mgr/src/http/auth.rs`

- [ ] **Step 1: Add failing ingress tests**

Cover remote peer plus `Host: localhost` without Authorization returning `remote_auth_required`; loopback/local Host becoming `LocalLoopback`; any peer with one valid Bearer becoming `Bearer`; missing `ConnectInfo` failing closed; hostile local Origin requiring Bearer; and remote preflight requiring `Authorization` in requested headers.

- [ ] **Step 2: Run the focused test and confirm failure**

Run: `cargo test -p proxy-crab-mgr http::auth::tests -- --nocapture`

Expected: the new tests fail because credential resolution is still based only on Host/Origin and optional Authorization.

- [ ] **Step 3: Add the explicit type and ingress middleware**

Use this credential boundary:

```rust
pub enum ManagementCredential {
    LocalLoopback,
    Bearer(String),
}

pub struct PermissionAction {
    pub action: &'static ApiAction,
    pub credential: ManagementCredential,
    // existing audit fields remain unchanged
}
```

The middleware must parse exactly one `Authorization: Bearer <token>` header when present. Without it, it must require a loopback TCP peer, local Host, and absent-or-local Origin. Missing peer information fails closed. Valid preflight is answered before permission checks.

- [ ] **Step 4: Run focused tests**

Run: `cargo test -p proxy-crab-mgr http::auth::tests -- --nocapture`

Expected: all ingress credential and CORS tests pass.

### Task 2: Public route boundary and permission catalog

**Files:**
- Modify: `crates/proxy-crab-mgr/src/http.rs`
- Modify: `crates/proxy-crab-mgr/src/permission.rs`
- Modify: `crates/proxy-crab-mgr/tests/permission_contract.rs`

- [ ] **Step 1: Add failing route-contract assertions**

Assert that `PUT /api/workspace`, `PUT /api/config`, and `POST /api/ca` are absent; their GET routes remain; and `POST /api/session-shares` exists with default `approval`.

- [ ] **Step 2: Run the contract test and confirm failure**

Run: `cargo test -p proxy-crab-mgr --test permission_contract -- --nocapture`

Expected: failures identify the old three writes and missing Session-share action.

- [ ] **Step 3: Change the router and catalog**

Keep these public shapes:

```text
GET  /api/workspace
GET  /api/config
GET  /api/ca
POST /api/session-shares  (default approval)
```

Remove the three trusted-host write handlers from HTTP route composition and HTTP change-event mapping, but retain their manager methods and Tauri commands.

- [ ] **Step 4: Run route and manager tests**

Run: `cargo test -p proxy-crab-mgr --test permission_contract && cargo test -p proxy-crab-mgr http::tests`

Expected: both commands pass.

### Task 3: Target permission implementations and file normalization

**Files:**
- Modify: `src-tauri/src/http_permissions/mod.rs`
- Modify: `src-tauri/src/http_permissions/store.rs`
- Modify: `cli-app/src/permissions/mod.rs`
- Modify: `cli-app/src/permissions/store.rs`

- [ ] **Step 1: Add failing normalization and identity tests**

Persist permission fixtures containing the removed write actions and no Session-share action. Assert that opening the store removes obsolete keys, inserts Session sharing with target-appropriate defaults, and saves the normalized file. Assert that `LocalLoopback` selects local identity and `Bearer` never falls back to local identity.

- [ ] **Step 2: Run target tests and confirm failure**

Run: `cargo test -p proxy-crab-t http_permissions && cargo test -p proxycrab-cli permissions`

Expected: old validation rejects obsolete actions and implementations still consume optional Authorization.

- [ ] **Step 3: Implement credential matching and normalization**

Match `LocalLoopback` directly to local identity. Match `Bearer(token)` against the CLI UI token first on CLI, then against stored API keys. Normalize persisted maps with `retain` before adding missing defaults so removed route IDs migrate safely.

- [ ] **Step 4: Run target tests**

Run: `cargo test -p proxy-crab-t http_permissions && cargo test -p proxycrab-cli permissions`

Expected: all target authentication, approval, and persistence tests pass.

### Task 4: Strict query-token Session sharing

**Files:**
- Modify: `crates/proxy-crab-mgr/src/session_share.rs`
- Modify: `cli-app/src/ui.rs`
- Test: `crates/proxy-crab-mgr/src/session_share.rs`

- [ ] **Step 1: Add failing share tests**

Assert that every `/share-api/*` route accepts exactly one `token=pcrab_share_…` query parameter; missing, empty, duplicate, and unknown tokens return `invalid_share_token`; expired tokens return `share_expired`; Authorization-only access is rejected; and successful/error responses contain `Cache-Control: no-store`.

- [ ] **Step 2: Run tests and confirm failure**

Run: `cargo test -p proxy-crab-mgr session_share::tests -- --nocapture`

Expected: query-token success fails and old Bearer-only tests expose the old behavior.

- [ ] **Step 3: Implement query authentication**

Parse the raw query into URL form pairs, require exactly one non-empty `token`, and authenticate the existing digest in constant time. Keep Session scoping and active-Session checks unchanged. Remove the CLI `/ui-api/session-shares` route and handler because creation now uses authenticated `/api/session-shares`.

- [ ] **Step 4: Run share tests**

Run: `cargo test -p proxy-crab-mgr session_share::tests -- --nocapture`

Expected: all query authentication, scope, expiry, and read-only route tests pass.

### Task 5: Frontend transport and host capabilities

**Files:**
- Modify: `src/api/backend.ts`
- Modify: `src/api/http-backend.ts`
- Modify: `src/api/tauri-backend.ts`
- Modify: `src/api/share-backend.ts`
- Modify: `src/api/body.ts`
- Modify: `src/components/AppToolbar.vue`
- Modify: `src/stores/proxy.ts`
- Modify: `src/windows/SettingsWindow.vue`
- Modify: `src/windows/CertManagerWindow.vue`
- Test: `src/utils/session-share.test.mts`

- [ ] **Step 1: Add failing URL/header tests**

Assert that share requests append an encoded `token` while retaining body query parameters and never construct an Authorization header. Assert that HTTP Session-share creation uses `/api/session-shares`.

- [ ] **Step 2: Run unit/type checks and confirm failure**

Run: `pnpm test:unit && pnpm exec vue-tsc --noEmit`

Expected: tests or types fail until adapters and capabilities are updated.

- [ ] **Step 3: Separate trusted host operations**

Expose Tauri-only host mutations through one optional backend capability containing workspace replacement, config replacement, CA regeneration, and Skill installation. HTTP and share backends omit it. Keep read operations on the common Backend. Disable CLI port editing, workspace editing, and CA regeneration while preserving proxy start/stop and all read surfaces.

- [ ] **Step 4: Change share transport**

Make all share JSON and body requests append the query token. Remove share Authorization headers. Add `<meta name="referrer" content="no-referrer">` or the equivalent response header for the share page.

- [ ] **Step 5: Run frontend verification**

Run: `pnpm test:unit && pnpm exec vue-tsc --noEmit && pnpm build:cli && pnpm build:tauri`

Expected: tests, type checking, and both target builds pass.

### Task 6: Documentation, Skill contract, and full verification

**Files:**
- Modify: `README.md`
- Modify: `docs/backend-api.md`
- Modify: `skills/proxycrab/SKILL.md`
- Modify: `skills/proxycrab/references/http-api.md`
- Modify: `skills/proxycrab/evals/evals.json`
- Modify: `src/utils/permission-meta.ts`

- [ ] **Step 1: Update stable contracts**

Document the four interface classes, TCP-loopback trust requirement, explicit management credentials, removed public writes, authenticated share creation, strict query-token share reads, and plain-HTTP URL-token risk. Remove instructions for the old routes and Bearer share token.

- [ ] **Step 2: Update Skill evaluation coverage**

Keep the rule that Agent code never reuses a share token as an API key. Add coverage requiring `POST /api/session-shares` to obey ordinary permissions and requiring browser share requests to use only their scoped query token.

- [ ] **Step 3: Run full verification**

Run:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
pnpm test:unit
pnpm exec vue-tsc --noEmit
pnpm build:cli
pnpm build:tauri
node --test skills/proxycrab/scripts/lib/common.test.mjs
git diff --check
```

Expected: every command exits zero; route catalogs contain every public management action exactly once; no removed write route or old share Authorization flow remains outside historical plan documents.
