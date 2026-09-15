# Upstream HTTP Version Tag Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `_crab_upstream_http_version` so request interceptors can force ProxyCrab's upstream hop to exact `HTTP/1.1` or `HTTP/2` while retaining the downstream protocol in capture metadata.

**Architecture:** Parse the final request tag into a small upstream transport enum without mutating `RequestData.version`. Pass the optional override into `UpstreamClient`; forced HTTP/1.1 uses the existing HTTP/1 pool, while forced HTTP/2 uses an H2-only TLS ALPN policy and a pool isolated from negotiated H2-with-H1-fallback traffic. Leave untagged, bypass, raw CONNECT, CA, and proxy-error behavior unchanged.

**Tech Stack:** Rust 2024, Tokio, Hyper 1, hyper-util 0.1, rustls 0.23, Lua 5.4 documentation, Cargo tests.

---

### Task 1: Define and parse the enum tag

**Files:**
- Modify: `crates/proxy-crab-mitm/src/proxy.rs`
- Modify: `crates/proxy-crab-mitm/src/proxy/mitm.rs`
- Test: `crates/proxy-crab-mitm/src/proxy.rs`

- [x] **Step 1: Write the failing parser test**

  Assert that an absent value returns no override, exact `HTTP/1.1` and `HTTP/2` values return their matching enum members, and empty, differently cased, or unsupported values return an invalid result.

- [x] **Step 2: Run the focused test and verify failure**

  Run: `cargo test -p proxy-crab-mitm upstream_http_version_tag --lib`

  Expected: compilation failure because the parser and enum do not exist.

- [x] **Step 3: Implement strict parsing**

  Add `CRAB_UPSTREAM_HTTP_VERSION_TAG`, an `UpstreamHttpVersion::{Http1, Http2}` enum, and a parser matching only the exact strings. Resolve it after request interceptors; warn and ignore a present invalid value.

- [x] **Step 4: Run the parser test**

  Run: `cargo test -p proxy-crab-mitm upstream_http_version_tag --lib`

  Expected: PASS.

### Task 2: Apply exact upstream protocol selection

**Files:**
- Modify: `crates/proxy-crab-mitm/src/proxy/upstream.rs`
- Modify: `crates/proxy-crab-mitm/src/proxy/bypass.rs`
- Test: `crates/proxy-crab-mitm/src/proxy/upstream.rs`

- [x] **Step 1: Write failing transport tests**

  Send a request marked as HTTP/1.0 to an HTTP/1.1-only loopback server with the HTTP/1.1 override and assert the exact request version. Send a request marked as HTTP/1.1 to an h2c loopback server with the HTTP/2 override and assert the server receives HTTP/2. Add a TLS test proving forced HTTP/2 does not reuse or accept the normal HTTP/1.1 ALPN fallback.

- [x] **Step 2: Run the focused upstream tests and verify failure**

  Run: `cargo test -p proxy-crab-mitm proxy::upstream::tests --lib`

  Expected: compilation failure because `UpstreamClient::send` has no protocol override.

- [x] **Step 3: Implement protocol-aware selection**

  Extend `send` with `Option<UpstreamHttpVersion>`. Route forced HTTP/1.1 to the existing HTTP/1 client, forced HTTP/2 to H2 with H2-only ALPN, and untagged traffic through the current version-preserving behavior. Include fallback policy in the H2 pool key and pass the override through the dedicated Upgrade path. Pass `None` from bypass traffic.

- [x] **Step 4: Run the upstream tests**

  Run: `cargo test -p proxy-crab-mitm proxy::upstream::tests --lib`

  Expected: PASS.

### Task 3: Verify browser-H2 to local-H1 behavior end to end

**Files:**
- Modify: `crates/proxy-crab-mitm/tests/proxy_integration.rs`

- [x] **Step 1: Add the integration test**

  Start an HTTP/1.1-only loopback upstream. Connect to ProxyCrab's MITM listener with TLS ALPN `h2`, attach a request interceptor that rewrites the URI to the loopback server and sets `_crab_upstream_http_version=HTTP/1.1`, then send an HTTP/2 request.

- [x] **Step 2: Assert both hops and capture metadata**

  Assert that the downstream response is HTTP/2, the upstream request line is HTTP/1.1, the special tag was not sent as a header, and the capture retains request version `HTTP/2` plus the tag value.

- [x] **Step 3: Run the integration test**

  Run: `cargo test -p proxy-crab-mitm --test proxy_integration upstream_http_version`

  Expected: PASS.

### Task 4: Document and evaluate the feature

**Files:**
- Modify: `docs/lua-api.md`
- Modify: `docs/backend-api.md`
- Modify: `skills/proxycrab/SKILL.md`
- Modify: `skills/proxycrab/references/lua-api.md`
- Modify: `skills/proxycrab/references/http-api.md`
- Modify: `skills/proxycrab/references/best-practices.md`
- Modify: `skills/proxycrab/evals/evals.json`
- Create: `skills/proxycrab/evals/fixtures/test-upstream-http-version.lua`

- [x] **Step 1: Document exact semantics**

  State that only exact `HTTP/1.1` and `HTTP/2` values are valid, the setting controls only the captured upstream hop after request interceptors, forced HTTP/2 is strict, the tag remains capture metadata rather than a header, and invalid values warn and preserve default behavior.

- [x] **Step 2: Add an eval fixture**

  Add a narrowly matched Rsbuild example that rewrites to `http://127.0.0.1:3001` and sets `HTTP/1.1`, plus an eval requiring preserved interceptor chains and capture evidence.

- [x] **Step 3: Validate JSON and references**

  Run: `jq empty skills/proxycrab/evals/evals.json`

  Run: `rg -n "_crab_upstream_http_version" docs skills/proxycrab`

  Expected: valid JSON and consistent tag documentation.

### Task 5: Verify the implementation

**Files:**
- Inspect: all modified feature files.

- [x] **Step 1: Format and run crate tests**

  Run: `cargo fmt --all -- --check`

  Run: `cargo test --workspace`

  Expected: PASS.

- [x] **Step 2: Run lint checks**

  Run: `cargo clippy -p proxy-crab-mitm --lib --tests -- -D warnings -A clippy::type_complexity -A clippy::needless_borrow -A clippy::derivable_impls -A clippy::collapsible_if`

  Expected: PASS. The additional three allows cover pre-existing warnings in untouched code; the strict command was also run and reported only those existing warnings.

- [x] **Step 3: Inspect the final diff**

  Run: `git diff --check && git status --short`

  Expected: no whitespace errors and only feature-related source, tests, documentation, skill, eval, and plan files changed. Do not create a commit unless the user requests one.
