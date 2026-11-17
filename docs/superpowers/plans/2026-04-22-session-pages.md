# Session pages implementation plan

**Goal:** Import reusable ZIP pages into a workspace and include selected pages in existing Session shares.

**Architecture:** Store each page as one validated ZIP in `session-pages/`. The shared manager owns page storage; SessionShareService owns sharing. Package routes authorize all files through the existing share token or package cookies. A native ES module `init.js` exports an initialized, Session-scoped Backend. Desktop and CLI reuse the same page runtime and management window.

**Confirmed behavior:** Asset menu opens page management with a page list and package details. Pages run through Session sharing links. Import/export/delete/confirmed whole-package replacement. ZIP root contains manifest and index; init.js is reserved. Shared pages have separate paths and one Session token; share selection is persisted. Missing pages are filtered automatically. No visitor ZIP download, navigation switcher, extra origin isolation, or online editor.

- [x] Implement validated ZIP storage and manager methods. Validate manifest versions/ID, limits, paths, duplicate entries, symlinks, required files, and reserved init.js. Verify atomic replacement and export.
- [x] Extend Session sharing with selected page IDs, persistent compatibility, missing-page cleanup, package authorization, resource cookies, and path routing. Cover revocation, cross-Session scope, concurrent tokens and traversal.
- [x] Add permission-controlled HTTP management routes and trusted desktop commands; publish page changes to both UIs.
- [x] Build the page runtime as a stable ES module reusing createShareBackend. Supply init.js from the package route, expose accurate Backend types, and preserve unsupported methods as absent.
- [x] Implement dense page management window with import/export/delete/replace and package details; add capability-gated asset menu entry.
- [x] Add page selection and per-page links to existing Session sharing UI, preserving token and partial-update semantics.
- [x] Document page format, native import, Backend methods, and a runnable example. Update bundled Skill references, management script, and evaluation case.
- [x] Run focused Rust and frontend tests, production build, desktop/CLI checks, and browser verification where available; review final diff.

No repository integration or deployment is requested.
