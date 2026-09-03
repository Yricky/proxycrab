# Skill Management Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Replace the desktop-only one-shot ProxyCrab Skill installer with persisted multi-path management that synchronizes every configured path at application startup and on demand.

**Architecture:** Keep `proxy-crab-mgr` as the transport-neutral owner of safe directory replacement used by both the CLI installer and desktop synchronization. Add a Tauri-local `SkillManager` that owns the application-data configuration file, startup adoption, per-path synchronization state, and optional cleanup of removed paths. Expose only get/save/sync host commands to the Vue application; the browser and share backends remain incapable of managing local Skill paths.

**Tech Stack:** Rust 2024, Serde JSON, Tauri 2 commands, Vue 3 Composition API, TypeScript 6.

---

### Task 1: Extend the low-level Skill filesystem operations

**Files:**
- Modify: `crates/proxy-crab-mgr/src/skill_install.rs`

- [x] **Step 1: Add failing tests for normalized parent paths and removal**

Add tests proving that absolute and `~/...` parent paths normalize to absolute paths, duplicate spellings can be compared after normalization, removal deletes only `<parent>/proxycrab`, and a missing target is a successful no-op.

- [x] **Step 2: Run the focused tests and verify they fail**

Run: `cargo test -p proxy-crab-mgr skill_install -- --nocapture`

Expected: FAIL because public normalization and removal operations do not exist.

- [x] **Step 3: Implement the minimum reusable operations**

Expose the existing parent expansion as a normalized absolute-path operation and add a safe removal function that always derives the fixed `proxycrab` child from a validated parent. Preserve the existing atomic temporary-directory/backup install behavior so `proxycrab-cli install-skill` continues to work unchanged.

- [x] **Step 4: Re-run the focused tests**

Run: `cargo test -p proxy-crab-mgr skill_install -- --nocapture`

Expected: PASS.

### Task 2: Add persisted desktop Skill management

**Files:**
- Create: `src-tauri/src/skill_manager.rs`
- Modify: `src-tauri/src/lib.rs`

- [x] **Step 1: Write failing `SkillManager` tests**

Cover these exact states: missing config plus missing default Skill reports setup-required without creating a config; missing config plus an existing default Skill creates a one-path config and synchronizes it; an existing empty config is valid and does not warn; corrupt config is preserved and reported; saving normalizes and deduplicates paths; saving can keep or delete removed Skill directories; one synchronization failure does not prevent successful paths from updating.

- [x] **Step 2: Run the focused Tauri tests and verify they fail**

Run: `cargo test -p proxy-crab-t skill_manager -- --nocapture`

Expected: FAIL because the module and types do not exist.

- [x] **Step 3: Implement the Tauri-local manager**

Persist this application-data-owned shape in `skill-paths.json`:

```json
{
  "paths": ["/absolute/parent/path"]
}
```

Return a runtime state containing `configured`, optional `config_error`, and one entry per configured parent with its derived target and optional synchronization error. On open, preserve unreadable/corrupt configuration, otherwise adopt `~/.agents/skills` only when no config exists and its `proxycrab` child already exists. Synchronize all configured entries independently by calling the low-level installer with overwrite enabled.

- [x] **Step 4: Replace the one-shot Tauri commands**

Store `SkillManager` in `BackendState` and replace install-info/check/install commands with:

```rust
fn get_proxycrab_skill_manager_state(...) -> Result<SkillManagerState, ManagerError>;
fn save_proxycrab_skill_paths(paths: Vec<String>, delete_removed: bool, ...) -> Result<SkillSaveResult, ManagerError>;
fn sync_proxycrab_skills(...) -> Result<SkillManagerState, ManagerError>;
```

Initialize it during Tauri setup so startup synchronization completes independently of Vue mounting and failures never abort the application.

- [x] **Step 5: Re-run the focused tests**

Run: `cargo test -p proxy-crab-t skill_manager -- --nocapture`

Expected: PASS.

### Task 3: Refactor the frontend host API and state

**Files:**
- Modify: `src/api/types.ts`
- Modify: `src/api/backend.ts`
- Modify: `src/api/tauri-backend.ts`
- Modify: `src/stores/skill.ts`
- Modify: `src/AppMain.vue`

- [x] **Step 1: Replace install DTOs with management DTOs**

Define TypeScript types matching the Rust state and save result. Replace `SkillInstaller` with a `SkillManager` host capability exposing `getState`, `savePaths`, and `sync`.

- [x] **Step 2: Model only the requested warning states**

Make the store warn when setup is required, configuration cannot be read, or at least one configured path has a synchronization error. Initial state remains non-warning to avoid a startup flash; `AppMain` refreshes the native startup result.

- [x] **Step 3: Run the frontend typecheck**

Run: `pnpm exec vue-tsc --noEmit`

Expected: FAIL until all old installer consumers are migrated in Task 4.

### Task 4: Build the Skill management window and removal choice

**Files:**
- Move: `src/windows/SkillInstallWindow.vue` to `src/windows/SkillManagerWindow.vue`
- Modify: `src/windows/launcher.ts`
- Modify: `src/components/AppToolbar.vue`
- Modify: `src/stores/dialog.ts`
- Modify: `src/components/ConfirmDialog.vue`

- [x] **Step 1: Add a backward-compatible three-way dialog result**

Keep existing `confirmDialog()` callers returning boolean. Add a choice API capable of returning primary, secondary, or cancel so removal save can offer “仅停止管理”, “停止管理并删除”, and “取消” without nesting confirmations.

- [x] **Step 2: Replace the install form with a draft path list**

The window loads saved absolute parent paths, accepts absolute or `~/...` input, adds/removes only in local draft state, marks new entries `未同步`, shows only an error marker and short error for failed saved entries, and displays a permanent notice that synchronization completely replaces each `<parent>/proxycrab` directory.

- [x] **Step 3: Implement explicit save and sync-all actions**

Disable “同步全部” while the draft is dirty. Saving without removals persists and immediately synchronizes. Saving with removals opens the three-way choice; cancel keeps the draft unsaved, the secondary action keeps removed directories, and the primary action deletes them. Show any deletion failures as toasts while accepting the saved list returned by the backend.

- [x] **Step 4: Rename all user-facing entry points**

Rename the floating window and launcher function to Skill management, change the AI menu label to “管理 ProxyCrab Skill…”, and replace installed/mismatched toolbar styling with the store's setup/synchronization warning.

- [x] **Step 5: Build the frontend**

Run: `pnpm build`

Expected: PASS with no Vue or TypeScript errors.

### Task 5: Synchronize documentation and Skill references

**Files:**
- Modify: `README.md`
- Modify: `docs/backend-api.md`
- Modify: `skills/proxycrab/references/http-api.md`
- Check: `skills/proxycrab/SKILL.md`
- Check: `skills/proxycrab/scripts/`
- Check: `skills/proxycrab/evals/evals.json`

- [x] **Step 1: Document the trusted-host boundary and lifecycle**

State that desktop Skill paths are stored in application data, adopted only from an existing default installation on first use, synchronized with full replacement at startup/save/manual sync, and optionally deleted when removed. Preserve the statement that browser backends expose no local Skill-management operation and that the CLI keeps the explicit `install-skill` command.

- [x] **Step 2: Check bundled Skill artifacts**

Confirm the agent-facing ProxyCrab API, scripts, and evaluation behavior are unchanged. Update only wording that incorrectly describes the desktop host operation as a one-shot installer; do not add an Agent HTTP route or evaluation for a Tauri-only feature.

- [x] **Step 3: Validate documentation references**

Run: `rg -n "SkillInstall|skillInstaller|openSkillInstall|Skill installation|Skill-install|安装 ProxyCrab Skill" src src-tauri README.md docs/backend-api.md skills/proxycrab`

Expected: no stale desktop one-shot installer names; CLI-specific installation wording remains only where intentional.

### Task 6: Full verification and review

**Files:**
- Check: all modified files

- [x] **Step 1: Format and run focused tests**

Run: `rustfmt --edition 2024 --check crates/proxy-crab-mgr/src/skill_install.rs src-tauri/src/skill_manager.rs src-tauri/src/lib.rs`

Run: `cargo test -p proxy-crab-mgr skill_install -- --nocapture`

Run: `cargo test -p proxy-crab-t skill_manager -- --nocapture`

Expected: all commands exit 0.

- [x] **Step 2: Run workspace and frontend verification**

Run: `cargo test --workspace`

Run: `cargo clippy -p proxy-crab-mgr -p proxy-crab-t --all-targets --no-deps -- -D warnings -A clippy::collapsible-if`

Run: `node --experimental-strip-types --test src/utils/*.test.mts` with Node 22 or newer.

Run: `pnpm build`

Run: `node --test skills/proxycrab/scripts/lib/common.test.mjs`

Run: `node -e 'JSON.parse(require("fs").readFileSync("skills/proxycrab/evals/evals.json", "utf8"))'`

Expected: all commands exit 0 with zero test failures.

- [x] **Step 3: Review the final diff against the clarified requirements**

Run: `git diff --check`

Run: `git status --short`

Inspect: `git diff -- crates/proxy-crab-mgr/src/skill_install.rs src-tauri/src src/api src/stores src/windows src/components README.md docs/backend-api.md skills/proxycrab docs/superpowers/plans/2026-09-03-skill-management.md`

Expected: every changed line maps to Skill management, tests, or required documentation, and no unrelated user changes are overwritten.
