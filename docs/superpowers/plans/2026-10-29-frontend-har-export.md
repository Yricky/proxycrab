# Frontend HAR export implementation plan

**Goal:** Export the displayed Session selection as HAR from desktop and shared browser UI, without backend HAR generation or HAR sharing.

**Architecture:** `saveFile({filename})` returns a Writer (`append`, `markAsFinish`, `markAsError`). Desktop writers append binary IPC chunks to same-directory temporary files and publish only on finish; browser writers accumulate Blob parts. A floating export window freezes the current Session/ordered IDs when its export button is pressed, reports progress/logs, and cancels on close. HAR is generated one entry at a time from existing read APIs; Body read failures become explanatory content. Ordinary HTTP failures/in-progress rows remain; synthetic CONNECT and WebSocket connection rows are skipped.

**Tech Stack:** Vue/TypeScript, Tauri IPC, Rust/tempfile, Vitest/cargo tests.

Execution stays in this chat, without subagents, per user/repository instructions.

- [x] Replace saveFile contract and migrate certificate, QR, and WebSocket downloads; test writer ordering, cancellation, terminal states, browser commit/abort, native file preservation.
- [x] Add HAR serializer/export coordinator and export window; test binary/text bodies, failed/missing responses, Body errors, skips, order/progress and commit failures.
- [x] Add toolbar entry gated by read/save methods, independent of active Session; verify window opens before export starts and closes safely.
- [x] Remove backend HAR serializer/service, DTOs, routes, IPC, permissions, UI card, helpers and obsolete tests. Retain ordinary Session sharing and read-time redaction.
- [x] Update README, API docs, Skill references/scripts/evals; assert removed routes are unavailable.
- [x] Run frontend tests/build, Rust workspace tests/checks, and inspect export UI. Record concrete results and remaining limits.

Validation completed:
- `pnpm test:unit`: 130 tests passed.
- `pnpm build`: typecheck and production build passed; existing large-chunk advisory remains.
- `cargo test --workspace --offline`: 343 tests passed, 1 pre-existing >1 GiB download test ignored.
- `node --test skills/proxycrab/scripts/*.test.mjs`: 14 tests passed.
- Playwright with actual Vue controls and a fixture Backend: export starts only inside the window,
  frozen order survives list changes, Body failure still produces parseable downloaded HAR, progress
  and warnings render, cancel/close produce no download, shared read-only export remains available,
  and sweep confirmation cancel/confirm branches both work. Screenshots and sample HAR are in target/.
- `git diff --check`: passed. Browser harness files were removed after verification.
