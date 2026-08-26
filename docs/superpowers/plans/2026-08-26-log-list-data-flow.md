# Log List Data Flow Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make log IDs a complete, independently synchronized list while loading row values only for the viewport, with active/filter reconciliation driven by the latest ProxyStatus snapshot.

**Architecture:** Keep `POST /api/logs/ids` as the single read-only ID/filter endpoint. It supports bounded range scans and explicit-ID re-evaluation, returning overlapping `matched_ids` and `in_progress_ids`; persist Session filters through a separate write endpoint. The frontend performs a watermarked full scan on Session/filter changes, discovers raw IDs incrementally, reconciles active and pending filter candidates, and stores row values in a current-Session 5000-entry LRU populated from the virtual table's viewport.

**Tech Stack:** Rust, Axum, Tokio, SQLite/rusqlite, Vue 3 reactive stores, TypeScript, Tauri, Node test runner.

---

### Task 1: Make log ID queries read-only and expressive

**Files:**
- Modify: `crates/proxy-crab-mgr/src/dto.rs`
- Modify: `crates/proxy-crab-mgr/src/manager.rs`
- Modify: `crates/proxy-crab-mgr/src/http.rs`
- Modify: `crates/proxy-crab-mgr/src/session_share.rs`
- Test: `crates/proxy-crab-mgr/src/manager.rs`
- Test: `crates/proxy-crab-mgr/src/http.rs`
- Test: `crates/proxy-crab-mgr/src/session_share.rs`

- [ ] **Step 1: Add failing DTO/manager tests for the new result contract**

  Cover a currently active in-progress capture that matches (present in both arrays), an active in-progress capture that does not match (only `in_progress_ids`), a stale unfinished capture excluded from `in_progress_ids`, a completed match, explicit `ids`, and rejection of `ids` combined with range bounds.

- [ ] **Step 2: Run the focused manager tests and confirm the old `ids/filter` payload fails them**

  Run: `cargo test -p proxy-crab-mgr log_ids -- --nocapture`

- [ ] **Step 3: Replace the request/response DTOs**

  Use this public shape:

  ```rust
  pub struct LogIdsRequest {
      pub session_id: Option<u64>,
      pub filter: Option<SessionFilter>,
      pub ids: Option<Vec<u64>>,
      pub min_id: Option<u64>,
      pub max_id: Option<u64>,
      pub limit: Option<usize>,
  }

  pub struct LogIdsPayload {
      pub matched_ids: Vec<u64>,
      pub in_progress_ids: Vec<u64>,
  }
  ```

  `filter: None` always means match all and never reads the saved Session filter. `ids` is mutually exclusive with `min_id`/`max_id`; requests and returned matching pages remain capped at 10,000.

- [ ] **Step 4: Implement range and explicit-ID evaluation**

  For every scanned summary, add its ID to `in_progress_ids` only when `outcome == InProgress` and the ID is present in the current status `active_netlog` for that Session, independently of matching. Add matching IDs to `matched_ids`, allowing overlap. Preserve the current ascending behavior for `min_id` scans and descending behavior otherwise; a range scan with fewer than `limit` matches must have scanned to exhaustion so the frontend can terminate pagination.

- [ ] **Step 5: Remove query-side persistence and HTTP change publication**

  Delete `persist_filter`, the implicit saved-filter lookup, and the `session_view` event emitted by `POST /api/logs/ids`. The share route only scopes `session_id`; it no longer rewrites persistence flags.

- [ ] **Step 6: Run focused Rust tests**

  Run: `cargo test -p proxy-crab-mgr log_ids -- --nocapture`
  Expected: all log ID manager, HTTP, and share tests pass with the new payload.

### Task 2: Add independent Session filter persistence

**Files:**
- Modify: `crates/proxy-crab-mgr/src/dto.rs`
- Modify: `crates/proxy-crab-mgr/src/manager.rs`
- Modify: `crates/proxy-crab-mgr/src/http.rs`
- Modify: `crates/proxy-crab-mgr/src/permission.rs`
- Modify: `crates/proxy-crab-mgr/tests/permission_contract.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/api/backend.ts`
- Modify: `src/api/http-backend.ts`
- Modify: `src/api/tauri-backend.ts`
- Modify: `src/api/share-backend.ts`
- Modify: `src/api/types.ts`
- Modify: `src/utils/permission-meta.ts`

- [ ] **Step 1: Add failing manager/HTTP/permission tests**

  Verify `PUT /api/sessions/{id}/filter` updates only `SessionView.filter`, preserves columns, emits a scoped `session_view` change after success, and is an approval-gated write action. Verify log ID reads never mutate the saved filter.

- [ ] **Step 2: Implement `replace_session_filter` through every normal backend**

  Add a manager method and Tauri command that read the current view, replace only `filter`, validate through the existing Session view path, and persist it. Add `Backend.replaceSessionFilter(sessionId, filter)`. The share backend must reject/fall through as read-only and exposes no share write route.

- [ ] **Step 3: Register the route and UI permission metadata**

  Route `PUT /api/sessions/{id}/filter`, map it to `session_view` synchronization, and list it in permission contracts/catalog metadata.

- [ ] **Step 4: Run focused contract tests**

  Run: `cargo test -p proxy-crab-mgr session_filter -- --nocapture`
  Run: `cargo test -p proxy-crab-mgr --test permission_contract`

### Task 3: Replace the frontend ID state machine

**Files:**
- Modify: `src/stores/logs.ts`
- Modify: `src/components/FilterBar.vue`
- Modify: `src/api/types.ts`
- Modify: `src/stores/column-groups.ts`
- Modify: `src/AppMain.vue`
- Modify: `src/AppReadonly.vue`

- [ ] **Step 1: Introduce generation-safe full loading**

  On every Session switch and filter change: show a loading state; fetch the latest raw ID with the read-only endpoint and no filter; scan matching IDs in descending pages bounded by that initial maximum; atomically install the complete ID set; set the frontend-only `maxId`; then hand off to the normal poller. Ignore results from an older Session/filter generation.

- [ ] **Step 2: Save normal-page filters separately**

  Normal pages call `replaceSessionFilter` before the full read-only scan. Share pages skip persistence and use the same read endpoint with their local filter. Every ID request passes its filter explicitly; omission always means unfiltered discovery.

- [ ] **Step 3: Implement raw incremental discovery**

  Only when the viewed Session is `active_session_id`, scan all raw `id > maxId` pages. Without a filter, add the raw IDs directly. With a filter, evaluate each raw page through the same endpoint's explicit `ids` mode; advance `maxId` only after that evaluation succeeds.

- [ ] **Step 4: Reconcile active and pending filter candidates**

  Maintain `pendingFilterIds`. With a filter, evaluate `status.active_netlog[session] ∪ pendingFilterIds` every fast tick. `matched_ids` adds/removes candidates from display; `in_progress_ids` replaces their pending state. When an ID leaves the status active set, force one final filter evaluation and final row hydration. UI activity always reads only ProxyStatus.

- [ ] **Step 5: Implement the agreed polling schedule**

  For the viewed active Session, poll every second while status-active/pending candidates exist and every two seconds otherwise. A viewed inactive Session polls only while it has status-active/pending candidates. A ProxyStatus membership change triggers an immediate tick; status itself is not periodically polled.

- [ ] **Step 6: Remove older-page discovery**

  Delete `loadOlder`, `olderExhausted`, and edge-triggered ID paging. Sorting operates entirely on the complete in-memory ID set.

### Task 4: Add viewport row loading and a bounded LRU

**Files:**
- Create: `src/utils/lru.ts`
- Create: `src/utils/lru.test.mts`
- Modify: `src/stores/logs.ts`
- Modify: `src/components/LogTable.vue`

- [ ] **Step 1: Write failing LRU tests**

  Verify touch-on-read/write, replacement without capacity growth, least-recent eviction, protected visible/active keys, and deferred shrink when protected keys temporarily exceed 5000.

- [ ] **Step 2: Implement the minimal LRU helper**

  Keep a `Map` insertion order, move touched keys to the end, and evict the first unprotected keys until within capacity. The store remains responsible for reactive row/error maps.

- [ ] **Step 3: Separate ID rendering from row values**

  `LogTable` slices sorted IDs first, then resolves only visible rows. Missing rows render their ID plus loading placeholders. Increase the virtual buffer to 500 and remove the old scroll-edge ID loader.

- [ ] **Step 4: Batch viewport requests**

  Watch visible+buffer IDs, coalesce missing IDs, and call `logs/views` in batches of 200. Touch cached rows when displayed. Refresh displayed status-active rows every fast tick even when cached, and force final hydration when activity ends.

- [ ] **Step 5: Apply cache lifecycle rules**

  Clear ID and row state on Session switch. Keep row values across filter changes in the same Session. Clear rows when the content-affecting column signature changes; exclude width-only changes from that signature. Evict non-visible, non-active rows beyond 5000.

- [ ] **Step 6: Run frontend verification**

  Run: `npm run test:unit`
  Run: `npm run build`
  Expected: LRU tests and TypeScript/Vite build pass.

### Task 5: Update Skill, scripts, docs, and evals

**Files:**
- Modify: `skills/proxycrab/SKILL.md`
- Modify: `skills/proxycrab/references/http-api.md`
- Modify: `skills/proxycrab/references/best-practices.md`
- Modify: `skills/proxycrab/scripts/log-query.mjs`
- Modify: `skills/proxycrab/scripts/log-wait.mjs`
- Modify: `skills/proxycrab/evals/evals.json`
- Modify: `crates/proxy-crab-mgr/src/agents.rs`
- Modify: `docs/backend-api.md`

- [ ] **Step 1: Record the old-skill baseline failure**

  The baseline must show that current instructions use `persist_filter:false`, read `result.ids`, cannot explicitly re-evaluate IDs, and do not describe independent filter persistence. This was captured before documentation edits.

- [ ] **Step 2: Update bundled scripts**

  Remove persistence flags, pass an explicit empty filter when unfiltered behavior is intended, and consume `matched_ids`. `log-wait` must retain `in_progress_ids` candidates so a request that becomes matching only at completion is rechecked.

- [ ] **Step 3: Update reference contracts and best practices**

  Document the one read-only ID endpoint, overlapping arrays, range versus explicit-ID modes, stable full pagination, the independent filter write endpoint, status as the only UI activity source, and share-page read-only behavior.

- [ ] **Step 4: Replace obsolete eval expectations and add coverage**

  Remove `persist_filter` expectations. Add scenarios for stateless reads, intentional filter persistence, explicit candidate re-evaluation, overlapping matched/in-progress IDs, and final filtering after status removal.

- [ ] **Step 5: Verify Skill artifacts**

  Run: `node --test skills/proxycrab/scripts/lib/common.test.mjs`
  Run: `node -e 'JSON.parse(require("fs").readFileSync("skills/proxycrab/evals/evals.json", "utf8"))'`
  Re-run the reference retrieval scenario against the updated Skill and confirm it gives the new contract.

### Task 6: Full verification and review

**Files:**
- Review all files above plus the existing ProxyStatus activity changes on the branch.

- [ ] **Step 1: Format and run Rust verification**

  Run: `cargo fmt --all`
  Run: `cargo fmt --all -- --check`
  Run: `cargo test -p proxy-crab-mitm -p proxy-crab-mgr`
  Run: `cargo check --workspace`

- [ ] **Step 2: Run frontend and artifact verification**

  Run: `npm run test:unit`
  Run: `npm run build`
  Run: `git diff --check`

- [ ] **Step 3: Review invariants in the final diff**

  Confirm there is one UI activity source (`active_netlog`), no `persist_filter`, no old `payload.ids`, no scroll-edge ID loading, max ID advances only after successful page processing, stale generations cannot mutate the current view, and row eviction cannot discard currently visible/active rows unless they are immediately recoverable.

- [ ] **Step 4: Report completion without committing**

  No commit is created unless the user explicitly requests one. Report changed behavior, compatibility impact, and exact verification results.
