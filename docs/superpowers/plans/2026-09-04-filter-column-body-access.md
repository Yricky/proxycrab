# Filter and Custom-Column Body Access Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow filter and custom-column Lua scripts to read persisted request and response bodies through read-only `as_string()` and `as_json()` methods.

**Architecture:** Extend the reusable historical-capture Lua environment with a lazy, read-only body resolver. The manager supplies a capture-scoped resolver backed by `ProxyCrab::capture_body_source`; each side is decoded at most once per capture and reused across custom columns. In-progress captures expose unavailable bodies so filtering never depends on a partial file.

**Tech Stack:** Rust, `mlua` 0.11, SQLite-backed capture summaries, filesystem body blobs, Cargo tests.

---

### Task 1: Historical body Lua model

**Files:**
- Modify: `crates/proxy-crab-mitm/src/lua.rs`
- Test: `crates/proxy-crab-mitm/src/lua.rs`

- [x] **Step 1: Write failing unit tests**

Add tests that construct a lazy body resolver and assert:

```rust
let bodies = CaptureBodyAccess::new(move |side| {
    reads.fetch_add(1, Ordering::Relaxed);
    Ok(Some(match side {
        BodySide::Request => BodySource {
            data: BodySourceData::Bytes(br#"{"order_id":"needle"}"#.to_vec()),
            path: None,
            stored_size: 21,
            content_type: Some("application/json".into()),
            content_encodings: Vec::new(),
        },
        BodySide::Response => BodySource {
            data: BodySourceData::Bytes(b"response".to_vec()),
            path: None,
            stored_size: 8,
            content_type: Some("text/plain".into()),
            content_encodings: Vec::new(),
        },
    }))
});
assert!(filter.evaluate_with_bodies("needle", &entry, bodies.clone()).unwrap());
assert_eq!(column.evaluate_with_bodies(&entry, bodies).unwrap(), "response");
assert_eq!(reads.load(Ordering::Relaxed), 2);
```

Also assert that unavailable bodies return `nil`, non-text bodies return `nil`, malformed JSON returns `nil`, compressed text is decoded, and decoded bodies over 16 MiB fail.

- [x] **Step 2: Run the focused test and confirm it fails**

Run:

```bash
cargo test -p proxy-crab-mitm lua::tests::historical_body
```

Expected: compilation failure because `CaptureBodyAccess` and `evaluate_with_bodies` do not exist.

- [x] **Step 3: Implement the read-only body object**

Add a capture-scoped resolver and side caches:

```rust
#[derive(Clone)]
pub struct CaptureBodyAccess(Arc<CaptureBodyAccessInner>);

struct CaptureBodyAccessInner {
    resolver: Option<Arc<BodySourceResolver>>,
    request: OnceLock<Result<Option<Arc<[u8]>>, String>>,
    response: OnceLock<Result<Option<Arc<[u8]>>, String>>,
}
```

Expose `entry.req.body` and `entry.resp.body` as read-only userdata with `as_string()` and `as_json()`. Factor the existing body-source decoding rules so historical and interceptor getters share textual `Content-Type`, gzip/br/deflate/zstd, UTF-8, malformed JSON, empty-body, and 16 MiB behavior. Do not expose replacement methods.

- [x] **Step 4: Pass body access into reusable evaluations**

Keep existing no-body convenience methods for unit callers and add:

```rust
pub fn evaluate_with_bodies(
    &self,
    argument: &str,
    entry: &CaptureSummary,
    bodies: CaptureBodyAccess,
) -> Result<bool>;

pub fn evaluate_with_bodies(
    &self,
    entry: &CaptureSummary,
    bodies: CaptureBodyAccess,
) -> Result<String>;
```

`ReusableScript::prepare` must construct `EntryView` with the supplied capture-scoped access object.

- [x] **Step 5: Run the Lua tests**

Run:

```bash
cargo test -p proxy-crab-mitm lua::tests
```

Expected: all Lua unit tests pass.

### Task 2: Manager integration and lazy loading

**Files:**
- Modify: `crates/proxy-crab-mgr/src/manager.rs`
- Test: `crates/proxy-crab-mgr/src/manager.rs`

- [x] **Step 1: Write failing manager tests**

Persist textual request and response bodies, finish their captures, install body-aware filter and column scripts, then verify `log_ids`, `log_views`, and `debug_filter_script`. Include an in-progress capture whose script observes both bodies as `nil`.

```lua
local req = entry.req.body:as_json()
return req ~= nil and req.order_id == ...
```

```lua
return entry.resp.body:as_string()
```

- [x] **Step 2: Run the focused manager tests and confirm they fail**

Run:

```bash
cargo test -p proxy-crab-mgr historical_bodies
```

Expected: scripts fail because historical request/response objects do not expose `body`.

- [x] **Step 3: Supply capture-scoped resolvers**

Add a helper equivalent to:

```rust
fn capture_body_access(runtime: &Arc<ProxyCrab>, item: &CaptureSummary) -> CaptureBodyAccess {
    let runtime = runtime.clone();
    let session_id = item.session_id;
    let capture_id = item.id;
    CaptureBodyAccess::new(move |side| {
        runtime.capture_body_source(session_id, capture_id, side)
    })
}
```

Use the body-aware evaluator for filter scripts, custom-column filters, visible custom columns, and filter debugging. Construct one access object per capture row and clone it across columns so request/response decoding is cached.
`ReusableScript::prepare` replaces that access with `CaptureBodyAccess::unavailable()` when the
capture outcome is `in_progress`, before any resolver can run.

- [x] **Step 4: Run manager tests**

Run:

```bash
cargo test -p proxy-crab-mgr manager::tests
```

Expected: all manager tests pass, including normal filter and view behavior.

### Task 3: Public and bundled Skill documentation

**Files:**
- Modify: `docs/lua-api.md`
- Modify: `skills/proxycrab/references/lua-api.md`
- Modify: `skills/proxycrab/SKILL.md`
- Modify: `skills/proxycrab/evals/evals.json`

- [x] **Step 1: Document the read-only API**

Document:

```lua
local request_text = entry.req.body:as_string()
local response_json = entry.resp and entry.resp.body:as_json()
```

State that historical getters are lazy and cached per capture evaluation, use the effective persisted body, return `nil` for in-progress captures, follow textual/UTF-8/content-encoding rules, and retain the 16 MiB decoded limit. Clarify that replacement methods remain interceptor-only.

- [x] **Step 2: Add a Skill evaluation case**

Add a prompt/expected-output pair requiring an agent to create a body-aware filter or column, handle `nil`, and avoid reading body unless metadata predicates match first.

- [x] **Step 3: Validate documentation consistency**

Run:

```bash
rg -n 'entry\.(req|resp)\.body|in-progress|16 MiB' docs/lua-api.md skills/proxycrab
```

Expected: both Lua references and the bundled Skill describe the same API and limits.

### Task 4: Full verification

**Files:**
- Verify all modified files.

- [x] **Step 1: Format Rust code**

Run:

```bash
rustfmt --edition 2024 --check crates/proxy-crab-mitm/src/lua.rs crates/proxy-crab-mgr/src/manager.rs
```

Expected: exit code 0. The repository-wide check currently also reports pre-existing formatting in
`crates/proxy-crab-mitm/src/model.rs`, which is outside this change.

- [x] **Step 2: Run affected crate tests**

Run:

```bash
cargo test -p proxy-crab-mitm -p proxy-crab-mgr
```

Expected: all tests pass.

- [x] **Step 3: Inspect the final diff**

Run:

```bash
git diff --check
git status --short
git diff --stat
```

Expected: no whitespace errors and only implementation, test, plan, Lua documentation, and bundled Skill files are changed.
