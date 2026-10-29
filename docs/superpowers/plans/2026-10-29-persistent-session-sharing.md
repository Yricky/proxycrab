# Persistent Session Sharing Implementation Plan

**Goal:** Add optional workspace-persistent Session shares while keeping temporary shares the default.

**Architecture:** SessionShareService loads session-shares.json from the selected workspace. Only persistent entries are saved, using an atomic private file replacement before committing memory changes. Existing query-token read routes remain unchanged. Share mode changes preserve tokens; disable removes durable credentials.

**Tech Stack:** Rust, Axum, Tauri, Vue, TypeScript.

- [x] Add service lifecycle tests in crates/proxy-crab-mgr/src/session_share.rs for restart, mode conversion, revocation, redaction, corrupt files, and failed writes.
- [x] Implement open(workspace), persistent state/options, atomic writes, and cleanup on permanent deletion; connect desktop and CLI startup.
- [x] Reuse POST /api/session-shares and enableSessionShare(sessionId, sessionShareConfig). Config groups redactionScript and persistent; supplying config updates enabled shares without rotating tokens, omitting config preserves existing state. No new routes or IPC commands.
- [x] Use the existing backend capability and compact 重启后保留 checkbox in SessionExportShareWindow.vue; pass persistence on initial enable.
- [x] Verify the unchanged API permission catalog, update backend tests, README, Skill HTTP reference, and eval scenarios.
- [x] Run cargo test -p proxy-crab-mgr --offline, relevant CLI/desktop checks, frontend tests/build, Skill script tests, and git diff --check. Review changes against agreed lifecycle semantics.

**Accepted behavior:** Persist token plaintext and redaction binding; automatically restore enabled shares; mode conversion preserves token; close/reopen rotates token. Archived shares remain unavailable until restored. Permanent deletion removes credentials. IP/port changes still require an updated URL. File load failures are surfaced without overwriting damaged data.

**Validation:** Management tests: 94 unit + 3 permission + 7 redaction tests passed. Frontend: 107 tests and production build passed. All 19 Skill script tests passed. Desktop cargo check passed. Isolated CLI/browser E2E verified restored token/binding, checkbox config updates through existing POST, disable across restart, token rotation, and temporary expiry.
