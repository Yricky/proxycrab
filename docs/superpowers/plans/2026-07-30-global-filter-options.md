# Global Filter Options Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace ad-hoc Lua filter text with per-Session persisted filter options backed by built-in columns, global custom-column scripts, or global parameterized Lua filter scripts.

**Architecture:** Add a typed `SessionFilter` to each Session view and let `POST /api/logs/ids` optionally apply and persist a new filter only after the ID query succeeds; later polling requests omit the filter and reuse the persisted state. Global filter scripts live beside the existing script libraries, receive the input string as Lua `...`, and keep `entry` as a read-only global. The Vue filter bar maintains a draft separate from the applied Session state, exposes a grouped target picker, and only commits changes on Enter except that selecting “未选择” clears immediately.

**Tech Stack:** Rust 2024, mlua 0.11, Serde, Tokio, Axum, Tauri 2, Vue 3, TypeScript, Monaco Editor, vue-icons-plus.

---

## File map

- `crates/proxy-crab-mitm/src/model.rs`: typed filter option and persisted Session filter model; `ScriptKind::Filter`.
- `crates/proxy-crab-mitm/src/lua.rs`: parameterized filter-script execution and tests.
- `crates/proxy-crab-mitm/src/workspace.rs`: global filter-script directory and default Session creation behavior.
- `crates/proxy-crab-mitm/src/runtime.rs`: Session filter validation/repair plus script rename/delete reference maintenance.
- `crates/proxy-crab-mgr/src/dto.rs`: structured log-ID filter request and filter-debug DTOs.
- `crates/proxy-crab-mgr/src/manager.rs`: filtering, successful-apply persistence, global filter-script CRUD, and debug execution.
- `crates/proxy-crab-mgr/src/http.rs`: HTTP routes for filter scripts and debug execution; removal of filter-history routes.
- `src-tauri/src/lib.rs`: matching Tauri commands.
- `src/api/types.ts`, `src/api/backend.ts`, `src/api/tauri-backend.ts`: frontend filter/script types and backend bindings.
- `src/stores/logs.ts`: applied filter state, polling against persisted state, and transactional UI application.
- `src/components/FilterBar.vue`: target picker, case toggle, draft/apply behavior, and animated Enter hint.
- `src/windows/FilterManagerWindow.vue`: filter-script CRUD, Monaco editing, and Log-ID/argument debug UI.
- `src/windows/launcher.ts`, `src/components/AppToolbar.vue`: filter manager entry point.
- `src/windows/ColumnManagerWindow.vue`: notify filter UI when global column scripts change.
- `docs/backend-api.md`, `docs/lua-api.md`: new API and Lua calling convention; remove history documentation.

### Task 1: Introduce the persisted filter model

**Files:**
- Modify: `crates/proxy-crab-mitm/src/model.rs`
- Modify: `crates/proxy-crab-mitm/src/workspace.rs`
- Test: `crates/proxy-crab-mitm/src/workspace.rs`

- [ ] **Step 1: Write model and workspace tests**

Add tests proving that a missing/legacy `view.json` receives the default filter, that filter state round-trips with the Session view, and that creating a new Session always uses `SessionView::default()` instead of cloning the active Session’s columns or filter.

The persisted types are:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FilterColumn {
    Method,
    Uri,
    Code,
    Source,
    Stage,
    Script { script_name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FilterOption {
    Column {
        column: FilterColumn,
        case_sensitive: bool,
    },
    Script {
        script_name: String,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionFilter {
    pub option: Option<FilterOption>,
    pub input: String,
}
```

Add `#[serde(default)] pub filter: SessionFilter` to `SessionView` and add `Filter` to `ScriptKind`.

- [ ] **Step 2: Run the focused tests and observe failure**

Run:

```bash
cargo test -p proxy-crab-mitm workspace::tests::session_filter_round_trip workspace::tests::new_session_uses_default_view
```

Expected before implementation: compilation fails because the filter types and fields do not exist.

- [ ] **Step 3: Implement model defaults and workspace layout**

Create `scripts/filter`, map `ScriptKind::Filter` to it, and remove active-Session view cloning from `Workspace::create_session`. Preserve backward compatibility only for stored JSON through Serde defaults; no API compatibility layer is needed.

- [ ] **Step 4: Run the focused tests**

Run:

```bash
cargo test -p proxy-crab-mitm workspace::tests
```

Expected: all workspace tests pass, including the new default-view and filter round-trip cases.

### Task 2: Execute parameterized Lua filters safely

**Files:**
- Modify: `crates/proxy-crab-mitm/src/lua.rs`
- Test: `crates/proxy-crab-mitm/src/lua.rs`

- [ ] **Step 1: Add failing Lua contract tests**

Test a script using the first chunk argument:

```lua
local needle = ...
return entry.req.uri.host == needle
```

Cover `true`, `false`, non-boolean return, runtime error, and instruction exhaustion. Syntax validation must identify the source as `filter.lua`.

- [ ] **Step 2: Run the Lua tests and observe failure**

Run:

```bash
cargo test -p proxy-crab-mitm lua::tests
```

Expected before implementation: the evaluator has no argument parameter and the new tests do not compile.

- [ ] **Step 3: Implement the evaluator**

Change the evaluator contract to:

```rust
pub fn evaluate_filter(
    source: &str,
    argument: &str,
    entry: &CaptureSummary,
) -> Result<bool>
```

Set `entry` in globals, compile the source as a Lua chunk, call it with the exact untrimmed UTF-8 input string, and require a boolean result. Keep the existing instruction and memory limits.

- [ ] **Step 4: Run the Lua tests**

Run:

```bash
cargo test -p proxy-crab-mitm lua::tests
```

Expected: all Lua tests pass.

### Task 3: Resolve and maintain Session filter references

**Files:**
- Modify: `crates/proxy-crab-mitm/src/runtime.rs`
- Modify: `crates/proxy-crab-mitm/src/workspace.rs`
- Test: `crates/proxy-crab-mitm/src/runtime.rs`

- [ ] **Step 1: Add failing reference-integrity tests**

Cover these cases:

- renaming a custom-column script rewrites both table-column references and Session filter references;
- deleting a custom-column script preserves stale table columns but resets every matching Session filter to `SessionFilter::default()`;
- renaming a filter script rewrites every matching Session filter;
- deleting a filter script resets every matching Session filter;
- reading a Session whose filter refers to a file deleted outside the application repairs and persists the default filter.

- [ ] **Step 2: Run the focused runtime tests and observe failure**

Run:

```bash
cargo test -p proxy-crab-mitm runtime::tests
```

Expected before implementation: the new `ScriptKind::Filter` and Session filter reference behavior are absent.

- [ ] **Step 3: Implement filter validation and reference rewrites**

Validate built-in column options directly. Resolve custom-column options against `ScriptKind::Column` and Lua filter options against `ScriptKind::Filter`. A missing script always repairs the whole filter to “未选择 + 空字符串”. Use the existing rollback pattern when rewriting multiple Session files during rename/delete.

- [ ] **Step 4: Run runtime and workspace tests**

Run:

```bash
cargo test -p proxy-crab-mitm runtime::tests workspace::tests
```

Expected: all focused tests pass.

### Task 4: Replace raw filter text with structured manager behavior

**Files:**
- Modify: `crates/proxy-crab-mgr/src/dto.rs`
- Modify: `crates/proxy-crab-mgr/src/manager.rs`
- Test: `crates/proxy-crab-mgr/src/manager.rs`

- [ ] **Step 1: Add failing manager tests**

Add coverage for:

- built-in column contains with case-sensitive and case-insensitive options;
- exact preservation of leading/trailing spaces;
- custom-column script output contains;
- custom-column evaluation errors silently producing no match;
- Lua filter script receiving the input parameter;
- Lua runtime errors, instruction exhaustion, and non-boolean results silently producing no match;
- an empty input returning all IDs while still persisting the selected option;
- an explicitly supplied filter being persisted only after the ID query succeeds;
- later requests with no filter payload using the persisted Session filter;
- debug execution returning `true`/`false` and surfacing Lua errors.

- [ ] **Step 2: Run the manager tests and observe failure**

Run:

```bash
cargo test -p proxy-crab-mgr manager::tests
```

Expected before implementation: structured filter fields and filter-script manager methods are missing.

- [ ] **Step 3: Implement matching and successful-apply persistence**

Change `LogIdsRequest.filter` to `Option<SessionFilter>`. `Some(filter)` means “test this draft and persist it after a successful ID scan”; `None` means “use the stored Session filter”. Matching rules:

```rust
match option {
    None => true,
    Some(_) if filter.input.is_empty() => true,
    Some(FilterOption::Column { .. }) => rendered_cell_contains_input,
    Some(FilterOption::Script { .. }) => evaluate_filter(...).unwrap_or(false),
}
```

For case-insensitive contains, compare Unicode-lowercased strings. A custom-column evaluation error is `false`. Infrastructure/storage failures remain request errors and must not replace the stored filter.

Add manager methods for filter-script list/get/create/update/delete and:

```rust
async fn debug_filter_script(
    &self,
    name: String,
    request: DebugFilterScriptRequest,
) -> ManagerResult<DebugFilterScriptPayload>;
```

The debug request contains `session_id`, `log_id`, and the exact `input`; unlike normal filtering, debug returns Lua errors.

- [ ] **Step 4: Run manager tests**

Run:

```bash
cargo test -p proxy-crab-mgr manager::tests
```

Expected: all manager tests pass.

### Task 5: Expose the new HTTP and Tauri APIs

**Files:**
- Modify: `crates/proxy-crab-mgr/src/http.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/api/types.ts`
- Modify: `src/api/backend.ts`
- Modify: `src/api/tauri-backend.ts`
- Test: `crates/proxy-crab-mgr/src/http.rs`

- [ ] **Step 1: Add route/serialization tests**

Verify these routes and payloads:

```text
GET    /api/filter-scripts
POST   /api/filter-scripts
GET    /api/filter-scripts/{name}
PUT    /api/filter-scripts/{name}
DELETE /api/filter-scripts/{name}
POST   /api/filter-scripts/{name}/debug
```

Remove `/api/filter-history` and all corresponding manager/Tauri/frontend methods. Verify `POST /api/logs/ids` accepts the structured `SessionFilter`.

- [ ] **Step 2: Run focused API tests and observe failure**

Run:

```bash
cargo test -p proxy-crab-mgr http::tests
```

Expected before implementation: filter-script routes are not registered and filter-history routes still exist.

- [ ] **Step 3: Implement HTTP, Tauri, and TypeScript bindings**

Keep method naming parallel to column scripts:

```ts
listFilterScripts(): Promise<Script[]>;
createFilterScript(request: ScriptRequest): Promise<void>;
getFilterScript(name: string): Promise<Script>;
updateFilterScript(name: string, request: UpdateScriptRequest): Promise<void>;
deleteFilterScript(name: string): Promise<void>;
debugFilterScript(name: string, request: DebugFilterScriptRequest): Promise<boolean>;
```

Represent the persisted option as discriminated TypeScript unions matching Serde’s tagged enums.

- [ ] **Step 4: Run API tests and TypeScript checking**

Run:

```bash
cargo test -p proxy-crab-mgr http::tests
pnpm exec vue-tsc --noEmit
```

Expected: route tests pass and TypeScript reports no errors.

### Task 6: Refactor the logs store around applied Session state

**Files:**
- Modify: `src/stores/logs.ts`
- Modify: `src/api/types.ts`

- [ ] **Step 1: Define the store invariants**

The store owns only the applied state:

```ts
appliedFilter: SessionFilter;
```

On Session switch it loads `SessionView.filter` before the first ID request. Polling and older-page requests omit a filter payload and therefore reuse backend-persisted state. Applying a draft sends it on the first ID request, waits for success, then updates `appliedFilter` and replaces displayed IDs. A failed apply preserves the old applied state and old ID list.

- [ ] **Step 2: Implement Session loading and application**

Remove `filterScript`, string-based `appliedFilter`, filter-history writes, and trimming. Define active filtering as `option !== null && input.length > 0`. Ensure Session switches cannot commit stale responses by checking `viewingSessionId` after every awaited request.

- [ ] **Step 3: Run TypeScript checking**

Run:

```bash
pnpm exec vue-tsc --noEmit
```

Expected: no store or API type errors.

### Task 7: Build the filter target picker and Enter hint

**Files:**
- Modify: `src/components/FilterBar.vue`
- Modify: `src/windows/ColumnManagerWindow.vue`
- Modify: `src/styles/base.css` only if a shared primitive is necessary

- [ ] **Step 1: Implement draft/applied separation**

Load the current Session filter into a local draft whenever the viewed Session changes. Preserve input text while switching among targets. Selecting a built-in/custom column or Lua filter script only changes the draft. Selecting “未选择” immediately applies `SessionFilter::default()`.

The target menu contains:

- “未选择”;
- a column section with `method`, `uri`, `code`, `source`, `stage`, and every global custom-column script;
- a “区分大小写” switch inside the column section;
- a filter-script section with every global filter script.

Custom columns do not need to be visible in the Session table. ID is not offered.

- [ ] **Step 2: Implement input behavior**

Disable the input when the draft option is absent. Enter applies the exact string. Escape and the clear button only clear the draft input. Option changes, case changes, and input changes all make the draft dirty. Failed application leaves it dirty and preserves the old applied filter.

- [ ] **Step 3: Implement restrained motion**

Place a return/Enter icon inside the leading edge of the input only while dirty. Animate scale and opacity continuously with a short, calm keyframe and reserve left padding only while the icon is visible. Add a `prefers-reduced-motion: reduce` rule that disables animation while retaining the icon.

- [ ] **Step 4: Refresh script choices**

Listen for `column-scripts-changed` and `filter-scripts-changed`. When a renamed/deleted option changes backend Session state, reload the applied state and option lists. Have `ColumnManagerWindow.vue` dispatch `column-scripts-changed` after create/rename/delete.

- [ ] **Step 5: Run frontend checks**

Run:

```bash
pnpm exec vue-tsc --noEmit
pnpm build
```

Expected: type checking and production build succeed.

### Task 8: Add the global filter-script manager

**Files:**
- Create: `src/windows/FilterManagerWindow.vue`
- Modify: `src/windows/launcher.ts`
- Modify: `src/components/AppToolbar.vue`

- [ ] **Step 1: Implement CRUD and editing**

Follow the existing column manager layout: script list and creation controls on the left, Monaco Lua editor on the right, dirty-change confirmation, rename, delete confirmation, save, and keyboard save. New scripts start with:

```lua
local input = ...
return false
```

Dispatch `filter-scripts-changed` after create, rename, delete, and save.

- [ ] **Step 2: Implement debug execution**

Provide Log ID and exact string-argument inputs plus a “保存并运行” action. Show `true` or `false` on success and the backend error text on failure. Require a viewed Session and a positive integer Log ID.

- [ ] **Step 3: Add the launcher and toolbar entry**

Add “过滤脚本” beneath “自定义列” in the existing “脚本” menu. Open a single `filter-manager` floating window sized consistently with the column manager.

- [ ] **Step 4: Run frontend checks**

Run:

```bash
pnpm exec vue-tsc --noEmit
pnpm build
```

Expected: type checking and production build succeed.

### Task 9: Remove history and update documentation

**Files:**
- Modify: `crates/proxy-crab-mitm/src/model.rs`
- Modify: `crates/proxy-crab-mitm/src/runtime.rs`
- Modify: `crates/proxy-crab-mgr/src/manager.rs`
- Modify: `crates/proxy-crab-mgr/src/http.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/api/backend.ts`
- Modify: `src/api/tauri-backend.ts`
- Modify: `src/components/FilterBar.vue`
- Modify: `docs/backend-api.md`
- Modify: `docs/lua-api.md`

- [ ] **Step 1: Remove filter-history code**

Delete `AppConfig.filter_history`, runtime/manager methods, HTTP route handlers, Tauri commands, TypeScript bindings, and the history dropdown. Existing JSON files may retain an ignored unknown `filter_history` field; no migration is required.

- [ ] **Step 2: Document the new contract**

Document structured filter options, successful-apply persistence, per-Session restoration, case behavior, error-as-non-match behavior, global filter-script CRUD/debug routes, and the Lua chunk-argument example:

```lua
local input = ...
return entry.req.uri.host:find(input, 1, true) ~= nil
```

- [ ] **Step 3: Scan for obsolete names**

Run:

```bash
rg -n "filter_history|FilterHistory|addFilterHistory|getFilterHistory|removeFilterHistory" \
  crates src src-tauri docs
```

Expected: no remaining implementation or documentation references.

### Task 10: Full verification and focused visual QA

**Files:**
- Verify all modified files without unrelated formatting or cleanup.

- [ ] **Step 1: Run Rust formatting and tests**

Run:

```bash
cargo fmt --all -- --check
cargo test --workspace
```

Expected: formatting check exits zero and all workspace tests pass.

- [ ] **Step 2: Run frontend type checking and build**

Run:

```bash
pnpm exec vue-tsc --noEmit
pnpm build
```

Expected: both commands exit zero.

- [ ] **Step 3: Inspect the final diff**

Run:

```bash
git diff --check
git status --short
git diff -- crates/proxy-crab-mitm crates/proxy-crab-mgr src-tauri src docs
```

Expected: no whitespace errors; every changed line maps to the confirmed filter refactor or the pre-existing interceptor work is left intact.

- [ ] **Step 4: Perform visual interaction QA**

Launch the app in the available local development mode and verify:

- no target disables the input and clears immediately;
- changing target/case/input does not alter rows until Enter;
- the return icon pulses only for a dirty draft and respects reduced motion;
- custom-column choices come from the global library, not visible table columns;
- Session switching restores the saved option and exact input;
- deleting or renaming a referenced script refreshes the picker safely;
- the filter manager can save and debug a script.

- [ ] **Step 5: Re-read the confirmed requirements**

Check every accepted requirement against the implementation and tests before reporting completion. Do not create a commit, push, or pull request unless the user separately requests it.
