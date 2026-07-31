# Tag Session Routing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Route HTTP requests and HTTPS CONNECT tunnels into Sessions through a selected Lua script that returns globally unique tags, while persistently recording all bypassed traffic.

**Architecture:** Session metadata owns a sorted tag set and the workspace enforces a global one-tag-to-one-Session invariant with atomic cross-Session updates. The proxy resolves a selected routing script before capture: direct HTTP is evaluated per request, CONNECT is evaluated once and pins the selected Session for the tunnel, while bypass traffic is streamed without TLS interception and recorded in a workspace-level SQLite database. Manager, HTTP, Tauri, and Vue layers expose the same tag/routing/bypass model and remove the old active-Session and script-rename concepts.

**Tech Stack:** Rust 2024, Tokio, Hyper 1, mlua 0.11, rusqlite, Axum, Tauri 2, Vue 3, TypeScript 6.

---

### Task 1: Session tags and immutable script names

**Files:**
- Modify: `crates/proxy-crab-mitm/src/model.rs`
- Modify: `crates/proxy-crab-mitm/src/workspace.rs`
- Modify: `crates/proxy-crab-mitm/src/runtime.rs`
- Test: `crates/proxy-crab-mitm/src/workspace.rs`
- Test: `crates/proxy-crab-mitm/src/runtime.rs`

- [x] **Step 1: Add failing Session tag tests**

Add tests proving:

```rust
assert_eq!(workspace.create_session(None, None).unwrap().tags, vec!["default"]);
assert_eq!(workspace.create_session(None, None).unwrap().tags, Vec::<String>::new());
assert!(validate_tag("checkout_2").is_ok());
assert!(validate_tag("Checkout").is_err());
```

Add an atomic move test that assigns `checkout` to Session B and verifies it is removed from Session A. Add a concurrent get-or-create test proving two callers resolving the same missing tag obtain one Session.

- [x] **Step 2: Run core tests and observe failure**

Run:

```bash
cargo test -p proxy-crab-mitm workspace::tests runtime::tests
```

Expected: compilation/test failure because tag fields and APIs do not exist.

- [x] **Step 3: Implement the tag model**

Change `SessionMetadata` to include:

```rust
#[serde(default)]
pub tags: Vec<String>,
```

Remove `AppConfig.active_session_id`. Add `ScriptKind::Routing`. Validate tags with `^[a-z0-9_]{1,64}$` semantics without normalization. Make manual creation assign `default` only when no Session exists. Implement one locked workspace transaction that validates, sorts, deduplicates, moves conflicting tags, writes all changed metadata atomically with rollback, and publishes the new in-memory Session list only after every file write succeeds.

Implement `resolve_or_create_tag(tag, script_name)` under the same Session lock. Missing tags create:

```rust
SessionMetadata {
    name: tag.to_string(),
    description: Some(format!("由分流脚本「{script_name}」自动创建")),
    tags: vec![tag.to_string()],
    ..
}
```

Automatic creation must not add `default` unless the returned tag is `default`.

- [x] **Step 4: Remove script rename behavior**

Replace `update_script(kind, old_name, Script)` with a content-only update:

```rust
pub fn update_script(&self, kind: ScriptKind, name: &str, content: String) -> Result<()>;
```

Remove rename helpers. On deletion, remove every Session reference for custom columns, filters, and interceptors; deleting the selected routing script clears its selection.

- [x] **Step 5: Run core tests**

Run:

```bash
cargo test -p proxy-crab-mitm workspace::tests runtime::tests
```

Expected: all selected tests pass.

### Task 2: Lua routing and persistent bypass traffic

**Files:**
- Create: `crates/proxy-crab-mitm/src/bypass.rs`
- Modify: `crates/proxy-crab-mitm/src/lib.rs`
- Modify: `crates/proxy-crab-mitm/src/lua.rs`
- Modify: `crates/proxy-crab-mitm/src/proxy.rs`
- Modify: `crates/proxy-crab-mitm/src/runtime.rs`
- Test: `crates/proxy-crab-mitm/src/lua.rs`
- Test: `crates/proxy-crab-mitm/src/bypass.rs`
- Test: `crates/proxy-crab-mitm/tests/proxy_integration.rs`

- [x] **Step 1: Add failing Lua and bypass tests**

Test a routing script against an HTTP and CONNECT context:

```lua
if phase == "connect" and req.authority == "api.example.com:443" then
  return "checkout"
end
return nil
```

Assert only a valid tag string or nil is accepted; booleans, numbers, invalid tags, and runtime errors fail. Add SQLite tests for begin/complete/fail, pagination, deletion rejection for in-progress rows, batch deletion, and clear-terminal behavior.

- [x] **Step 2: Run focused tests and observe failure**

Run:

```bash
cargo test -p proxy-crab-mitm lua::tests bypass::tests
```

Expected: compilation failure because routing and bypass APIs are absent.

- [x] **Step 3: Implement the routing Lua API**

Reuse the existing Lua limits and codecs. Expose read-only:

```text
phase
req.method
req.version
req.authority
req.uri.raw/scheme/host/port/path/query
source.ip/port/address
```

Do not expose headers, body, SNI, or ALPN. Return `Option<String>` and validate tag syntax.

- [x] **Step 4: Implement `bypass.db`**

Open `<workspace>/bypass.db` with a schema containing ID, timestamps, source, method, URI/authority, version, reason, outcome, response status, error, and nullable byte counters. Provide thread-safe begin, complete, fail, list-page, delete-one, delete-many, and clear-terminal operations. In-progress rows cannot be deleted.

- [x] **Step 5: Refactor proxy routing**

At proxy start, do not create a Session. For plain HTTP, snapshot and execute the selected routing script per request; for CONNECT, execute once and either:

- bypass via a raw TCP tunnel without TLS interception; or
- pin the selected Session for the complete CONNECT lifetime, record CONNECT there, MITM TLS, and route every inner HTTP request to that fixed Session without rerunning Lua.

No selected rule resolves `default`; an unbound implicit default bypasses without creating. Explicit Lua tags use atomic resolve-or-create. Lua errors/invalid returns log and fall back to a bound default without creating one. Session creation failures log and bypass. Always serve `GET http://proxy.crab/ca.crt` locally before routing.

Stream bypass traffic without persisting headers or bodies. Record response status and byte counts when inexpensive; otherwise leave counts null.

- [x] **Step 6: Run proxy integration tests**

Run:

```bash
cargo test -p proxy-crab-mitm
```

Expected: all crate tests pass, including routing, fallback, auto-create, bypass, and CONNECT binding tests.

### Task 3: Manager, HTTP, and Tauri APIs

**Files:**
- Modify: `crates/proxy-crab-mgr/src/dto.rs`
- Modify: `crates/proxy-crab-mgr/src/manager.rs`
- Modify: `crates/proxy-crab-mgr/src/http.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `crates/proxy-crab-mgr/src/http.rs`
- Test: `crates/proxy-crab-mgr/src/manager.rs`

- [x] **Step 1: Add failing API tests**

Cover:

- Session create/update payloads with complete tag arrays;
- no activate endpoint and no `active_session_id`;
- deletion rejected while proxy is starting/running/stopping;
- routing script CRUD and nullable selection;
- content-only updates rejecting rename fields;
- bypass pagination, single/batch deletion, and clear-terminal;
- HTTP change resources for sessions, routing scripts/config, and bypass rows.

- [x] **Step 2: Run Manager tests and observe failure**

Run:

```bash
cargo test -p proxy-crab-mgr
```

Expected: compilation/test failure until the new DTOs and trait methods exist.

- [x] **Step 3: Implement DTO and trait changes**

Remove activation and rename DTO fields. Extend Session metadata/update requests with tags. Add routing library and selection payloads plus bypass row/page/delete DTOs. Extend `HttpApiResource` with routing and bypass resources.

- [x] **Step 4: Implement endpoints and Tauri commands**

Expose:

```text
GET/POST /api/routing-scripts
GET/PUT/DELETE /api/routing-scripts/{name}
GET/PUT /api/routing-script-selection
GET /api/bypass
DELETE /api/bypass/{id}
POST /api/bypass/delete
DELETE /api/bypass
```

Keep script update content-only. Remove `/api/sessions/{id}/activate`. Reuse full config replacement without active state. Mirror the same capabilities as Tauri commands.

- [x] **Step 5: Run Manager tests**

Run:

```bash
cargo test -p proxy-crab-mgr
```

Expected: all tests pass.

### Task 4: Vue routing, tags, and bypass UI

**Files:**
- Modify: `src/api/types.ts`
- Modify: `src/api/backend.ts`
- Modify: `src/api/tauri-backend.ts`
- Modify: `src/stores/sessions.ts`
- Modify: `src/stores/http-api-sync.ts`
- Modify: `src/stores/windows.ts`
- Modify: `src/windows/launcher.ts`
- Modify: `src/components/SessionSidebar.vue`
- Create: `src/stores/routing.ts`
- Create: `src/stores/bypass.ts`
- Create: `src/windows/RoutingManagerWindow.vue`
- Create: `src/windows/BypassWindow.vue`
- Modify: `src/App.vue`

- [x] **Step 1: Update frontend contracts**

Remove `active_session_id` and activation methods. Add Session `tags`, full tag update payloads, routing script selection, and bypass page/delete types consistently across backend adapters.

- [x] **Step 2: Implement stores and synchronization**

Add routing and bypass stores. Apply HTTP resource change notifications without overwriting unsaved editor content. Ensure external deletion of a selected rule refreshes selection and Session updates refresh tag chips.

- [x] **Step 3: Implement Session tag editing**

Remove active indicators and context actions. Render restrained tag chips, emphasizing `default`. Extend the existing inline Session editor with token-style tag entry and validation feedback. Preserve the user's pre-existing `LogTable.vue` changes untouched.

- [x] **Step 4: Implement the routing manager**

Follow existing Filter Manager conventions: script library on the left, Monaco editor on the right, nullable current selection at the top, create/save/delete actions, no rename and no debug surface.

- [x] **Step 5: Implement the bypass window**

Add a button beside Session `+`. Render a fixed-column, dense table with pagination, row selection, delete selected, delete one, and clear-terminal actions. Clearly distinguish in-progress, complete, and failed rows; disable deletion for in-progress rows. Do not add filters, custom columns, body details, cards, or decorative effects.

- [x] **Step 6: Build the frontend**

Run:

```bash
npm run build
```

Expected: Vue TypeScript check and Vite build succeed.

### Task 5: ProxyCrab skill and project documentation

**Files:**
- Modify: `skills/proxycrab/SKILL.md`
- Modify: `skills/proxycrab/references/http-api.md`
- Modify: `skills/proxycrab/references/lua-api.md`
- Modify: `skills/proxycrab/references/best-practices.md`
- Modify: `skills/proxycrab/evals/evals.json`
- Modify: `skills/proxycrab/scripts/session-list.mjs`
- Delete: `skills/proxycrab/scripts/session-activate.mjs`
- Create: `skills/proxycrab/scripts/routing-upsert.mjs`
- Create: `skills/proxycrab/scripts/routing-select.mjs`
- Create: `skills/proxycrab/scripts/bypass-list.mjs`
- Modify: `docs/backend-api.md`

- [x] **Step 1: Update user and agent documentation**

Replace active-Session language with default-tag routing. Document Lua globals/returns, implicit-default versus explicit-tag creation, bypass persistence and deletion, immutable script names, Session tag uniqueness/movement, and all HTTP endpoints.

- [x] **Step 2: Update bundled scripts and evals**

Remove activation workflow. Make Session list show tags. Add dependency-free scripts for routing CRUD/selection and bypass query. Extend eval cases to assert tag routing and absence of activation/rename guidance.

- [x] **Step 3: Validate scripts and docs**

Run:

```bash
node --check skills/proxycrab/scripts/*.mjs
jq empty skills/proxycrab/evals/evals.json
rg -n "active Session|active_session_id|activate|rename|重命名" skills/proxycrab docs/backend-api.md
```

Expected: syntax/JSON checks pass; remaining search hits appear only in explicit removal or migration notes, otherwise none.

### Task 6: Full verification and requirement audit

**Files:**
- Verify all modified files
- Preserve: `src/components/LogTable.vue` user-owned changes

- [x] **Step 1: Format and lint source**

Run:

```bash
cargo fmt --all -- --check
git diff --check
```

Expected: both exit successfully.

- [x] **Step 2: Run all Rust tests**

Run:

```bash
cargo test --workspace
```

Expected: every Rust unit and integration test passes.

- [x] **Step 3: Build the frontend**

Run:

```bash
npm run build
```

Expected: TypeScript and production build pass.

- [x] **Step 4: Audit the final diff**

Confirm every changed line maps to tag routing, bypass storage/UI, active/rename removal, tests, or documentation. Verify `git diff -- src/components/LogTable.vue` still contains the user's original copy-cell change and no unrelated edits.

- [x] **Step 5: Review the requirement matrix**

Verify explicitly:

- valid tag syntax and unique ownership;
- explicit missing tags auto-create exactly once;
- implicit/error default never auto-creates;
- nil bypasses;
- CONNECT executes once and binds;
- no-rule/no-default performs raw passthrough;
- CA download remains local;
- bypass persistence and deletion semantics;
- proxy-running Session deletion guard;
- immutable names and reference cleanup for every script kind;
- UI/API/Skill agreement.
