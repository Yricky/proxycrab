# Full Body Retrieval and Preview Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add bounded raw body retrieval for persisted logs and live breakpoints, rich detail-page previews, and URL/full-cURL copy actions.

**Architecture:** Keep the compatible log-detail payload, reduce embedded decoded text/JSON to 64 KiB, and attach the selected capture-file path plus stored-byte size. Raw HTTP endpoints default to streaming stored bytes with their captured `Content-Encoding` and enforce limits only against stored size; optional server decompression is deliberately unbounded and single-pass. The Tauri UI calls the loopback endpoint in non-decompressing mode, lets the browser handle `Content-Encoding`, creates Blob URLs for browser-native media, parses protobuf/gRPC locally, and uses the stored request-body path for reproducible cURL commands.

**Tech Stack:** Rust 2024, Axum, Tokio, async-compression, Vue 3, TypeScript, Monaco, browser Blob/media APIs.

---

### Task 1: Core body sources and compatible detail metadata

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/proxy-crab-mitm/Cargo.toml`
- Modify: `crates/proxy-crab-mitm/src/model.rs`
- Modify: `crates/proxy-crab-mitm/src/storage.rs`
- Modify: `crates/proxy-crab-mitm/src/runtime.rs`
- Test: `crates/proxy-crab-mitm/src/storage.rs`
- Test: `crates/proxy-crab-mitm/tests/proxy_integration.rs`

- [x] **Step 1: Add failing tests for the 64 KiB embed limit and paths**

Create captures whose text body is exactly 64 KiB and 64 KiB + 1, then assert the first returns `Text { content, size, path }` and the second returns `Large { size, path }`. Add compressed fixtures for gzip, br, deflate, zstd, and stacked encodings.

- [x] **Step 2: Run focused core tests and observe the old 16 MiB/pathless behavior**

Run: `cargo test -p proxy-crab-mitm storage::tests -- --nocapture`

Expected: new assertions fail because `BODY_DETAIL_LIMIT` is still 16 MiB and `BodyPayload` has no path.

- [x] **Step 3: Extend the core data model**

Use these compatible tagged variants:

```rust
pub enum BodyPayload {
    Empty,
    Text { content: String, size: u64, path: Option<String> },
    Json { content: serde_json::Value, size: u64, path: Option<String> },
    Binary { size: u64, path: Option<String> },
    Large { size: u64, path: Option<String> },
}

pub enum BodySourceData {
    File(PathBuf),
    Bytes(Vec<u8>),
}

pub struct BodySource {
    pub data: BodySourceData,
    pub path: Option<String>,
    pub stored_size: u64,
    pub content_type: Option<String>,
    pub content_encodings: Vec<String>,
}
```

Regular captures select `.modified` when present. Live breakpoint string replacements use `Bytes` with no path; file replacements expose their absolute file path and have no content encoding because replacements are sent as their literal bytes.

- [x] **Step 4: Implement bounded embedded decoding**

Set the detail embed limit to `64 * 1024`. Decode `gzip`, `br`, `deflate`, `zstd`, and stacked encodings in reverse order, stopping once decoded output exceeds the limit. Keep `size` equal to stored bytes and fall back to `Binary` metadata when embedded decoding is malformed; the raw endpoint owns strict decode errors.

- [x] **Step 5: Expose persisted and live breakpoint sources**

Add runtime methods that return a request/response `BodySource`, distinguish a missing response from an empty body file, and overlay the active breakpoint's current replacement only for the paused phase.

- [x] **Step 6: Run core tests**

Run: `cargo test -p proxy-crab-mitm storage::tests -- --nocapture`

Expected: all storage body tests pass.

### Task 2: Streaming raw HTTP body endpoints

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/proxy-crab-mgr/Cargo.toml`
- Modify: `crates/proxy-crab-mgr/src/dto.rs`
- Modify: `crates/proxy-crab-mgr/src/manager.rs`
- Modify: `crates/proxy-crab-mgr/src/http.rs`
- Test: `crates/proxy-crab-mgr/src/http.rs`

- [x] **Step 1: Add endpoint contract tests**

Cover:

```text
GET /api/logs/{id}/body?session_id=3&side=request&max_size=16777216
GET /api/breakpoints/{id}/body?side=response&max_size=16777216
```

Assert stored-byte success, captured media type and encoding, stored `Content-Length`/`X-ProxyCrab-Body-Size`, default 16 MiB, invalid side/max-size errors, optional decompression, `body_not_found`, `body_read_failed`, `body_decode_failed`, and structured 413 fields `actual_size`/`max_size`.

- [x] **Step 2: Run focused manager tests and observe missing routes**

Run: `cargo test -p proxy-crab-mgr http::tests -- --nocapture`

Expected: body route tests fail with 404.

- [x] **Step 3: Add body queries and structured errors**

```rust
pub struct BodyQuery {
    pub session_id: Option<u64>,
    pub side: String,
    #[serde(default)]
    pub decompress: bool,
    pub max_size: Option<u64>,
}

pub struct ManagerError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_size: Option<u64>,
}
```

Map `body_too_large` to 413 and `body_decode_failed` to 422. A zero `max_size`, or any `max_size` combined with `decompress=true`, is `bad_request`; there is deliberately no server-side maximum.

- [x] **Step 4: Implement direct streaming with optional single-pass decoding**

With `decompress=false`, compare `max_size` to file metadata, preserve `Content-Encoding`, and stream the stored file once. With `decompress=true`, reject `max_size`, apply encodings in reverse, and stream one decoding pass without pre-counting or decoded `Content-Length`.

- [x] **Step 5: Permit only validated local UI origins**

For accepted `http://localhost:*` and `tauri://localhost` origins, echo `Access-Control-Allow-Origin` and `Vary: Origin`; continue rejecting non-local origins. Do not emit wildcard CORS.

- [x] **Step 6: Run manager endpoint tests**

Run: `cargo test -p proxy-crab-mgr http::tests -- --nocapture`

Expected: all raw body contract and existing HTTP tests pass.

### Task 3: Frontend body client, cURL generation, and protobuf utilities

**Files:**
- Modify: `src/api/types.ts`
- Create: `src/api/body.ts`
- Create: `src/utils/curl.ts`
- Create: `src/utils/protobuf.ts`
- Modify: `src/utils/body-language.ts`

- [x] **Step 1: Mirror body metadata and errors in TypeScript**

Add stored `size` and optional `path` to every non-empty body variant. Model a loaded body with `bytes`, `decodedSize`, `contentType`, and an object URL that callers revoke.

- [x] **Step 2: Implement the loopback body client**

Resolve the configured management host/port, call the log or breakpoint endpoint with `decompress=false` and a 16 MiB stored-size default, parse the browser-decoded `ArrayBuffer`, and parse structured 413 errors. Never request a stored body larger than the UI's 1 GiB limit.

- [x] **Step 3: Implement cURL generation**

Generate POSIX-shell-safe multiline cURL with method, URL, every captured request header except case-insensitive `Content-Length`, and `--data-binary '@<path>'` only for a non-empty body with a path. Do not add HTTP-version flags.

- [x] **Step 4: Implement full protobuf and gRPC parsing**

Parse all protobuf wire types with `BigInt` for 64-bit values. Recursively interpret length-delimited values as UTF-8, nested protobuf, or bytes. For `application/grpc` and `application/grpc+proto`, parse every 5-byte frame; decode compressed frames using `grpc-encoding` when the browser supports it, otherwise show the complete raw frame bytes.

### Task 4: Detail-page previews and copy menu

**Files:**
- Modify: `src/windows/LogDetailWindow.vue`
- Reuse: `src/components/ContextMenu.vue`
- Reuse: `src/components/ConfirmDialog.vue`

- [x] **Step 1: Add per-side body loading state**

Automatically request each non-empty visible body with a 16 MiB stored-size limit. Store loaded bytes and loading/error state, and revoke old Blob URLs on refresh/unmount. Breakpoint mode uses the breakpoint endpoint and falls back to the persisted log endpoint after release.

- [x] **Step 2: Add the size-gated interaction**

For stored bodies over 16 MiB and at most 1 GiB, show stored size plus a confirmation-gated full-load button. Over 1 GiB, show stored size and copy-path only. Preserve retry and path copy after errors.

- [x] **Step 3: Render supported content**

Use Monaco for complete text/JSON/XML/HTML/JavaScript/form data, `<img>` for images, `<audio controls>` for audio, `<video controls>` for video, and a complete protobuf/gRPC tree. Unsupported binary has no hex preview and shows metadata/path actions only.

- [x] **Step 4: Replace the summary copy action with a dropdown**

The menu contains “复制 URL” and, outside breakpoint mode, “复制完整 cURL”. Breakpoint mode exposes URL only. Show existing toast/check feedback after copying.

- [x] **Step 5: Build the frontend**

Run: `pnpm build`

Expected: Vue type checking and Vite production build both succeed.

### Task 5: Skill surface, public docs, and full verification

**Files:**
- Create: `skills/proxycrab/scripts/body-get.mjs`
- Modify: `skills/proxycrab/SKILL.md`
- Modify: `skills/proxycrab/references/http-api.md`
- Modify: `skills/proxycrab/references/best-practices.md`
- Modify: `skills/proxycrab/evals/evals.json`
- Modify: `docs/backend-api.md`

- [x] **Step 1: Add a body retrieval script**

Support `--session-id`, `--log-id` or `--breakpoint-id`, `--side`, `--max-size`, and required `--output`. Request the default non-server-decompressed stream, let Node decode the preserved `Content-Encoding` for normal inspection, and surface structured over-limit errors without printing body bytes to stdout.

- [x] **Step 2: Update docs and evaluations**

Document the new routes, raw success exception to the JSON envelope, stored-size semantics, default non-decompression, optional single-pass decompression, 64 KiB embed behavior, error codes, breakpoint semantics, and safe output-file workflow. Replace the old Skill statement that binary/oversized body bytes are unavailable.

- [x] **Step 3: Format and run all Rust tests**

Run: `cargo fmt --all -- --check && cargo test --workspace`

Expected: formatting check exits zero and every workspace test passes.

- [x] **Step 4: Run the frontend build and inspect the diff**

Run: `pnpm build && git diff --check && git status --short`

Expected: build exits zero, diff check reports no whitespace errors, and status lists only files belonging to this feature.

- [x] **Step 5: Self-review requirement coverage**

Verify every confirmed requirement: 64 KiB compatible detail metadata with paths; raw log and breakpoint endpoints; stored-size limiting and 413 metadata; default encoded streaming plus optional gzip/br/deflate/zstd single-pass decoding; stored-size 16 MiB/1 GiB UI behavior; complete text/media/protobuf/gRPC rendering; unsupported-binary metadata only; copy URL/full cURL rules; no full cURL during breakpoints; Skill/docs/eval synchronization.
