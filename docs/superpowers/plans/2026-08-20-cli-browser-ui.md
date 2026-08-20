# CLI Browser UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Serve the existing ProxyCrab Vue application from the headless CLI, authenticate each CLI UI session with an in-memory access token, and reuse the same business UI through target-specific Backend implementations.

**Architecture:** A target Landing component installs either the Tauri or HTTP Backend before mounting the shared App. The CLI embeds its Vite build, serves public static assets plus token-protected UI administration routes on the existing loopback management server, and treats persisted `approval` permissions as deny without creating approval state.

**Tech Stack:** Vue 3, TypeScript, Vite, Rust 2024, Axum, Tokio, Tauri 2

---

### Task 1: Runtime Backend and target bootstrap

**Files:**
- Create: `src/api/runtime-backend.ts`
- Create: `src/Root.vue`
- Create: `src/landing/TauriLanding.vue`
- Create: `src/landing/CliLanding.vue`
- Modify: `src/api/index.ts`
- Modify: `src/main.ts`
- Modify: `src/stores/*.ts`

- [ ] Add a set-once runtime Backend registry exposed as `window.proxyCrabBackend`.
- [ ] Remove module-load-time Tauri Backend creation from every Store.
- [ ] Mount the shared `App.vue` only after the selected Landing installs a Backend.
- [ ] Verify both targets type-check.

### Task 2: Backend portability and builds

**Files:**
- Create: `src/api/http-backend.ts`
- Create: `src/api/target.ts`
- Modify: `src/api/backend.ts`
- Modify: `src/api/body.ts`
- Modify: `src/stores/http-api-sync.ts`
- Modify: `src/windows/BrowserWindow.vue`
- Modify: `src/windows/CertManagerWindow.vue`
- Modify: `vite.config.ts`
- Modify: `package.json`
- Modify: `src-tauri/tauri.conf.json`

- [ ] Add target capabilities, raw body loading, change subscriptions, and external URL opening to Backend.
- [ ] Implement the HTTP success/error envelope and authenticated raw-body requests.
- [ ] Produce separate `dist/tauri` and `dist/cli` builds without Tauri imports in the CLI entry graph.
- [ ] Verify both Vite builds succeed.

### Task 3: CLI UI authentication and hosting

**Files:**
- Create: `cli-app/src/ui.rs`
- Modify: `cli-app/src/main.rs`
- Modify: `cli-app/Cargo.toml`
- Modify: `crates/proxy-crab-mgr/src/http.rs`

- [ ] Generate a 256-bit per-run UI token and print the URL and token to the terminal.
- [ ] Let the CLI permission service recognize the UI token as trusted while ordinary identities retain allow/deny checks.
- [ ] Add a UI-only router protected by the token and embed the CLI frontend assets.
- [ ] Expose bootstrap, local-IP, regex validation, Agent preset, permission, and change-stream operations required by Backend. Keep Skill installation Tauri-only because the CLI page may be remotely deployed.
- [ ] Verify invalid tokens fail and trusted UI requests bypass ordinary local denial.

### Task 4: CLI capability behavior

**Files:**
- Modify: `src/App.vue`
- Modify: `src/components/PermissionEditor.vue`
- Modify: `src/windows/SettingsWindow.vue`
- Modify: `src/components/AppToolbar.vue`
- Modify: `cli-app/src/permissions/*`

- [ ] Disable approval startup/UI for CLI.
- [ ] Restrict CLI permission editing to allow/deny and evaluate stored approval as deny.
- [ ] Make the CLI workspace path read-only because `--workspace` owns startup selection.
- [ ] Verify Tauri retains all current capabilities.

### Task 5: Documentation and verification

**Files:**
- Modify: `README.md`
- Modify: `docs/backend-api.md`
- Modify: `skills/proxycrab/SKILL.md`
- Modify: `skills/proxycrab/references/http-api.md`
- Modify: tests adjacent to changed Rust and TypeScript modules

- [ ] Document the CLI UI URL/token flow and UI-only endpoints.
- [ ] Document that CLI maps approval to deny.
- [ ] Run frontend unit tests and both frontend builds.
- [ ] Run Rust formatting, checks, and targeted/full tests.
