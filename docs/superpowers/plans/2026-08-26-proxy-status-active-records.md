# Proxy Status Active Records Implementation Plan

> **For Codex:** REQUIRED SUB-SKILL: Use executing-plans to implement this plan task-by-task.

**Goal:** Replace frontend inference and periodic proxy-status polling with an event-refreshed, runtime-authoritative set of active capture IDs and counts, while keeping shared-session responses scoped by authentication.

**Architecture:** A small runtime activity tracker owns exact active netlog IDs grouped by Session and active bypass IDs. RAII guards register after a database record is created and unregister when processing reaches a terminal path. `ProxyController::status()` projects the tracker snapshot into `ProxyStatus::Running`, and a watch notification is bridged into the existing `HttpApiChange::Proxy` channel. The frontend fetches status initially and after change events, treats `active_netlog` as the only active-ID source, and continues using persisted `outcome` and `stage` for their separate result and execution-phase meanings.

**Tech Stack:** Rust/Tokio/Axum/Serde, Vue 3/Pinia/TypeScript, Vitest/Node tests, Cargo tests.

---

### Task 1: Add runtime-authoritative activity tracking

**Files:**
- Modify: `crates/proxy-crab-mitm/src/model.rs`
- Modify: `crates/proxy-crab-mitm/src/proxy.rs`
- Modify: `crates/proxy-crab-mitm/src/body.rs`
- Modify: `crates/proxy-crab-mitm/src/runtime.rs`
- Test: `crates/proxy-crab-mitm/tests/proxy_integration.rs`

- [ ] Extend only the running status variant with `active_netlog: BTreeMap<u64, Vec<u64>>` and `active_bypass_count: usize`.
- [ ] Implement a cloneable tracker and RAII activity guard; register only after record creation succeeds and remove exactly once on guard completion/drop.
- [ ] Carry capture guards through ordinary response streaming, CONNECT capture, bypass response streaming, and bypass CONNECT paths so terminal or cancelled work becomes inactive.
- [ ] Clear tracker state between proxy runs and expose a watch receiver that changes for lifecycle and activity transitions.
- [ ] Add focused tests for grouped IDs, count transitions, and cleanup on terminal/error paths.

### Task 2: Publish status changes through the existing proxy event

**Files:**
- Modify: `crates/proxy-crab-mgr/src/manager.rs`
- Modify: `crates/proxy-crab-mgr/src/http.rs`
- Test: `crates/proxy-crab-mgr/src/http.rs`

- [ ] Expose the proxy-status watch receiver through the manager abstraction.
- [ ] Bridge watch changes into the existing `HttpApiChange { resources: [Proxy] }` broadcaster used by Tauri and CLI web clients.
- [ ] Remove redundant request-path-generated proxy changes for start/stop once lifecycle notifications are authoritative.
- [ ] Verify one activity transition wakes a change subscriber and a following status read contains the new snapshot.

### Task 3: Scope shared-session proxy status

**Files:**
- Modify: `crates/proxy-crab-mgr/src/session_share.rs`
- Modify: `src/api/share-backend.ts`
- Test: `crates/proxy-crab-mgr/src/session_share.rs`

- [ ] Project the shared status through the authenticated `ShareScope` rather than returning the global manager status directly.
- [ ] For running status, retain only `active_netlog[authorized_session_id]` and omit `active_bypass_count`; keep lifecycle fields required by the shared frontend.
- [ ] Apply the same projection to bootstrap and `/share-api/proxy/status` so cached bootstrap data cannot bypass scoping.
- [ ] Add tests proving unrelated Session IDs and bypass activity are absent.

### Task 4: Make status the frontend's unique active-ID source

**Files:**
- Modify: `src/api/types.ts`
- Modify: `src/stores/proxy.ts`
- Modify: `src/stores/http-api-sync.ts`
- Modify: `src/stores/logs.ts`
- Modify: `src/stores/column-groups.ts`
- Modify: `src/components/LogTable.vue`
- Modify: `src/windows/LogDetailWindow.vue`
- Modify: `src/utils/capture-outcome.ts`
- Modify: `src/utils/capture-outcome.test.mts`
- Modify: `src/AppMain.vue`
- Modify: `src/AppReadonly.vue`

- [ ] Mirror the running status fields in TypeScript, with bypass count optional for scoped shared responses.
- [ ] Remove the proxy store's two-second timer and serialize event-driven refreshes so an older response cannot overwrite a newer snapshot.
- [ ] Replace active/stale decisions for netlogs with membership in `active_netlog`; retain the existing run-age helper only for bypass rows, which do not receive individual IDs.
- [ ] On newly active IDs, refresh ID discovery; on IDs that become inactive, perform one final row hydration so cached `in_progress` outcomes converge to terminal results.
- [ ] Keep log ID listing as membership/order discovery and log views as row-content retrieval; neither may infer active truth.

### Task 5: Render activity count badges

**Files:**
- Modify: `src/components/SessionSidebar.vue`
- Test: relevant frontend component/store tests if present

- [ ] Replace the Session “活跃” icon with that Session's active connection count badge when the count is positive, retaining the label and exact-count tooltip.
- [ ] Replace the transparent-forwarding icon with the active bypass count badge when positive, preserving its click behavior and adding an exact-count accessible label.
- [ ] Display counts above 99 as `99+` while retaining the exact count in title/ARIA text.

### Task 6: Synchronize docs, Skill guidance, and evaluations

**Files:**
- Modify: `docs/backend-api.md`
- Modify: `skills/proxycrab/SKILL.md`
- Modify: `skills/proxycrab/references/http-api.md`
- Modify: `skills/proxycrab/evals/evals.json`

- [ ] Document the new running status schema, corrected `active_bypass_count` spelling, event invalidation behavior, and share-response scoping.
- [ ] Replace Skill guidance that infers activity from `created_at`/`started_at` with exact `active_netlog` membership.
- [ ] Update the matching evaluation expectation without changing unrelated cases.

### Task 7: Verify the complete change

**Files:**
- Review: all files above

- [ ] Run targeted Rust formatting and tests for `proxy-crab-mitm` and `proxy-crab-mgr`.
- [ ] Run frontend type checking and targeted unit tests, then the normal frontend test/build command available in `package.json`.
- [ ] Search for remaining proxy status polling and netlog active inference; verify only bypass fallback logic still uses run-age staleness.
- [ ] Review `git diff --check`, `git diff --stat`, and the final diff for accidental or unrelated changes.

No commits will be created unless explicitly requested.
