# FilterBar Column Groups Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add case-sensitive literal/regex column filtering, an on-demand per-Session column grouping popup, and compact capture-outcome indicators in the log table.

**Architecture:** Replace the column filter's `case_sensitive` flag with a `regex` flag and compile regular expressions once per ID query in the manager. Validate Rust regex syntax through a local Tauri command while the user types. Keep group snapshots entirely in a Vue store keyed by Session and column; page through unfiltered IDs and request one-column log views in batches, pausing work whenever the popup closes and polling only unfinished captures while open.

**Tech Stack:** Rust 2024, regex crate, Tauri 2 commands, Vue 3 Composition API, TypeScript, Node test runner.

---

### Task 1: Filter model and regex execution

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/proxy-crab-mgr/Cargo.toml`
- Modify: `crates/proxy-crab-mitm/src/model.rs`
- Modify: `crates/proxy-crab-mgr/src/manager.rs`
- Modify: `crates/proxy-crab-mgr/src/http.rs`

- [ ] **Step 1: Replace model tests/fixtures with the new wire shape**

Use `{ "kind": "column", "column": ..., "regex": false }`. Add a deserialization assertion proving an obsolete `case_sensitive` field is ignored and missing `regex` defaults to `false`.

- [ ] **Step 2: Run focused Rust tests and confirm the old implementation fails**

Run: `cargo test -p proxy-crab-mgr log_ids -- --nocapture`

- [ ] **Step 3: Implement literal and regex matching**

Define the model field as:

```rust
Column {
    column: FilterColumn,
    #[serde(default)]
    regex: bool,
}
```

Prepare `regex::Regex` once when `regex` is true. Literal mode uses case-sensitive `value.contains(input)`; regex mode uses `regex.is_match(&value)). Return a bad-request error for invalid patterns.

- [ ] **Step 4: Add focused manager tests**

Cover case-sensitive literal matching, substring regex matching, inline `(?i)`, anchors, custom-column regex matching, and invalid regex rejection.

- [ ] **Step 5: Run the manager tests**

Run: `cargo test -p proxy-crab-mgr`

### Task 2: Outcome payload and local regex validation

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/lib.rs`
- Modify: `crates/proxy-crab-mgr/src/dto.rs`
- Modify: `crates/proxy-crab-mgr/src/manager.rs`
- Modify: `src/api/backend.ts`
- Modify: `src/api/tauri-backend.ts`
- Modify: `src/api/types.ts`

- [ ] **Step 1: Add outcome assertions to log-view tests**

Assert every returned `LogViewRow` carries `in_progress`, `success`, `failed`, or `tunneled`.

- [ ] **Step 2: Extend the payload**

Add `outcome: String` to Rust `LogViewRow` and `outcome: CaptureOutcome` to the TypeScript mirror. Populate it from each capture summary.

- [ ] **Step 3: Add the Tauri validator**

Register:

```rust
#[tauri::command]
fn validate_filter_regex(pattern: String) -> Result<(), String> {
    regex::Regex::new(&pattern).map(|_| ()).map_err(|error| error.to_string())
}
```

Expose `validateFilterRegex(pattern: string): Promise<void>` through the frontend Backend abstraction.

- [ ] **Step 4: Run focused Rust tests**

Run: `cargo test -p proxy-crab-t validate_filter_regex && cargo test -p proxy-crab-mgr log_views`

### Task 3: Pure grouping helpers and resumable cache

**Files:**
- Create: `src/utils/column-groups.ts`
- Create: `src/utils/column-groups.test.mts`
- Create: `src/stores/column-groups.ts`
- Modify: `package.json`

- [ ] **Step 1: Write failing Node tests for pure behavior**

Test stable column keys, regex escaping (`a.b` → `^a\\.b$`), case-insensitive popup search, count/value sorting in both directions, empty/error labels, and exact-pattern selection.

- [ ] **Step 2: Run the tests and confirm failure**

Run: `node --experimental-strip-types --test src/utils/column-groups.test.mts`

- [ ] **Step 3: Implement pure helpers**

Export `columnGroupKey`, `exactColumnRegex`, `displayGroupValue`, and `filterAndSortGroups`. Use explicit sentinels for empty values and calculation failures so a legitimate string never collides with a special group.

- [ ] **Step 4: Implement the cache**

Keep reactive caches keyed by Session and column. Each cache retains group search/sort settings, per-ID value/update/outcome state, older-page cursor, and initialization completion. On open:

1. fetch unfiltered new IDs with `persist_filter: false`;
2. resume older ID pages until exhausted;
3. hydrate rows in batches of 200 using a one-column `view`;
4. re-request only rows whose outcome is `in_progress`;
5. repeat at the existing 1s/2s polling cadence while open.

Closing invalidates the active run token so paging and polling stop after the current request. Reopening resumes from cached cursors. Script-change events invalidate the affected custom-column caches.

- [ ] **Step 5: Run helper tests**

Run: `node --experimental-strip-types --test src/utils/column-groups.test.mts`

### Task 4: FilterBar popup and outcome dots

**Files:**
- Create: `src/components/ColumnGroupPopup.vue`
- Modify: `src/components/FilterBar.vue`
- Modify: `src/components/LogTable.vue`
- Modify: `src/stores/logs.ts`

- [ ] **Step 1: Add the group popup**

Render the group button only for a draft column, between the target picker and input. The anchored popup contains a top search/sort area, an indeterminate loading bar, and a scrollable value/count list. It stays open after group selection. The calculation-failure row is disabled.

- [ ] **Step 2: Apply exact groups**

Clicking a value enables regex mode, writes `^<escaped value>$` (or `^$` for empty), and immediately applies it. Highlight is derived only by comparing the current input string with the generated exact pattern.

- [ ] **Step 3: Replace the case toggle**

Replace “区分大小写” with “正则表达式”. Validate every regex input change through the Tauri command without debounce or stale-result races. Invalid input swaps the Enter hint for an error icon with the compiler message and blocks Enter.

- [ ] **Step 4: Render capture outcomes**

Add a 4px dot before the ID only when hydrated: blue for `in_progress`, red for `failed`, none for `success` or `tunneled`. Preserve outcomes in row fallback/merge state.

- [ ] **Step 5: Build the frontend**

Run: `npm run build`

### Task 5: API, Skill, and evaluation documentation

**Files:**
- Modify: `docs/backend-api.md`
- Modify: `skills/proxycrab/references/http-api.md`
- Modify: `skills/proxycrab/evals/evals.json`
- Inspect: `skills/proxycrab/scripts/log-query.mjs`

- [ ] **Step 1: Update examples and semantics**

Replace `case_sensitive` examples with `regex`. Document case-sensitive literal contains matching, Rust regex matching and inline flags, invalid-regex errors, and the new log-view `outcome` field.

- [ ] **Step 2: Update evaluation expectations**

Add or adjust an evaluation case requiring the new filter wire shape and outcome payload knowledge. Change scripts only if they construct column filter objects.

- [ ] **Step 3: Run documentation and script checks**

Run: `rg -n "case_sensitive" crates src skills/proxycrab docs/backend-api.md`

Run: `node --test skills/proxycrab/scripts/lib/common.test.mjs`

### Task 6: Full verification

**Files:**
- Verify all modified files.

- [ ] **Step 1: Format and inspect the diff**

Run: `cargo fmt --all -- --check`

Run: `git diff --check`

- [ ] **Step 2: Run the relevant test suites**

Run: `cargo test -p proxy-crab-mitm -p proxy-crab-mgr -p proxy-crab-t`

Run: `node --experimental-strip-types --test src/utils/column-groups.test.mts`

Run: `node --test skills/proxycrab/scripts/lib/common.test.mjs`

- [ ] **Step 3: Build the frontend**

Run: `npm run build`

- [ ] **Step 4: Review requirements against the final diff**

Confirm popup visibility, pause/resume behavior, unfiltered all-Session scope, completed-outcome polling stop, exact regex application, error state, retained per-cache controls, special groups, outcome dots, docs, and Skill evaluation coverage.
