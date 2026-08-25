# Read-only Session Sharing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let desktop and CLI administrators create 1–720 hour in-memory links that show one live Session through the existing `app-content` UI while every server-side operation remains scoped and read-only.

**Architecture:** Add an in-memory `SessionShareService` to the transport-neutral management crate. A separate `/share-api/*` router authenticates an opaque token, derives the Session ID from its server-side scope, and exposes only the read operations needed by the shared UI; it never merges the token into the normal `/api/*` permission system. Reuse the CLI browser bundle for `/session`, add a share-specific HTTP Backend, and render the existing filter/table/detail components with every mutating control removed.

**Tech Stack:** Rust 2024, Axum, Tokio, SHA-256 constant-time token lookup, Vue 3, TypeScript, Vite, Tauri 2.

---

### Task 1: Fix the management listener address

**Files:**
- Modify: `crates/proxy-crab-mitm/src/model.rs`
- Modify: `crates/proxy-crab-mgr/src/http.rs`
- Modify: `crates/proxy-crab-mgr/src/dto.rs`
- Modify: `src/api/types.ts`
- Modify: `cli-app/src/main.rs`
- Modify: `README.md`
- Modify: `docs/backend-api.md`
- Modify: `skills/proxycrab/references/http-api.md`

- [ ] **Step 1: Remove `api_host` from the Rust and TypeScript `AppConfig` models**

Keep `api_port`; Serde naturally ignores the legacy `api_host` key in existing JSON without a migration.

- [ ] **Step 2: Bind the management listener to the fixed unspecified IPv4 address**

Use `Ipv4Addr::UNSPECIFIED` in `start_http_server_with_routes` and report `0.0.0.0` through `HttpServerHandle` and `HttpServiceStatus`.

- [ ] **Step 3: Remove CLI management-host overrides**

Delete `--api-host`, `PROXYCRAB_API_HOST`, and the `PROXYCRAB_API_URL` startup override path. Retain `--api-port` / `PROXYCRAB_API_PORT`.

- [ ] **Step 4: Run focused compilation**

Run: `cargo check -p proxy-crab-mitm -p proxy-crab-mgr -p proxycrab-cli`

Expected: exit 0 and no remaining `AppConfig.api_host` references.

### Task 2: Add the in-memory share token service

**Files:**
- Create: `crates/proxy-crab-mgr/src/session_share.rs`
- Modify: `crates/proxy-crab-mgr/src/lib.rs`
- Modify: `crates/proxy-crab-mgr/Cargo.toml`

- [ ] **Step 1: Write token lifecycle tests**

Cover positive integer validation, the 1–720 hour bounds, opaque token creation, per-Session scope, distinct tokens for repeated creation, expiry, and explicit rejection after expiry.

- [ ] **Step 2: Implement `SessionShareService`**

Generate 256-bit `pcrab_share_…` tokens with `getrandom`, retain only SHA-256 digests, store `session_id` and `expires_at` under a mutex, and lazily remove expired entries during creation/authentication. Tokens intentionally disappear on process exit.

- [ ] **Step 3: Verify lifecycle tests**

Run: `cargo test -p proxy-crab-mgr session_share::tests`

Expected: all share lifecycle tests pass.

### Task 3: Add a strictly scoped share HTTP surface

**Files:**
- Modify: `crates/proxy-crab-mgr/src/session_share.rs`
- Modify: `crates/proxy-crab-mgr/src/http.rs`

- [ ] **Step 1: Write route security tests**

Build a router with two Sessions and assert:

- missing, unknown, and expired tokens return 401;
- bootstrap returns only the shared Session;
- request `session_id` values are overwritten by the token scope;
- filters are forced to `persist_filter: false` and do not change `view.json`;
- log IDs, rendered rows, details, bodies, current view, proxy status, column names, and filter names are readable;
- archived Sessions return `share_session_unavailable`;
- `/share-api/*` has no mutation, interceptor, breakpoint-control, export, permission, config, routing, CA, system-log, or bypass routes.

- [ ] **Step 2: Implement authenticated share routes**

Expose only:

```text
GET  /share-api/bootstrap
GET  /share-api/proxy/status
GET  /share-api/session-view
POST /share-api/logs/ids
POST /share-api/logs/views
GET  /share-api/logs/{id}
GET  /share-api/logs/{id}/body
GET  /share-api/column-scripts
GET  /share-api/filter-scripts
POST /share-api/validate-filter-regex
```

Authenticate a single `Authorization: Bearer pcrab_share_…` header. Resolve the Session from the token on every call, verify that it is still active/non-archived, and never accept a Session scope from the client.

- [ ] **Step 3: Return names without global script source**

The filter UI needs script names, not their Lua source. Return name-only script objects so sharing a Session does not disclose unrelated global source files.

- [ ] **Step 4: Verify route tests**

Run: `cargo test -p proxy-crab-mgr session_share`

Expected: lifecycle and route tests pass, including cross-Session and stateless-filter assertions.

### Task 4: Wire share creation and browser assets into both hosts

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/main.rs`
- Modify: `src-tauri/build.rs`
- Create: `src-tauri/src/share_ui.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: `cli-app/src/main.rs`
- Modify: `cli-app/src/ui.rs`

- [ ] **Step 1: Add host-private creation operations**

Add a trusted Tauri command and a CLI `/ui-api/session-shares` POST handler accepting:

```json
{ "session_id": 42, "hours": 24 }
```

Return the clear token once with `expires_at`. Do not add creation to the public management API.

- [ ] **Step 2: Share one service instance with the read-only router**

Instantiate one `Arc<SessionShareService>` per process and give it to both the trusted creation path and `/share-api/*` router.

- [ ] **Step 3: Serve the browser bundle at `/session`**

The CLI already embeds `dist/cli`; make extensionless paths fall back to `index.html`. Generate the same asset table from `dist/cli` in the Tauri build and expose only `/session` plus its `/assets/*` files from the management listener.

- [ ] **Step 4: Verify both host integrations compile**

Run: `cargo check -p proxycrab-cli -p proxy-crab-t`

Expected: exit 0.

### Task 5: Add the share dialog

**Files:**
- Create: `src/components/SessionShareDialog.vue`
- Modify: `src/components/SessionSidebar.vue`
- Modify: `src/api/backend.ts`
- Modify: `src/api/types.ts`
- Modify: `src/api/tauri-backend.ts`
- Modify: `src/api/http-backend.ts`

- [ ] **Step 1: Add the shared DTO and Backend method**

Define `CreatedSessionShare { token, session_id, expires_at }` and `createSessionShare(sessionId, hours)` for the trusted Tauri and CLI backends.

- [ ] **Step 2: Add “链接分享” to the Session context menu**

Open a modal for the selected non-archived Session. The share UI is unavailable from the share page because that page has no sidebar.

- [ ] **Step 3: Implement duration and link generation UI**

Start with `24`, accept integers from `1` through `720`, and create a new token on every Generate action. Fetch all local IPv4 addresses, exclude only `127.0.0.1` and `0.0.0.0`, deduplicate them, and show one complete URL per line:

```text
http://192.168.1.20:18089/session?token=pcrab_share_…
```

Use the actual management port, one token for every IP URL, a read-only textarea, and a “复制全部” action. Existing tokens remain valid until their own expiry.

- [ ] **Step 4: Verify TypeScript**

Run: `pnpm exec vue-tsc --noEmit`

Expected: exit 0.

### Task 6: Add the shared read-only app-content target

**Files:**
- Create: `src/landing/ShareLanding.vue`
- Create: `src/api/share-backend.ts`
- Modify: `src/landing/CliLanding.vue`
- Modify: `src/App.vue`
- Modify: `src/api/backend.ts`
- Modify: `src/components/LogTable.vue`
- Modify: `src/stores/logs.ts`

- [ ] **Step 1: Add share access bootstrap**

When the CLI bundle is opened at `/session`, read the query token, verify it through `/share-api/bootstrap`, install a `target: "share"` Backend, and render the shared view. Keep the token in the URL so refresh works. On expiry or archiving, unmount the content and display the stable server error.

- [ ] **Step 2: Render only app-content and supporting overlays**

Hide `AppToolbar`, `SessionSidebar`, and `InterceptorPipeline`. Keep `FilterBar`, `LogTable`, floating log-detail/read-only windows, context menus, copy actions, and toasts.

- [ ] **Step 3: Make the table read-only**

Keep local ID sort, scrolling, copying cells, opening details, complete body loading, historical interceptor executions, and script snapshots. Hide column menus and Add Column, disable resizing/persistence, and never call `replaceSessionView`.

- [ ] **Step 4: Make filters viewer-local**

Seed the page from the Session's current filter. Every later filter query must set `persist_filter: false`. Owner column changes are detected and reloaded, while an owner's later filter change never overwrites the viewer's current temporary filter.

- [ ] **Step 5: Keep live records without breakpoint control**

Continue log polling and proxy-status polling. Do not start breakpoint polling and do not expose breakpoint list/detail/control methods in the share Backend.

- [ ] **Step 6: Verify browser builds**

Run: `pnpm build:cli && pnpm build:tauri`

Expected: both Vite builds exit 0 and the CLI bundle contains the `/session` landing.

### Task 7: Documentation, Skill contract, and full verification

**Files:**
- Modify: `README.md`
- Modify: `docs/backend-api.md`
- Modify: `skills/proxycrab/SKILL.md`
- Modify: `skills/proxycrab/references/http-api.md`
- Modify: `skills/proxycrab/evals/evals.json`

- [ ] **Step 1: Document exposure and token safety**

Document the fixed `0.0.0.0` listener, remote Authorization requirement for normal management APIs, plain-HTTP LAN token risk, per-process expiry, per-Session scope, lack of revocation/persistence, and read-only share surface.

- [ ] **Step 2: Update the bundled Skill and eval coverage**

Remove stale client assumptions about a loopback-only default. State that share tokens are never Agent API keys and add an eval assertion that Agents do not use or expose them.

- [ ] **Step 3: Run formatters and complete verification**

Run:

```bash
cargo fmt --all -- --check
cargo test --workspace
pnpm test:unit
pnpm build:cli
pnpm build:tauri
```

Expected: every command exits 0 with no test failures or TypeScript errors.

- [ ] **Step 4: Audit the final diff against the requirements**

Confirm: live updates; initial filter inheritance; private stateless filters; live owner column changes; 24-hour default; integer 1–720 validation; one link per eligible IPv4; same scoped token in every line; memory-only expiry; archived/expired invalidation; no toolbar/sidebar/pipeline/column editing/interceptor/breakpoint/write API; full log details, bodies, historical executions, snapshots, sorting, and copying remain available.
