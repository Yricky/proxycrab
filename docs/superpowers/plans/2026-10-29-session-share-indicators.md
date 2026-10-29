# Session Share Indicators Implementation Plan

**Goal:** Display a compact sharing icon before shared Session names using one summary request.

**Architecture:** GET /api/session-shares and list_session_shares IPC return {shares:[{session_id,persistent}]} for enabled, non-archived Session page shares only. A frontend map drives SessionListItem and refreshes on Session lifecycle/share changes. No token or redaction binding is returned.

- [x] Add service list, read-only permission, HTTP endpoint and IPC; test scope, token exclusion, revocation and authorization.
- [x] Add optional listSessionShares backend capability, shared state cache, stale-read protection, and local/remote refresh paths.
- [x] Add a 12px icon before shared Session names, preserving row height and providing temporary/persistent tooltips.
- [x] Update Skill, relevant scripts/evals and backend/project documentation.
- [x] Run targeted Rust/frontend tests, production build, desktop check, script tests and isolated browser integration checks.

**Validation:** 106 management tests, 117 frontend tests, 13 desktop permission tests and 20 Skill script tests passed. Production frontend build and CLI build passed. Isolated browser integration confirmed compact icons, long-name truncation, unchanged row height, no initial per-Session share reads, HTTP/local toggle synchronization, archive/restore and exclusion of HAR-only shares. Independent review findings were fixed and rechecked.
