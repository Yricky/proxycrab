# Stale In-Progress Records Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop rewriting persisted capture outcomes during proxy/application lifecycle events and derive stale `in_progress` records from the current proxy start time.

**Architecture:** The running `ProxyStatus` carries a Unix-millisecond `started_at` timestamp. Persisted outcomes remain unchanged; frontend consumers classify `in_progress` records as stale when the proxy is not running or their `created_at` predates the current run, while the bypass deletion layer applies the same rule server-side. Generation-scoped forced-drain finalization remains unchanged.

**Tech Stack:** Rust, Tokio, rusqlite, serde, Vue 3, TypeScript, Tauri, Node test runner.

---

### Task 1: Proxy lifecycle contract

**Files:**
- Modify: `crates/proxy-crab-mitm/src/model.rs`
- Modify: `crates/proxy-crab-mitm/src/proxy.rs`
- Modify: `crates/proxy-crab-mitm/src/runtime.rs`
- Modify: `crates/proxy-crab-mitm/src/storage.rs`
- Test: `crates/proxy-crab-mitm/tests/proxy_integration.rs`

- [ ] Add `started_at: u64` only to `ProxyStatus::Running` and set it immediately before publishing a successfully bound listener.
- [ ] Remove startup and top-level stop calls that update every `in_progress` capture, while retaining `TaskGroup` ID-range finalization for a drained/aborted connection generation.
- [ ] Remove the now-unused all-session stale-finalization helpers and their obsolete tests.
- [ ] Add lifecycle assertions proving a running status exposes a plausible start time and stopped status does not retain it.

### Task 2: Transparent-forwarding persistence and deletion

**Files:**
- Modify: `crates/proxy-crab-mitm/src/bypass.rs`
- Modify: `crates/proxy-crab-mitm/src/runtime.rs`
- Test: `crates/proxy-crab-mitm/src/bypass.rs`
- Test: `crates/proxy-crab-mitm/tests/proxy_integration.rs`

- [ ] Stop changing leftover `in_progress` rows when `bypass.db` opens.
- [ ] Define deletable rows as terminal rows plus stale `in_progress` rows: every `in_progress` row while not running, or rows created before `started_at` while running.
- [ ] Keep current-run `in_progress` rows protected from single deletion, batch deletion, and clear.
- [ ] Cover stopped, stale-running, current-running, and mixed batch/clear behavior with storage tests.

### Task 3: Shared frontend stale classification

**Files:**
- Create: `src/utils/capture-outcome.ts`
- Create: `src/utils/capture-outcome.test.mts`
- Modify: `package.json`
- Modify: `src/api/types.ts`
- Modify: `crates/proxy-crab-mgr/src/dto.rs`
- Modify: `crates/proxy-crab-mgr/src/manager.rs`
- Test: `crates/proxy-crab-mgr/src/manager.rs`

- [ ] Add `created_at` to log-view rows so list and column-group consumers can classify records without loading detail payloads.
- [ ] Implement one frontend predicate using the exact rule `outcome === "in_progress" && (status !== "running" || created_at < started_at)`.
- [ ] Test stopped/transition/failed states, older/current/equal timestamps, and terminal outcomes.

### Task 4: Session and bypass UI behavior

**Files:**
- Modify: `src/stores/logs.ts`
- Modify: `src/stores/column-groups.ts`
- Modify: `src/components/LogTable.vue`
- Modify: `src/windows/LogDetailWindow.vue`
- Modify: `src/windows/BypassWindow.vue`

- [ ] Exclude stale rows from active polling and breakpoint polling decisions in the main table and column-group cache.
- [ ] Render current `in_progress` rows with the existing active styling and stale rows with a gray dot in the main table.
- [ ] Render stale detail status as gray `已失效` and stop its detail refresh loop.
- [ ] Render stale transparent-forwarding status as gray `已失效`, allow selecting/deleting it, and describe clear as removing completed, failed, and stale rows while retaining current-run rows.

### Task 5: API, Skill, and verification

**Files:**
- Modify: `docs/backend-api.md`
- Modify: `skills/proxycrab/references/http-api.md`
- Modify: `skills/proxycrab/SKILL.md`
- Modify: `skills/proxycrab/evals/evals.json` if the lifecycle contract is represented there

- [ ] Document `running.started_at`, immutable lifecycle outcomes, stale derivation, generation-scoped forced shutdown behavior, and bypass deletion semantics.
- [ ] Update Skill guidance so agents do not interpret every persisted `in_progress` row as active.
- [ ] Run focused Rust tests, the full workspace tests, frontend unit tests/typecheck/build, and bundled Skill script tests.
- [ ] Review `git diff` to ensure unrelated user edits in `BypassWindow.vue` and `LogDetailWindow.vue` remain intact.
