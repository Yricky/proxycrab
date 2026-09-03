# Session HAR Sharing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor the Session “导出和分享” window into vertically stacked share cards and replace the authenticated HAR export API with an independently revocable, process-local HAR download share.

**Architecture:** Keep the existing read-only Session page share unchanged and add a separate in-memory HAR share service keyed by Session. Enabling a HAR share stores an explicit, immutable list of capture IDs and a separate token; a public `GET /session.har?token=…` route authenticates that token and delegates to the existing internal HAR generator. Both Tauri and CLI browser frontends use the same backend abstraction and card UI, while the old `POST /api/logs/export` route and Agent export script are removed.

**Tech Stack:** Rust, Axum, Tauri 2, Vue 3, TypeScript, Node test runner.

---

### Task 1: Add the HAR share domain and download route

**Files:**
- Create: `crates/proxy-crab-mgr/src/har_share.rs`
- Modify: `crates/proxy-crab-mgr/src/lib.rs`
- Modify: `crates/proxy-crab-mgr/src/http.rs`
- Test: `crates/proxy-crab-mgr/src/har_share.rs`

- [ ] **Step 1: Write failing HAR share service tests**

Cover disabled status, enable with a scope and explicit IDs, idempotent re-enable, independent token validation, revocation, empty ID lists, Session validation, archived Session rejection, and direct download headers/body.

- [ ] **Step 2: Run the focused test and verify it fails**

Run: `cargo test -p proxy-crab-mgr har_share::tests -- --nocapture`

Expected: compilation/test failure because the HAR share module does not exist yet.

- [ ] **Step 3: Implement the minimal in-memory HAR share service**

Define `HarShareScope` (`all` or `filtered`), strict enable DTOs, serialized status containing `session_id`, `enabled`, `scope`, `log_count`, and optional token, plus one stored entry per Session containing the token digest and frozen sorted unique IDs. Add an authenticated `GET /session.har?token=…` route that calls `ProxyCrabManager::export_logs` with the stored IDs and emits the existing filename, content type, attachment, content length, no-store, and no-referrer headers.

- [ ] **Step 4: Run the focused test and verify it passes**

Run: `cargo test -p proxy-crab-mgr har_share::tests -- --nocapture`

Expected: all HAR share tests pass.

### Task 2: Expose independent HAR share management operations and remove legacy export

**Files:**
- Modify: `crates/proxy-crab-mgr/src/http.rs`
- Modify: `crates/proxy-crab-mgr/src/permission.rs`
- Modify: `crates/proxy-crab-mgr/src/dto.rs`
- Modify: `crates/proxy-crab-mgr/tests/permission_contract.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/http_permissions/mod.rs`
- Modify: `src-tauri/src/http_permissions/store.rs`

- [ ] **Step 1: Add failing route, change-event, permission, and Tauri command tests**

Expect `GET /api/session-har-shares/{id}`, `POST /api/session-har-shares`, and `DELETE /api/session-har-shares/{id}` to be separate permission-controlled actions and `session_har_share` change events. Expect `POST /api/logs/export` to be absent from routing and the permission catalog.

- [ ] **Step 2: Run focused tests and verify they fail**

Run: `cargo test -p proxy-crab-mgr permission_contract -- --nocapture`

Expected: assertions fail because the new actions are absent and the old export action remains.

- [ ] **Step 3: Implement the management surface**

Wire one shared `HarShareService` instance into the HTTP server and Tauri state, add status/enable/disable handlers and Tauri commands, publish scoped change events, add the three permission actions with the same defaults as Session page sharing, and migrate stored permissions by dropping the removed export action and adopting the new actions. Delete only the public legacy export handler/route; retain the manager DTO and generator as the internal implementation used by HAR downloads.

- [ ] **Step 4: Run focused tests and verify they pass**

Run: `cargo test -p proxy-crab-mgr permission_contract -- --nocapture`

Expected: permission and route contracts pass.

### Task 3: Add frontend HAR share transport and link construction

**Files:**
- Modify: `src/api/types.ts`
- Modify: `src/api/backend.ts`
- Modify: `src/api/tauri-backend.ts`
- Modify: `src/api/http-backend.ts`
- Modify: `src/stores/http-api-sync.ts`
- Modify: `src/utils/session-share.ts`
- Modify: `src/utils/session-share.test.mts`
- Modify: `src/utils/permission-meta.ts`

- [ ] **Step 1: Write failing link utility tests**

Assert HAR links use the same reachable IPv4 selection and token encoding as page links but target `/session.har`.

- [ ] **Step 2: Run the unit tests and verify they fail**

Run: `pnpm test:unit`

Expected: failure because HAR link construction is not implemented.

- [ ] **Step 3: Implement typed frontend operations**

Add HAR scope/state/enable request types, backend methods for status/enable/disable in Tauri and HTTP targets, the `session_har_share` sync resource, permission labels/grouping, and a shared URL builder parameterized by the page or HAR path.

- [ ] **Step 4: Run unit tests and verify they pass**

Run: `pnpm test:unit`

Expected: all utility tests pass.

### Task 4: Refactor the export-and-share window into independent cards

**Files:**
- Modify: `src/windows/SessionExportShareWindow.vue`
- Modify: `src/windows/launcher.ts`

- [ ] **Step 1: Implement card state and actions**

Load page share state, HAR share state, HTTP service state, local addresses, and the Session's saved filter. The page-share switch enables/disables immediately. While HAR sharing is off, show an `全部结果` / `当前筛选结果` selector; on enable, page through the existing log-ID API using either an empty filter or the saved applied filter, merge matched and in-progress IDs, and send the frozen set and chosen scope. While enabled, lock the scope and show the frozen count. Both switches expose accessible names and loading/disabled state without visible status text.

- [ ] **Step 2: Implement the two-card layout**

Make the window body one vertical scrolling region containing a `链接分享` card followed by a `HAR 分享` card. Put each switch in its card's upper-right corner, preserve link click/copy behavior and service-down warnings, and include a prominent HAR warning that headers, cookies, and bodies are not redacted. Remove the page-level footer and disable confirmations.

- [ ] **Step 3: Build the frontend**

Run: `pnpm build`

Expected: Vue type checking and Vite production build succeed.

### Task 5: Remove obsolete Agent export support and update active documentation

**Files:**
- Delete: `skills/proxycrab/scripts/har-export.mjs`
- Modify: `skills/proxycrab/SKILL.md`
- Modify: `skills/proxycrab/references/http-api.md`
- Modify: `skills/proxycrab/evals/evals.json`
- Modify: `README.md`
- Modify: `docs/backend-api.md`

- [ ] **Step 1: Remove legacy references**

Delete the Agent HAR export script and its evaluation, remove all active documentation for `POST /api/logs/export`, and document independent HAR share status/enable/disable operations, frozen IDs, immediate revocation, process-local lifetime, direct download URL, sensitive unredacted contents, and UI behavior.

- [ ] **Step 2: Verify no active legacy route or script references remain**

Run: `rg -n 'POST /api/logs/export|har-export\.mjs' README.md docs/backend-api.md skills/proxycrab crates src src-tauri --glob '!docs/superpowers/plans/**'`

Expected: no matches.

### Task 6: Full verification

**Files:**
- Verify all files changed above.

- [ ] **Step 1: Format and run Rust tests**

Run: `cargo fmt -p proxy-crab-mgr -- --check && cargo test -p proxy-crab-mgr && cargo test -p proxy-crab-t`

Expected: formatting is clean and all targeted Rust tests pass.

- [ ] **Step 2: Run frontend tests and build**

Run: `pnpm test:unit && pnpm build`

Expected: all unit tests and the production build pass.

- [ ] **Step 3: Review the final diff against the clarified requirements**

Run: `git diff --check && git status --short`

Expected: no whitespace errors, the pre-existing staged `src/components/SessionSidebar.vue` change is preserved, and every new diff maps to this plan.

### Task 7: Stream HAR downloads one entry at a time

**Files:**
- Modify: `crates/proxy-crab-mgr/src/har.rs`
- Modify: `crates/proxy-crab-mgr/src/dto.rs`
- Modify: `crates/proxy-crab-mgr/src/manager.rs`
- Modify: `crates/proxy-crab-mgr/src/har_share.rs`
- Modify: `README.md`
- Modify: `docs/backend-api.md`
- Modify: `skills/proxycrab/references/http-api.md`

- [x] **Step 1: Write failing serializer and download tests**

Make the HAR serializer test consume a stream, assert its first chunk is the HAR envelope before a missing body file is read, and assert the public download response omits `Content-Length` while still collecting into a valid empty HAR.

- [x] **Step 2: Run the focused tests and verify they fail**

Run: `cargo test -p proxy-crab-mgr har::tests -- --nocapture && cargo test -p proxy-crab-mgr har_share::tests -- --nocapture`

Expected: compilation or assertion failure because exports still return one completed byte vector and downloads still set `Content-Length`.

- [x] **Step 3: Implement entry-granular streaming**

Change `LogExport` to carry a boxed `Stream<Item = ManagerResult<Bytes>>`. Have `har` emit one prefix chunk, serialize and emit each `HarEntry` sequentially with its comma delimiter, and emit one suffix chunk. Keep capture validation and ordering in `MitmManager`, then pass the lazy stream directly to Axum `Body::from_stream` without `Content-Length`. A body read or serialization error after HTTP 200 terminates the response stream.

- [x] **Step 4: Update active documentation**

Document entry-granular streaming, unknown response length, and the fact that a late body/storage error interrupts the download and leaves a partial HAR instead of returning a structured HTTP 500.

- [x] **Step 5: Run focused and regression verification**

Run: `cargo fmt -p proxy-crab-mgr -- --check && cargo test -p proxy-crab-mgr && cargo test -p proxy-crab-t && pnpm test:unit && pnpm build && git diff --check`

Expected: formatting, Rust tests, frontend tests, build, and whitespace checks all pass.
