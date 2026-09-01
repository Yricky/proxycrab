# Built-in Time Columns Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add selectable `created_at` and `updated_at` table columns that preserve raw Unix-millisecond API values and render timezone-aware local timestamps in the UI.

**Architecture:** Extend the persisted Rust/TypeScript column union without changing the default Session view or filter model. Keep `LogViewRow.created_at` and `updated_at` as the frontend display source, while retaining aligned raw timestamp strings in `cells` for management API consumers. Format and copy time cells through one frontend helper.

**Tech Stack:** Rust, Serde, TypeScript, Vue 3 canvas UI, Node test runner.

---

### Task 1: Extend the column model and management API rendering

**Files:**
- Modify: `crates/proxy-crab-mitm/src/model.rs`
- Modify: `crates/proxy-crab-mgr/src/manager.rs`

- [ ] **Step 1: Add a failing manager test**

Extend the batch-view test to request `Column::CreatedAt` and `Column::UpdatedAt`, then assert that their cells contain the row's Unix-millisecond timestamps as decimal strings.

- [ ] **Step 2: Run the focused Rust test and verify it fails**

Run: `cargo test -p proxy-crab-mgr log_ids_apply_exclusive_bounds_and_batch_views_report_cell_errors -- --exact`

Expected: compilation fails because the two column variants do not exist.

- [ ] **Step 3: Add the minimal column variants and renderers**

Add `CreatedAt { width: f32 }` and `UpdatedAt { width: f32 }` to `Column`, serialize them as `created_at` and `updated_at`, include them in `width()` and `name()`, and render `CaptureSummary` timestamps with `to_string()`. Do not add them to `default_columns()` or `FilterColumn`.

- [ ] **Step 4: Run the focused Rust test and verify it passes**

Run: `cargo test -p proxy-crab-mgr log_ids_apply_exclusive_bounds_and_batch_views_report_cell_errors -- --exact`

Expected: one passing test.

### Task 2: Add timezone-aware frontend display and menu entries

**Files:**
- Modify: `src/api/types.ts`
- Modify: `src/utils/format.ts`
- Create: `src/utils/format.test.mts`
- Modify: `src/components/LogTable.vue`

- [ ] **Step 1: Add failing formatter tests**

Cover the exact `YYYY-MM-DD HH:mm:ss GMT±HH:mm` shape and verify the offset is derived from the computer's timezone for the timestamp.

- [ ] **Step 2: Run frontend unit tests and verify the new test fails**

Run: `npm run test:unit`

Expected: failure because the timezone-aware formatter is missing.

- [ ] **Step 3: Implement the frontend model and rendering**

Add the two TypeScript column variants. Add a formatter that uses local date/time components and `Date#getTimezoneOffset()`. Add 200px menu entries named `created_at` and `updated_at`; make every built-in menu label equal its raw kind while keeping script labels unchanged. Render and copy time cells from `LogViewRow.created_at`/`updated_at` through the formatter. Leave sorting ID-only.

- [ ] **Step 4: Run unit tests and the frontend build**

Run: `npm run test:unit`

Expected: all tests pass.

Run: `npm run build`

Expected: TypeScript checking and Vite build succeed.

### Task 3: Update API and bundled Skill documentation

**Files:**
- Modify: `docs/backend-api.md`
- Modify: `skills/proxycrab/references/http-api.md`
- Check: `skills/proxycrab/SKILL.md`
- Check: `skills/proxycrab/evals/evals.json`

- [ ] **Step 1: Document the new column kinds and cell semantics**

State that Session views accept `created_at` and `updated_at`, their aligned cells contain decimal Unix-millisecond strings, and row metadata retains numeric timestamps. Keep the main API documentation and bundled Skill reference consistent.

- [ ] **Step 2: Check Skill routing and eval coverage**

Confirm that neither the top-level Skill instructions nor existing eval prompts enumerate the table column union in a way that requires further changes.

- [ ] **Step 3: Run final verification**

Run: `cargo test -p proxy-crab-mitm -p proxy-crab-mgr`

Expected: all Rust tests pass.

Run: `npm run test:unit && npm run build`

Expected: all frontend tests pass and the production build succeeds.

Run: `git diff --check`

Expected: no whitespace errors.
