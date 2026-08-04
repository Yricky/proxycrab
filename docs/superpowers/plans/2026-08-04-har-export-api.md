# HAR Export API Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a management HTTP endpoint and bundled Skill script that export successful ProxyCrab HTTP captures as a HAR 1.2 file.

**Architecture:** Keep capture selection and filesystem access in `MitmManager`, isolate HAR serialization in a focused `har` module, and let the Axum handler return the completed UTF-8 JSON document with download headers. The endpoint snapshots IDs before reading bodies, exports only successful responses (excluding the synthetic TLS CONNECT record), and preserves ProxyCrab-only metadata under `_proxyCrab`.

**Tech Stack:** Rust 2024, Axum 0.8, Serde JSON, Tokio async readers/decoders, `url`, `time`, Node.js Skill scripts.

---

### Task 1: Define and verify the HAR serializer

**Files:**
- Create: `crates/proxy-crab-mgr/src/har.rs`
- Modify: `crates/proxy-crab-mgr/src/lib.rs`
- Modify: `crates/proxy-crab-mgr/Cargo.toml`

- [x] **Step 1: Write serializer tests for standard and fallback bodies**

Cover a text request, binary request, decoded response, raw-Base64 fallback, repeated query parameters, request/response cookies, redirect location, body sizes, unknown timings, RFC 3339 timestamps, and `_proxyCrab` metadata. Assert the HAR root is `log.version = "1.2"`, creator is ProxyCrab, and `entries` is ordered input.

- [x] **Step 2: Run the focused test and verify it fails**

Run: `cargo test -p proxy-crab-mgr har::tests -- --nocapture`

Expected: FAIL because `har.rs` and its conversion functions do not exist.

- [x] **Step 3: Implement the minimal serializer**

Define private Serde structures for the HAR log, entries, request/response, content, cookies, cache, timings, and `_proxyCrab`. Provide an async builder with this boundary:

```rust
pub(crate) struct HarCapture {
    pub detail: CaptureDetail,
    pub request_body: Option<BodySource>,
    pub response_body: Option<BodySource>,
}

pub(crate) async fn serialize(captures: Vec<HarCapture>) -> ManagerResult<Vec<u8>>;
```

Read each stored body completely. Decode `gzip`, `br`, `deflate`, `zstd`, and stacked encodings; if decoding is unsupported or invalid, retain the raw bytes, use Base64, and set the matching body-decoded flag to `false`. Use `content.encoding = "base64"` for binary responses and `_encoding = "base64"` for binary request `postData`.

- [x] **Step 4: Run the focused serializer tests**

Run: `cargo test -p proxy-crab-mgr har::tests -- --nocapture`

Expected: PASS.

### Task 2: Add capture selection and export orchestration

**Files:**
- Modify: `crates/proxy-crab-mgr/src/dto.rs`
- Modify: `crates/proxy-crab-mgr/src/manager.rs`
- Test: `crates/proxy-crab-mgr/src/manager.rs`

- [x] **Step 1: Write failing Manager tests**

Add tests that verify:

- `format` accepts only `har` and returns `unsupported_export_format` otherwise.
- omitted `log_ids` snapshots the Session's current maximum ID and pages all older IDs in ascending order;
- explicit IDs are deduplicated and missing IDs fail the entire call with `log_not_found`;
- an empty explicit list exports an empty HAR;
- failed, in-progress, response-less, and synthetic `CONNECT / tls_mitm` captures are skipped;
- successful normal HTTP and status-101 captures remain eligible.

- [x] **Step 2: Run the focused Manager tests and verify they fail**

Run: `cargo test -p proxy-crab-mgr manager::tests::export -- --nocapture`

Expected: FAIL because the request DTO and Manager method do not exist.

- [x] **Step 3: Add the DTO and Manager contract**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportLogsRequest {
    pub format: String,
    pub session_id: Option<u64>,
    pub log_ids: Option<Vec<u64>>,
}

pub struct LogExport {
    pub session_id: u64,
    pub filename: String,
    pub bytes: Vec<u8>,
}
```

Add `ProxyCrabManager::export_logs`. Resolve an omitted Session using the active Session, validate the Session even for an empty ID list, snapshot the maximum ID before paging an all-Session selection, and load full body sources only for eligible captures. Generate `proxycrab-session-<id>.har` after all reads and serialization succeed.

- [x] **Step 4: Run the focused Manager tests**

Run: `cargo test -p proxy-crab-mgr manager::tests::export -- --nocapture`

Expected: PASS.

### Task 3: Expose the management HTTP endpoint

**Files:**
- Modify: `crates/proxy-crab-mgr/src/http.rs`
- Test: `crates/proxy-crab-mgr/src/http.rs`

- [x] **Step 1: Write failing HTTP contract tests**

Exercise `POST /api/logs/export` with an active and explicit Session. Assert raw HAR JSON without the normal `{ok,data}` envelope, `application/json; charset=utf-8`, `Content-Disposition: attachment; filename="proxycrab-session-<id>.har"`, ordered eligible entries, full decoded/base64 bodies, empty export behavior, `404` for missing explicit IDs, `409` without an active Session, and `400` for unsupported formats or unknown fields.

- [x] **Step 2: Run the endpoint tests and verify they fail**

Run: `cargo test -p proxy-crab-mgr http::tests::export -- --nocapture`

Expected: FAIL with the endpoint missing.

- [x] **Step 3: Add the route and response handler**

Register `.route("/api/logs/export", post(export_logs))` before the dynamic log routes. Convert `LogExport.bytes` directly into the response body, attach download headers and content length, and map `unsupported_export_format` to HTTP 400. Do not publish a UI-sync change for this read-only POST.

- [x] **Step 4: Run HTTP and crate tests**

Run: `cargo test -p proxy-crab-mgr`

Expected: PASS.

### Task 4: Add the bundled Skill command and documentation

**Files:**
- Create: `skills/proxycrab/scripts/har-export.mjs`
- Modify: `skills/proxycrab/scripts/lib/common.mjs`
- Modify: `skills/proxycrab/SKILL.md`
- Modify: `skills/proxycrab/references/http-api.md`
- Modify: `skills/proxycrab/evals/evals.json`
- Modify: `docs/backend-api.md`

- [x] **Step 1: Add a raw-download helper and HAR CLI**

Implement:

```text
node har-export.mjs [--session-id ID] [--log-ids 1,2,3] [--output FILE]
```

The script posts `{format:"har", session_id?, log_ids?}`, writes the response bytes without parsing/reformatting them, uses the server filename when `--output` is absent, and prints a machine-readable summary. Keep `log-export.mjs` unchanged for backward-compatible raw single-log JSON export.

- [x] **Step 2: Document the API and sensitive-data warning**

Document selection, eligibility, ordering, body encoding/decompression fallback, sizes/timings, `_proxyCrab`, error behavior, response headers, examples, and the fact that credentials, cookies, and bodies are not redacted.

- [x] **Step 3: Add a Skill evaluation case**

Add an eval that asks for selected logs and a full Session to be exported as HAR while checking that the agent uses `har-export.mjs`, reports the saved path, and warns about sensitive data without printing it.

- [x] **Step 4: Validate scripts and JSON**

Run:

```bash
node --check skills/proxycrab/scripts/har-export.mjs
node --check skills/proxycrab/scripts/lib/common.mjs
node -e 'JSON.parse(require("node:fs").readFileSync("skills/proxycrab/evals/evals.json", "utf8"))'
```

Expected: all commands exit 0.

### Task 5: Full verification and review

**Files:**
- Verify all files above; do not modify unrelated `src/windows/LogDetailWindow.vue` changes.

- [x] **Step 1: Format and lint**

Run: `cargo fmt --all -- --check`

Run: `cargo clippy -p proxy-crab-mgr --all-targets -- -D warnings`

Expected: both commands exit 0.

- [x] **Step 2: Run relevant project tests**

Run: `cargo test -p proxy-crab-mgr`

Run: `pnpm exec vue-tsc --noEmit`

Expected: both commands exit 0.

- [x] **Step 3: Review the final diff**

Run: `git diff --check` and inspect `git diff -- crates/proxy-crab-mgr skills/proxycrab docs/backend-api.md docs/superpowers/plans/2026-08-04-har-export-api.md`.

Expected: no whitespace errors, no unrelated edits, and every confirmed requirement is represented by code, tests, or documentation.
