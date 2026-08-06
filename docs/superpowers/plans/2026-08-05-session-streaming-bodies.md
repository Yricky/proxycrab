# Session Streaming Bodies Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stream captured request and response bodies without size limits, run each interceptor once when its phase headers arrive, preserve raw and replaced bodies concurrently, and support response-frame idle timeout through `_crab_resp_bodyframe_timeout`.

**Architecture:** Keep `proxy.rs` as the listener, connection-routing, and shared-helper entry; move transparent bypass handling to `proxy/bypass.rs` and captured MITM/Session handling to `proxy/mitm.rs`. Replace Session-wide body materialization with bounded-channel pumps that forward each Hyper frame while appending its data bytes to the capture file. Request and response interceptor chains run before their bodies are consumed; a replacement body becomes the outbound stream while the original inbound body continues draining to the raw capture file in a tracked background task. Response capture completion waits for both raw request and raw response drains, while replacement delivery is independent from raw-response timeout and storage warnings.

**Tech Stack:** Rust, Tokio async file I/O and channels, Hyper 1 bodies/frames, `http-body-util`, Lua 5.4 through `mlua`, SQLite capture metadata, Rust integration tests.

---

### Task 0: Split proxy entry, bypass, and MITM modules without behavior changes

**Files:**
- Modify: `crates/proxy-crab-mitm/src/proxy.rs`
- Create: `crates/proxy-crab-mitm/src/proxy/bypass.rs`
- Create: `crates/proxy-crab-mitm/src/proxy/mitm.rs`
- Test: `crates/proxy-crab-mitm/tests/proxy_integration.rs`

- [ ] **Step 1: Record the baseline proxy test result**

Run: `cargo test -p proxy-crab-mitm --test proxy_integration`

Expected: all existing integration tests pass before the move.

- [ ] **Step 2: Move transparent bypass handling**

Move `handle_bypass_http`, bypass CONNECT/tunnel forwarding, and bypass-specific request/response assembly into `proxy/bypass.rs`. Keep `resolve_route`, listener/service entry points, shared HTTP conversion helpers, `TaskGroup`, and `ProxyController` in `proxy.rs`. Expose only `pub(super)` functions required by the entry module.

- [ ] **Step 3: Move captured MITM/Session handling**

Move `handle_session_http_request`, interceptor snapshot/execution helpers, capture failure/note helpers, captured CONNECT handling, and Session request/response assembly into `proxy/mitm.rs`. Keep TLS accept/connection dispatch in `proxy.rs`, which calls the MITM module after routing selects a Session.

- [ ] **Step 4: Format and verify the behavior-preserving split**

Run: `cargo fmt --all`

Run: `cargo test -p proxy-crab-mitm --test proxy_integration`

Expected: the same integration tests pass after the move with no behavioral assertion changes.

### Task 1: Add append-only capture body writers

**Files:**
- Modify: `crates/proxy-crab-mitm/src/storage.rs`
- Test: `crates/proxy-crab-mitm/src/storage.rs`

- [ ] **Step 1: Write failing storage tests**

Add Tokio tests that create a capture, open a raw request writer and a modified response writer, append two chunks to each, finish them, and assert `body_source` reads the concatenated bytes and still prefers the modified file. Include a payload larger than the former 64 MiB boundary by creating and appending repeated chunks; assert its stored size is exact.

```rust
#[tokio::test]
async fn body_writer_appends_without_a_size_limit() {
    let (_directory, store, id) = store_with_capture();
    let mut writer = store
        .create_body_writer(id, BodySide::Response, false)
        .await
        .unwrap();
    let chunk = vec![b'x'; 1024 * 1024];
    for _ in 0..65 {
        writer.write_all(&chunk).await.unwrap();
    }
    writer.finish().await.unwrap();

    let source = store.body_source(id, BodySide::Response).unwrap().unwrap();
    assert_eq!(source.stored_size, 65 * 1024 * 1024);
}
```

- [ ] **Step 2: Run the focused storage tests and verify they fail**

Run: `cargo test -p proxy-crab-mitm storage::tests::body_writer -- --nocapture`

Expected: compilation fails because `create_body_writer` does not exist.

- [ ] **Step 3: Implement `CaptureBodyWriter`**

Add a focused writer type backed by `tokio::io::BufWriter<tokio::fs::File>`. `CaptureStore::create_body_writer` must create/truncate the existing deterministic body path without holding the SQLite mutex. `write_all` appends bytes; `finish` flushes and updates `updated_at` once. Do not add a byte counter or maximum.

```rust
pub struct CaptureBodyWriter {
    store: CaptureStore,
    id: u64,
    writer: tokio::io::BufWriter<tokio::fs::File>,
}

impl CaptureBodyWriter {
    pub async fn write_all(&mut self, bytes: &[u8]) -> Result<()> {
        tokio::io::AsyncWriteExt::write_all(&mut self.writer, bytes).await?;
        Ok(())
    }

    pub async fn finish(mut self) -> Result<()> {
        tokio::io::AsyncWriteExt::flush(&mut self.writer).await?;
        self.store.touch(self.id)
    }
}
```

Keep `save_body` for small synthetic/string bodies and existing call sites. Expose no arbitrary filesystem path outside the crate.

- [ ] **Step 4: Run storage tests**

Run: `cargo test -p proxy-crab-mitm storage::tests -- --nocapture`

Expected: all storage tests pass, including the 65 MiB append test.

### Task 2: Build reusable streaming, pacing, recording, and timeout bodies

**Files:**
- Modify: `crates/proxy-crab-mitm/src/proxy/body.rs`
- Modify: `crates/proxy-crab-mitm/src/proxy.rs`
- Test: `crates/proxy-crab-mitm/src/proxy.rs`

- [ ] **Step 1: Add failing unit tests for the response-frame timeout and file replacement stream**

Test that a body whose next frame is delayed beyond a configured duration produces a `response body frame timed out` error, while a frame arriving before the deadline resets the deadline for the following frame. Test that a replacement file larger than 64 MiB exposes the exact size hint without reading the file into a `Vec<u8>`.

- [ ] **Step 2: Run the focused body tests and verify they fail**

Run: `cargo test -p proxy-crab-mitm proxy::tests::streaming -- --nocapture`

Expected: compilation fails because the new streaming body helpers are absent.

- [ ] **Step 3: Add channel-backed body and tracked pump helpers**

Add a small `ChannelBody` implementing `hyper::body::Body<Data = Bytes, Error = BoxError>` over a bounded Tokio receiver. Add a pump factory that owns an inbound body, an optional `CaptureBodyWriter`, an optional frame idle timeout, and a completion callback.

```rust
pub(super) enum PumpOutcome {
    Complete,
    InputError(String),
    FrameTimeout,
    OutputClosed,
}

pub(super) fn pump_body<B>(
    body: B,
    writer: Option<CaptureBodyWriter>,
    frame_timeout: Option<Duration>,
    forward: bool,
    tracker: &TaskGroup,
    complete: impl FnOnce(PumpOutcome) + Send + 'static,
) -> Option<ProxyBody>
where
    B: Body<Data = Bytes> + Send + 'static,
    B::Error: StdError + Send + Sync + 'static,
```

Use a bounded channel with capacity at most 8. For every frame, append `data_ref()` bytes before forwarding the unchanged frame, including forwarding trailer frames. Wrap each `body.frame()` wait—not the whole response—in `tokio::time::timeout`. When capture-file creation or writing fails, record the error through the callback, disable further recording, and continue forwarding frames.

- [ ] **Step 4: Generalize pacing to streamed bodies**

Retain `PacedBody` for byte/string bodies, and add a streamed pacing wrapper that splits incoming data frames to the existing 50-chunks-per-second target without initial or catch-up bursts. It must pass trailers unchanged and preserve bounded memory. `_crab_req_speed` and `_crab_resp_speed` keep their existing final-outbound-byte semantics.

- [ ] **Step 5: Add a file-backed replacement body**

Open `BodyReplacement::File` once at interceptor-application time, retain its metadata length, convert the handle to `tokio::fs::File`, and expose it through `tokio_util::io::ReaderStream`. Tee its emitted bytes to the modified capture writer. `BodyReplacement::String` remains a `Bytes` body and is saved directly. Remove `MAX_BODY_REPLACEMENT_BYTES` and stop using `read_to_end` on files in the proxy path.

- [ ] **Step 6: Run the body tests**

Run: `cargo test -p proxy-crab-mitm proxy::tests -- --nocapture`

Expected: timeout reset, trailers, pacing, and large-file source tests pass.

### Task 3: Move request interceptors to headers and stream the original request body

**Files:**
- Modify: `crates/proxy-crab-mitm/src/proxy.rs`
- Modify: `crates/proxy-crab-mitm/src/runtime.rs`
- Modify: `crates/proxy-crab-mitm/src/lua.rs`
- Test: `crates/proxy-crab-mitm/tests/proxy_integration.rs`

- [ ] **Step 1: Write failing request-phase integration tests**

Add a staged client upload test that sends request headers, delays its original body, and attaches a request interceptor that changes a header and replaces the body. Assert the upstream receives the modified headers and replacement body before the client releases the original body. After release, assert the raw capture contains the original body and the modified capture contains the replacement. Assert exactly one saved interceptor execution exists.

Add a no-replacement companion asserting the first client body chunk reaches upstream before the upload finishes and the final raw capture is byte-exact.

- [ ] **Step 2: Run the request streaming tests and verify they fail**

Run: `cargo test -p proxy-crab-mitm --test proxy_integration request_interceptors_run_at_headers -- --nocapture`

Expected: timeout/failure because the current implementation waits for the complete request body before running interceptors.

- [ ] **Step 3: Extract header-only request interceptor execution**

Move the existing request interceptor loop to immediately after `CaptureStore::begin`. Preserve exact chain order, breakpoint behavior, mutation journals, errors-before-effects behavior, tags, and one historical run per saved interceptor. Track the last successfully opened replacement source; an unreadable later file replacement records the same interceptor error and leaves the previous valid replacement selected.

- [ ] **Step 4: Start raw request drain and choose the outbound request body**

Create the raw request writer before consuming `Incoming`. Start one tracked pump that always attempts to drain the original request body into the raw file. Without replacement, the same bounded pump forwards frames upstream. With replacement or `_crab_skip`, it only drains raw data in the background while the replacement body is sent upstream immediately or upstream is skipped. Return a one-shot raw-request completion receiver for final capture coordination.

Build the upstream request from mutated method/URI/headers plus the selected streaming body. Preserve original framing when unmodified; remove transfer framing and set the exact content length for string/file replacements. Remove `BODY_READ_TIMEOUT`, `MAX_CAPTURE_BODY_BYTES`, `Limited`, and `LengthLimitError` from the request path.

- [ ] **Step 5: Keep breakpoint previews bounded**

Remove the 64 MiB replacement rejection from `lua.rs`, but do not load arbitrary replacement files in `runtime.breakpoint`. For a replacement file larger than the existing 64 KiB detail limit, return `BodyPayload::Large` with its path and size; read only small files for inline preview. The Lua body API remains write-only.

- [ ] **Step 6: Run request, interceptor, and breakpoint tests**

Run: `cargo test -p proxy-crab-mitm --test proxy_integration -- --nocapture`

Run: `cargo test -p proxy-crab-mitm lua::tests runtime::tests -- --nocapture`

Expected: all request streaming, replacement, pacing, timeout, and breakpoint tests pass.

### Task 4: Move response interceptors to headers and stream raw responses

**Files:**
- Modify: `crates/proxy-crab-mitm/src/proxy.rs`
- Modify: `crates/proxy-crab-mitm/src/proxy/body.rs`
- Test: `crates/proxy-crab-mitm/tests/proxy_integration.rs`

- [ ] **Step 1: Write failing response streaming tests**

Use the existing staged upstream to assert an active Session receives `hello` before the upstream releases `world`, then assert the raw capture is `helloworld` and exactly one response interceptor execution was recorded.

Add a replacement test whose interceptor runs at response headers. Assert the client receives the replacement before the staged raw upstream is released; after release, assert raw and modified body files match the upstream and replacement respectively.

- [ ] **Step 2: Write failing `_crab_resp_bodyframe_timeout` tests**

Add a response interceptor that sets `_crab_resp_bodyframe_timeout` to a short positive millisecond value. Test timeout from headers to first frame and timeout between frames. For a normal response, assert the downstream stream terminates and the capture is failed with `response_body_timeout`. For a replaced response, assert the replacement succeeds, the raw partial body is retained, the capture outcome becomes success, and its warning/error metadata records `raw_response_body_timeout`.

Also test invalid values (`0`, signs, whitespace, decimal notation, overflow) are ignored with one warning, matching existing special-tag parsing.

- [ ] **Step 3: Run the response tests and verify they fail**

Run: `cargo test -p proxy-crab-mitm --test proxy_integration response_stream -- --nocapture`

Expected: the staged Session response remains buffered or the new tag is absent.

- [ ] **Step 4: Extract header-only response interceptor execution**

Immediately after upstream response headers arrive, persist response metadata and run the snapshotted response chain once. Hold downstream headers until the chain and any breakpoint complete. Apply status/header/tag changes before constructing the downstream response. Resolve `_crab_resp_speed` and `_crab_resp_bodyframe_timeout` only after the full response chain.

Define `_crab_resp_bodyframe_timeout` as positive ASCII decimal milliseconds. It bounds the wait from final response headers to the first upstream body frame and every adjacent upstream body frame, resetting after each frame. It does not apply to request bodies, bypass, CONNECT, Upgrade/WebSocket, the local CA response, or proxy-generated responses.

- [ ] **Step 5: Stream or replace without blocking on the raw response**

Without replacement, return downstream headers immediately with a body backed by the raw-response pump. On EOF, wait for raw request drain, flush both files, and mark the capture complete. On upstream body error or frame timeout, terminate the downstream body and mark the capture failed.

With replacement, return the replacement stream immediately while a separate tracked task drains the original upstream body to the raw response file. Its timeout, upstream error, or storage error calls `note_error` with a raw-response-specific kind, retains partial raw bytes, waits for raw request drain, and then marks the capture successful. Do not cancel replacement delivery or convert it to a proxy error.

Rebuild headers without adding `Content-Length` to unknown-length streams. Preserve valid original length when the body is unchanged; use exact replacement length for strings/files. Remove stale `content-encoding` for replacements and forward trailers for unchanged streams.

- [ ] **Step 6: Remove obsolete capture materialization semaphore**

Delete `capture_slots`, `acquire_capture_slot`, and their Tokio semaphore imports from `runtime.rs`. Streaming channels and the existing 256-connection bound provide bounded memory; no exchange-wide body buffer remains.

- [ ] **Step 7: Run all proxy integration tests**

Run: `cargo test -p proxy-crab-mitm --test proxy_integration -- --nocapture`

Expected: all Session streaming, bypass, interceptor, breakpoint, pacing, timeout, upgrade, and TLS tests pass.

### Task 5: Update user-facing contracts and ProxyCrab Skill coverage

**Files:**
- Modify: `README.md`
- Modify: `docs/backend-api.md`
- Modify: `docs/lua-api.md`
- Modify: `skills/proxycrab/SKILL.md`
- Modify: `skills/proxycrab/references/lua-api.md`
- Modify: `skills/proxycrab/references/http-api.md`
- Modify: `skills/proxycrab/evals/evals.json`
- Modify: `skills/proxycrab/evals/fixtures/test-traffic-limits.lua`

- [ ] **Step 1: Update project documentation**

Replace every statement that captured/replacement bodies are limited to 64 MiB or use a 60-second whole-body timeout. Document streaming append semantics, 64 KiB inline preview only, header-time one-shot interceptor execution, concurrent raw/replacement capture, and the response-frame timeout behavior.

- [ ] **Step 2: Update Lua and Skill documentation**

Add `_crab_resp_bodyframe_timeout` beside existing special tags with exact units, validation, reset behavior, exclusions, and replacement/raw warning behavior. State explicitly that body replacement has no ProxyCrab byte limit, file replacements stream from disk, and Lua still cannot read original bodies.

Update `skills/proxycrab/SKILL.md` safety notes so agents know captures and replacement files can be unbounded on disk and should use deliberate output limits when exporting or inspecting them.

- [ ] **Step 3: Update Skill evaluations**

Extend `test-traffic-limits.lua` to set `_crab_resp_bodyframe_timeout`, and update its expected output to require preservation of the interceptor chains and verification from a new capture. Add an evaluation case that rejects claims that the tag is an HTTP header or a total response deadline.

- [ ] **Step 4: Validate documentation and fixtures**

Run: `rg -n "64 MiB|60-second read timeout|MAX_CAPTURE_BODY_BYTES|BODY_READ_TIMEOUT" README.md docs skills crates/proxy-crab-mitm`

Expected: only intentional historical plan text or unrelated codec limits remain; current contracts contain no stale capture/replacement limit.

Run: `jq empty skills/proxycrab/evals/evals.json`

Expected: exit 0.

### Task 6: Full verification

**Files:**
- Verify only

- [ ] **Step 1: Format and inspect the diff**

Run: `cargo fmt --all -- --check`

Run: `git diff --check`

Expected: both exit 0.

- [ ] **Step 2: Run the complete Rust workspace test suite**

Run: `cargo test --workspace --all-targets`

Expected: all targets pass with zero failures.

- [ ] **Step 3: Run the frontend build**

Run: `pnpm build`

Expected: `vue-tsc --noEmit` and `vite build` both exit 0. No DTO shape change is expected, so any failure must be investigated rather than bypassed.

- [ ] **Step 4: Recheck requirement coverage**

Confirm with focused tests that: bodies above 64 MiB forward and persist; request and response interceptors run once at headers; raw and replacement bodies proceed concurrently; replacement files stream; `_crab_resp_bodyframe_timeout` is an idle per-frame timeout; normal stream timeout fails the transfer; replacement raw timeout is only a capture warning; and inline previews remain capped at 64 KiB.
