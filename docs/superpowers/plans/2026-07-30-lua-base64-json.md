# Lua Base64 and JSON API Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add bounded, sandbox-safe Base64 and JSON serialization/deserialization modules to every ProxyCrab Lua script type, including aggregated system-log warnings for lossy JSON numbers and duplicate object keys.

**Architecture:** Keep Lua execution and ProxyCrab userdata in `lua.rs`, and place codec installation and conversion logic in a focused `lua/codec.rs` module. Each fresh sandbox installs read-only `base64` and `json` proxy tables and owns an execution-local warning accumulator; the existing filter, column, and interceptor call paths provide script identity so warnings can be emitted once after each execution.

**Tech Stack:** Rust 2024, mlua 0.11/Lua 5.4, base64 0.22, serde/serde_json 1, tracing, existing ProxyCrab log buffer.

---

### Task 1: Add failing Lua codec tests

**Files:**
- Modify: `crates/proxy-crab-mitm/src/lua.rs`
- Create: `crates/proxy-crab-mitm/src/lua/codec.rs`

- [ ] **Step 1: Add Base64 contract tests**

Cover standard padded encoding/strict decoding, URL-safe padded and unpadded forms, arbitrary bytes including `\0`, read-only module fields, required boolean padding argument, malformed alphabets/padding, and 16 MiB input/output limits. Exercise the functions through the same `safe_lua` sandbox used by real scripts:

```rust
assert_eq!(
    evaluate_column("return base64.encode('hello')", &entry()).unwrap(),
    "aGVsbG8="
);
assert_eq!(
    evaluate_column(
        "return base64.url_decode(base64.url_encode(string.char(0, 255), false))",
        &entry(),
    )
    .unwrap()
    .as_bytes(),
    &[0, 255],
);
assert!(
    evaluate_column("return base64.decode('aGV sbG8=')", &entry()).is_err()
);
```

- [ ] **Step 2: Add JSON round-trip and mapping tests**

Cover every scalar, `nil`/`json.null`, stable object ordering, automatic arrays/objects, forced empty containers, decode type preservation, Unicode escapes, trailing input rejection, invalid UTF-8, unsupported values, sparse/mixed tables, cycles, shared tables, existing metatables, non-finite numbers, 128-level nesting, and 16 MiB input/output bounds:

```rust
assert_eq!(
    evaluate_column(
        "return json.encode({z = 1, a = json.null, list = json.array({})})",
        &entry(),
    )
    .unwrap(),
    r#"{"a":null,"list":[],"z":1}"#,
);
assert_eq!(
    evaluate_column(
        "local v = json.decode('{\"items\":[]}'); return json.encode(v)",
        &entry(),
    )
    .unwrap(),
    r#"{"items":[]}"#,
);
```

- [ ] **Step 3: Run the focused tests and confirm failure**

Run:

```bash
cargo test -p proxy-crab-mitm lua::tests -- --nocapture
```

Expected: the new tests fail because `base64`, `json`, `json.null`, and the warning integration do not exist.

### Task 2: Implement bounded read-only Base64 globals

**Files:**
- Create: `crates/proxy-crab-mitm/src/lua/codec.rs`
- Modify: `crates/proxy-crab-mitm/src/lua.rs`

- [ ] **Step 1: Install a read-only module proxy**

Create an empty public table whose metatable forwards `__index` to a private backing table, rejects every `__newindex`, and hides its metatable. Install the proxy as the global `base64`.

- [ ] **Step 2: Add the four Base64 functions**

Implement these exact signatures:

```lua
base64.encode(data)
base64.decode(text)
base64.url_encode(data, with_padding)
base64.url_decode(text)
```

Use `STANDARD`, `URL_SAFE`, and `URL_SAFE_NO_PAD`; reject non-string arguments through mlua conversion, enforce 16 MiB input and output limits, keep standard decoding strict, let URL decoding accept either canonical padded or canonical unpadded input, and return arbitrary Lua byte strings.

- [ ] **Step 3: Run Base64 tests**

Run:

```bash
cargo test -p proxy-crab-mitm lua::tests::base64 -- --nocapture
```

Expected: all Base64-specific tests pass.

### Task 3: Implement JSON conversion and execution-local warnings

**Files:**
- Create: `crates/proxy-crab-mitm/src/lua/codec.rs`
- Modify: `crates/proxy-crab-mitm/src/lua.rs`

- [ ] **Step 1: Define the JSON value and warning model**

Use a private value tree with sorted objects:

```rust
#[derive(serde::Serialize)]
#[serde(untagged)]
enum JsonValue {
    Null,
    Boolean(bool),
    Integer(i64),
    Number(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(std::collections::BTreeMap<String, JsonValue>),
}

#[derive(Debug, Default, Clone, Copy)]
pub(super) struct JsonWarningCounts {
    pub lossy_numbers: usize,
    pub duplicate_keys: usize,
}
```

Store counts in an `Arc<Mutex<_>>` owned by one fresh Lua sandbox.

- [ ] **Step 2: Implement strict seeded JSON decoding**

Use a custom serde visitor/seed so object insertion can count duplicate keys before the last value replaces the first, nested containers can enforce the depth limit, signed integers remain Lua integers, out-of-range unsigned/integral values become finite Lua numbers and increment `lossy_numbers`, and non-finite/out-of-range values fail. Call `Deserializer::end()` to reject trailing non-whitespace input.

- [ ] **Step 3: Preserve decoded array/object identity**

Create protected metatables for decoded and explicitly marked containers. `json.array(table)` and `json.object(table)` return the same table, reject an existing metatable, and mark the table so empty containers round-trip. Ordinary unmarked tables auto-detect consecutive integer keys `1..n` as arrays and string-only keys as objects; empty unmarked tables become objects.

- [ ] **Step 4: Implement strict JSON encoding**

Walk Lua values recursively with a path-local `HashSet` of table pointers, allowing shared tables but rejecting cycles. Reject sparse/mixed/unsupported tables, invalid UTF-8, non-finite numbers, and nesting beyond 128. Serialize sorted objects through a size-limited writer so output cannot exceed 16 MiB.

- [ ] **Step 5: Install the read-only `json` module**

Expose only:

```lua
json.encode(value)
json.decode(text)
json.array(table)
json.object(table)
json.null
```

`json.null` is a stable immutable userdata identity. Module members are served through the same read-only proxy pattern as Base64.

- [ ] **Step 6: Run JSON tests**

Run:

```bash
cargo test -p proxy-crab-mitm lua::tests::json -- --nocapture
```

Expected: JSON behavior and bound tests pass.

### Task 4: Emit contextual warnings from every script type

**Files:**
- Modify: `crates/proxy-crab-mitm/src/lua.rs`
- Modify: `crates/proxy-crab-mgr/src/manager.rs`
- Modify: `crates/proxy-crab-mitm/src/proxy.rs`

- [ ] **Step 1: Add named execution entry points**

Preserve the existing public helpers and add named variants used by production call sites:

```rust
pub fn evaluate_filter_named(
    source: &str,
    argument: &str,
    entry: &CaptureSummary,
    script_name: &str,
) -> Result<bool>;

pub fn evaluate_column_named(
    source: &str,
    entry: &CaptureSummary,
    script_name: &str,
) -> Result<String>;
```

Do the equivalent for lenient request and response interceptor execution with script name and capture log ID.

- [ ] **Step 2: Aggregate and emit after script execution**

After every success or runtime failure, take the execution-local counts and emit at most one `tracing::warn!` event:

```text
Lua JSON decode warning in column script "payload": capture_log_id=123, lossy_numbers=1, duplicate_keys=2
```

Do not include JSON content, paths, or original number values.

- [ ] **Step 3: Retain script names in prepared manager filters**

Extend `PreparedFilter` so loaded filter and column scripts retain both name and source, then call the named helpers for normal list filtering, log-view rendering, and filter debugging.

- [ ] **Step 4: Pass interceptor snapshot identity**

Call the named lenient helpers with the historical snapshot name and current capture ID for both request and response chains.

- [ ] **Step 5: Verify system-log capture**

Use `tracing_subscriber::registry().with(BufferLayer::new(buffer.clone()))` in a focused test, execute JSON containing both a duplicate key and a lossy integer, and assert one `WARN` entry with script type/name, capture ID, and both counts but no input value.

### Task 5: Update ProxyCrab documentation and Skill evaluation

**Files:**
- Modify: `docs/lua-api.md`
- Modify: `skills/proxycrab/references/lua-api.md`
- Modify: `skills/proxycrab/evals/evals.json`

- [ ] **Step 1: Document exact Base64 and JSON APIs**

Document signatures, examples, strict padding/whitespace behavior, binary Lua strings, JSON mapping, `json.null`, container markers, stable ordering, duplicate/lossy warnings, errors, and 16 MiB/128-depth limits. Reiterate that codecs do not add Body read access.

- [ ] **Step 2: Add a Skill evaluation case**

Add an evaluation prompt that requires writing a Lua interceptor using `json.encode` and URL-safe Base64, while preserving existing interceptor chains and verifying historical execution on a new capture. The expected output must require valid API usage and capture evidence rather than save-only success.

- [ ] **Step 3: Validate evaluation JSON**

Run:

```bash
jq empty skills/proxycrab/evals/evals.json
```

Expected: exit code 0.

### Task 6: Final verification

**Files:**
- Verify all files changed above

- [ ] **Step 1: Format Rust**

Run:

```bash
cargo fmt --all -- --check
```

Expected: exit code 0. If it reports formatting differences, run `cargo fmt --all` and repeat the check.

- [ ] **Step 2: Run focused crate tests**

Run:

```bash
cargo test -p proxy-crab-mitm
cargo test -p proxy-crab-mgr
```

Expected: all tests pass.

- [ ] **Step 3: Run workspace tests**

Run:

```bash
cargo test --workspace
```

Expected: all workspace tests pass.

- [ ] **Step 4: Review scope and diff**

Run:

```bash
git diff --check
git diff --stat
git status --short
```

Expected: no whitespace errors; only Lua codec/runtime call-path documentation and Skill evaluation files are changed by this work, while the pre-existing `src/components/InterceptorPipeline.vue` modification remains untouched.
