# Read-time redaction implementation plan

> Execute inline in the current conversation. The user explicitly forbids subagents. Preserve the existing staged changes and the original capture/forwarding pipeline.

**Goal:** Bind one global Lua redaction script to a local identity, API key, Session share, or HAR share and apply it only to reads.

**Architecture:** Authentication resolves a binding into a request-scoped reader. A reusable Lua evaluator produces a per-record/per-snapshot read view with immutable originals and transactional output. Header and body availability advances independently; body implies header availability. Existing forwarding and persisted captures stay unchanged.

**Tech Stack:** Rust, mlua/Lua 5.4, Axum, Vue/TypeScript, existing script library UI.

## Contract

- Globals: `req`, `resp`, `context.api`, `context.req.header/body`, `context.resp.header/body`.
- States: `unloaded`, `loaded`, `redacted`. Reads always expose this record/snapshot's pre-redaction values. Only parts `loaded` at invocation start accept writes. A successful invocation marks every loaded part redacted, even without writes; failure commits nothing and refuses output.
- Request header state includes URI. URI-only filtering exposes only URI; method/status are read-only. Other built-in filters and empty filters do not run redaction. Custom filters/columns see sanitized input.
- Unbound reads use existing paths. Missing/broken bound scripts fail closed. Interceptor modification history and script source are out of scope; snapshots are in scope.
- Body sources remain lazy and use original encoding metadata for reads. Output replacements have coherent transport metadata.
- HAR pins a script per download, redacts each entry before serialization, and terminates the stream on failure without sending the failing entry.

## Tasks and verification

- [x] Lua engine in `crates/proxy-crab-mitm/src/lua/redaction.rs`; register `ScriptKind::Redaction` and its directory. Test immutable reads, cumulative writes, stage completion without edits, failed transaction, URI-only access, lazy body access, unchanged originals and repeated stages.
- [x] Request-scoped reader and global script CRUD in `crates/proxy-crab-mgr/src/manager.rs`; adapt ID filtering, columns, detail, body, snapshots, breakpoint summaries and bypass URIs. Test built-in fast paths, sanitized script inputs and failure propagation.
- [x] Binding persistence in CLI/desktop permission stores and sharing services; expose owner-only binding updates and script CRUD. Add the resolved binding to authenticated reads without changing original network processing.
- [x] HAR redaction in `crates/proxy-crab-mgr/src/har.rs`; test derived URL/cookie/redirect fields and error termination.
- [x] Frontend backend contracts, reusable script library, identity/share selectors and read revision invalidation. Validate `pnpm exec vue-tsc --noEmit` and frontend unit tests.
- [x] Update `docs/backend-api.md`, `docs/lua-api.md`, `skills/proxycrab/` references/evaluations. Run targeted Rust tests, then workspace checks and relevant tests; inspect the final diff for unrelated changes.

Commands: `cargo test -p proxy-crab-mitm redaction`, `cargo test -p proxy-crab-mgr`, `cargo test -p proxycrab-cli`, `cargo check --workspace`, `pnpm test:unit`, `pnpm build` (use actual package names from manifests). No Gradle or external-device operations.

## Verification completed

- `cargo test --workspace --offline`: 297 passed, 0 failed; the existing >1 GiB native download test remains ignored.
- `pnpm test:unit`: 58 passed.
- Node Skill-script tests: 11 passed, including both share binding routes and credential-free output.
- `pnpm build`: successful (existing Monaco chunk-size warning only).
- `git diff --check`: clean.
- Reviewed raw-read/output separation, failure propagation through Lua pcall and filter non-match handling, per-snapshot availability, original Body reuse across stages, HAR version pinning/termination, owner binding persistence and frontend invalidation.
- No subagents used. Changes remain in the shared working tree; no commit was created by this task.
