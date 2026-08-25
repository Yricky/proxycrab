# Workspace Ownership Refactor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Tauri own app-data workspace selection, make CLI own its explicit workspace path, make the MITM runtime accept only a workspace root, and remove `GET /api/workspace` plus workspace selection from the manager contract.

**Architecture:** `ProxyCrab` opens one already-resolved workspace root and has no knowledge of application data or next-launch selection. Tauri persists `app_data_dir/config.json` in a focused local module and exposes the existing trusted commands from Tauri state. CLI includes its fixed workspace paths in its private bootstrap response instead of calling a public management API.

**Tech Stack:** Rust, Tauri 2, Axum, Vue/TypeScript, Serde, Cargo tests, Vue TypeScript compiler.

---

## File map

- Create `src-tauri/src/workspace_selection.rs`: Tauri-only pointer resolution, validation, persistence, and tests.
- Modify `crates/proxy-crab-mitm/src/runtime.rs`: keep one workspace-root constructor and remove app-data/workspace-path state.
- Modify `crates/proxy-crab-mitm/src/workspace.rs`: remove application pointer helpers while retaining workspace persistence helpers.
- Modify `crates/proxy-crab-mitm/src/model.rs`: remove the host-facing `WorkspacePaths` type.
- Modify `crates/proxy-crab-mgr/src/manager.rs`: remove workspace selection methods and initialize AgentsStore from `Workspace::root()`.
- Modify `crates/proxy-crab-mgr/src/http.rs` and permission contract: remove `GET /api/workspace`.
- Modify `src-tauri/src/lib.rs`: resolve workspace before opening runtime and serve trusted workspace commands from Tauri state.
- Modify `cli-app/src/main.rs`, `cli-app/src/ui.rs`, and `src/api/http-backend.ts`: expose the fixed CLI workspace through private bootstrap.
- Modify frontend permission metadata, README, backend API docs, ProxyCrab Skill reference, and evals to remove the public workspace API.

### Task 1: Move workspace pointer ownership to Tauri

**Files:**
- Create: `src-tauri/src/workspace_selection.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add Tauri-local selection tests**

Cover missing pointer initialization, invalid configured directory fallback, absolute-path validation, and the rule that changing the configured path does not change the current path.

- [ ] **Step 2: Run the focused test and verify it fails**

Run: `cargo test -p proxy-crab-t workspace_selection::tests`

Expected: failure because the module does not exist.

- [ ] **Step 3: Implement the selection module**

Define a serializable Tauri-local `WorkspacePaths`, an internal `{workspace_path}` pointer record, `WorkspaceSelection::open(app_data_dir)`, `paths()`, and `set_for_next_start(path)`. Preserve the existing `config.json` format, default `<app_data_dir>/workspace`, writable-directory probe, and atomic write behavior.

- [ ] **Step 4: Store selection in BackendState**

Resolve it during Tauri setup, open `ProxyCrab` with `selection.current_path()`, and make `get_workspace`/`set_workspace_for_next_start` call selection directly.

- [ ] **Step 5: Verify focused tests**

Run: `cargo test -p proxy-crab-t workspace_selection::tests`

Expected: all workspace selection tests pass.

### Task 2: Make MITM runtime workspace-root-only

**Files:**
- Modify: `crates/proxy-crab-mitm/src/runtime.rs`
- Modify: `crates/proxy-crab-mitm/src/workspace.rs`
- Modify: `crates/proxy-crab-mitm/src/model.rs`
- Modify: MITM and manager test call sites using `ProxyCrab::open`

- [ ] **Step 1: Collapse runtime construction**

Change `ProxyCrab::open` to accept the workspace root directly, create it, and build stores from `Workspace::open`. Remove `open_workspace`, `app_data_dir`, `workspace_paths`, and the workspace-path lock.

- [ ] **Step 2: Remove pointer helpers and model**

Delete `resolve_workspace`, `configure_workspace_for_next_start`, `configured_workspace`, `WorkspacePointer`, and `WorkspacePaths` from MITM. Keep generic workspace JSON helpers used by actual workspace data.

- [ ] **Step 3: Update tests mechanically**

Every test temporary directory becomes the workspace root. Replace path lookups through `workspace_paths().current_path` with `runtime.workspace().root()`.

- [ ] **Step 4: Verify MITM**

Run: `cargo test -p proxy-crab-mitm`

Expected: unit and proxy integration tests pass.

### Task 3: Remove workspace selection from Manager and HTTP API

**Files:**
- Modify: `crates/proxy-crab-mgr/src/manager.rs`
- Modify: `crates/proxy-crab-mgr/src/dto.rs`
- Modify: `crates/proxy-crab-mgr/src/http.rs`
- Modify: `crates/proxy-crab-mgr/src/permission.rs`
- Modify: `crates/proxy-crab-mgr/tests/permission_contract.rs`
- Modify: desktop and CLI permission-store migration expectations

- [ ] **Step 1: Remove manager methods**

Delete `workspace()` and `set_workspace_for_next_start()` from `ProxyCrabManager` and `MitmManager`. Initialize AgentsStore with `runtime.workspace().root()`.

- [ ] **Step 2: Remove public route and permission action**

Delete `GET /api/workspace`, its handler, action label, and permission catalog entry. Add it to obsolete permission IDs so existing permission files normalize instead of failing closed.

- [ ] **Step 3: Update contract tests**

Assert both GET and PUT workspace actions are absent and that requests to `/api/workspace` return not found or method not allowed.

- [ ] **Step 4: Verify manager package**

Run: `cargo test -p proxy-crab-mgr`

Expected: manager tests and permission contract pass.

### Task 4: Preserve trusted Tauri and CLI UI behavior

**Files:**
- Modify: `cli-app/src/main.rs`
- Modify: `cli-app/src/ui.rs`
- Modify: `src/api/http-backend.ts`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add workspace data to CLI bootstrap**

Pass the canonical CLI workspace into the private UI router. Return `{current_path, configured_path}` with identical values from `/ui-api/bootstrap`.

- [ ] **Step 2: Use bootstrap data in the frontend**

Extend `CliBootstrap` and make HTTP Backend `getWorkspace()` return the bootstrap value without calling `/api/workspace`.

- [ ] **Step 3: Keep Tauri mutation trusted**

Keep the Tauri commands and `HostOperations` frontend interface unchanged; only their implementation owner changes from Manager to `WorkspaceSelection`.

- [ ] **Step 4: Verify frontend types**

Run: `pnpm exec vue-tsc --noEmit`

Expected: no TypeScript errors.

### Task 5: Synchronize documentation and Skill artifacts

**Files:**
- Modify: `README.md`
- Modify: `docs/backend-api.md`
- Modify: `skills/proxycrab/SKILL.md`
- Modify: `skills/proxycrab/references/http-api.md`
- Modify: `skills/proxycrab/evals/evals.json`
- Modify: `src/utils/permission-meta.ts`

- [ ] **Step 1: Remove the public workspace API documentation**

Document workspace selection as Tauri-only plus CLI startup configuration. Remove `WorkspacePaths` and `GET /api/workspace` from Agent HTTP references and permission metadata.

- [ ] **Step 2: Add an eval boundary case**

Add an evaluation requiring an Agent not to call `/api/workspace` and to explain that workspace selection belongs to the host UI or CLI startup arguments.

- [ ] **Step 3: Validate artifacts**

Run: `node -e "JSON.parse(require('fs').readFileSync('skills/proxycrab/evals/evals.json','utf8'))"`

Expected: exit zero.

### Task 6: Full verification

**Files:**
- Verify all modified files.

- [ ] **Step 1: Check removed symbols**

Run: `rg 'app_data_dir|set_workspace_for_next_start|workspace_paths|/api/workspace' crates/proxy-crab-mitm crates/proxy-crab-mgr`

Expected: no production matches except obsolete permission migration text for `/api/workspace`.

- [ ] **Step 2: Run formatting and all Rust tests**

Run: `cargo fmt --all -- --check && cargo test --workspace`

Expected: all tests pass.

- [ ] **Step 3: Build both frontend targets and run unit tests**

Run: `pnpm build:tauri && pnpm run test:unit`

Expected: both bundles build and all unit tests pass.

- [ ] **Step 4: Inspect final diff**

Run: `git diff --check && git status --short`

Expected: no whitespace errors; existing user changes remain intact.
