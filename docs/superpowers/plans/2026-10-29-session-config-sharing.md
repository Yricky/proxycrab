# Session configuration sharing and intent import

**Goal:** Share live session configuration ZIPs through an optional link capability; import explicitly accepted session intents by dropping ZIP files.

**Architecture:** A transport-neutral bundle service owns ZIP validation, dependency collection, temporary imports and conflict decisions. Workspace applies file changes and newly created sessions as one rollback-capable batch. HTTP and Tauri expose the same manager operations; share tokens authorize downloads only. No activation, Lua analysis, resource renaming or Lua rewriting.

**Confirmed behavior:** ZIP paths mirror workspace paths. `intents.json` version 1 contains unique `id`, `type: session`, `path`, and `assets` (exact IDs or trailing-slash prefixes). Live export includes saved view/interceptors and their scripts, selected assets and metadata. Missing references fail. Initial import takes file plus nonempty accepted IDs, executes immediately without conflicts or returns an opaque temporary import ID with file statuses. Resolution rechecks targets. Same files reuse; asset conflicts keep/overwrite; script conflicts additionally rename with configuration reference updates. Any failure rolls back the entire batch.

- [x] Bundle backend: add ZIP parsing/writing, bounded validated paths, explicit intent selection, dependency validation, temporary packages, conflict detection/resolution and workspace rollback. Test round trips, subset selection, missing dependencies, conflicts, rename and rollback.
- [x] Transports: manager operations, HTTP import/resolve endpoints and permissions/change events; Tauri command adapters. Share settings/state/persistence/bootstrap capability and token-authorized ZIP download.
- [x] Frontend: ZIP intent preview on file drop in management UI; selection and conflict window; sharing resource rules editor and capability-gated download next to HAR. Reuse window, tree and file writer conventions.
- [x] Documentation: bundle protocol/API, proxycrab skill references/scripts/evals; Lua asset best practices.
- [x] Verification: focused Rust and frontend tests, frontend build, workspace cargo check. Inspect diff and resolve failures before completion.

Implementation proceeds inline under the user's no-subagent instruction. Do not commit unrelated changes.

Verification completed: frontend production build and 152 unit tests; management backend 96 tests; MITM library 145 tests; entire Rust workspace check; 23 skill helper/common tests. Chromium end-to-end run in a temporary CLI workspace verified owner asset-prefix selection, share-only ZIP download, file-drop intent selection, script rename conflict resolution, unchanged active session and live revocation, with no page errors. Screenshots were visually inspected. No subagents or commits were used.
