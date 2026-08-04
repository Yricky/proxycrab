# Request TLS Insecure Tag Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow an intercepted request tagged `_crab_tls_insecure=true` to connect to an HTTPS upstream without certificate verification while preserving verification for every untagged request.

**Architecture:** Parse the special tag after request interceptors finish and pass a boolean transport option into `UpstreamClient`. Keep verified and insecure `hyper-util` clients in separate instances so their origin-keyed pools can never share TLS connections; use the same transport option when Upgrade/WebSocket traffic takes the dedicated connection path. Implement the insecure rustls verifier only in the upstream transport module.

**Tech Stack:** Rust 2024, Tokio, Hyper 1, hyper-util 0.1, hyper-rustls 0.27, rustls 0.23, rcgen, Cargo tests.

---

### Task 1: Define special-tag semantics

**Files:**
- Modify: `crates/proxy-crab-mitm/src/proxy.rs`
- Test: `crates/proxy-crab-mitm/src/proxy.rs`

- [x] **Step 1: Write failing parser tests**

Add focused unit tests proving that `_crab_tls_insecure` is disabled when absent, enabled only for the exact value `true`, and disabled with a warning-compatible invalid result for empty, mixed-case, or numeric values.

- [x] **Step 2: Run the parser tests to verify failure**

Run: `cargo test -p proxy-crab-mitm tls_insecure_tag --lib`

Expected: FAIL because the tag constant and parser do not exist.

- [x] **Step 3: Implement strict parsing**

Add `CRAB_TLS_INSECURE_TAG` and a small parser that returns `true` only for an exact `true` value. Parse it after request interceptors and before `request_from_data`, then pass the result to the upstream client. Do not serialize the tag into HTTP headers.

- [x] **Step 4: Run the parser tests**

Run: `cargo test -p proxy-crab-mitm tls_insecure_tag --lib`

Expected: PASS.

### Task 2: Isolate verified and insecure upstream pools

**Files:**
- Modify: `crates/proxy-crab-mitm/src/proxy/upstream.rs`
- Test: `crates/proxy-crab-mitm/src/proxy/upstream.rs`

- [x] **Step 1: Write failing selection tests**

Add transport tests that exercise selection between independently built verified and insecure clients and keep existing HTTP/1 reuse behavior.

- [x] **Step 2: Run the upstream unit tests to verify failure**

Run: `cargo test -p proxy-crab-mitm proxy::upstream::tests --lib`

Expected: FAIL because `UpstreamClient` has only one pool and `send` has no TLS policy argument.

- [x] **Step 3: Implement the two-pool transport**

Change `UpstreamClient` to hold `verified` and `insecure` pooled clients. Build the insecure `ClientConfig` with a local `ServerCertVerifier`, select the matching pool in `send`, and pass the same policy into `send_dedicated` so HTTPS Upgrade/WebSocket traffic behaves consistently. Leave HTTP behavior unchanged.

- [x] **Step 4: Run upstream unit tests**

Run: `cargo test -p proxy-crab-mitm proxy::upstream::tests --lib`

Expected: PASS.

### Task 3: Verify self-signed HTTPS behavior end to end

**Files:**
- Modify: `crates/proxy-crab-mitm/tests/proxy_integration.rs`

- [x] **Step 1: Add a local self-signed HTTPS server helper**

Generate a localhost certificate with `rcgen`, serve HTTP/1.1 through `tokio-rustls`, and count accepted TLS connections. Keep all traffic on loopback.

- [x] **Step 2: Add the behavior and isolation test**

Issue an untagged request and assert 502, enable a request interceptor that sets `_crab_tls_insecure=true` and assert a successful response, then remove the interceptor and assert the same origin returns 502 again. Assert the successful capture retains the tag and the upstream request contains no `_crab_tls_insecure` header.

- [x] **Step 3: Run the integration test**

Run: `cargo test -p proxy-crab-mitm --test proxy_integration tls_insecure`

Expected: PASS, including the final untagged rejection that proves pool isolation.

### Task 4: Document and evaluate the feature

**Files:**
- Modify: `docs/lua-api.md`
- Modify: `docs/backend-api.md`
- Modify: `skills/proxycrab/SKILL.md`
- Modify: `skills/proxycrab/references/lua-api.md`
- Modify: `skills/proxycrab/references/http-api.md`
- Modify: `skills/proxycrab/references/best-practices.md`
- Modify: `skills/proxycrab/evals/evals.json`
- Create: `skills/proxycrab/evals/fixtures/test-tls-insecure.lua`

- [x] **Step 1: Document exact semantics and warning**

State that `_crab_tls_insecure=true` disables upstream HTTPS certificate and hostname verification for the tagged request, is ignored for HTTP, is not an upstream header, and should be limited to controlled test systems.

- [x] **Step 2: Add an eval case and fixture**

Add a fixture that conditionally sets the tag for a concrete test host and an eval requiring preservation of existing interceptor chains plus capture-based verification.

- [x] **Step 3: Validate JSON and documentation references**

Run: `jq empty skills/proxycrab/evals/evals.json`

Expected: exit 0.

### Task 5: Final verification

**Files:**
- Modify: all files above only as required by formatting.

- [x] **Step 1: Format code**

Run: `cargo fmt --all -- --check`

Expected: exit 0 after applying `cargo fmt --all` if required.

- [x] **Step 2: Run crate tests**

Run: `cargo test -p proxy-crab-mitm`

Expected: all tests pass.

- [x] **Step 3: Run workspace checks**

Run: `cargo test --workspace`

Expected: all workspace tests pass.

- [x] **Step 4: Review the final diff**

Run: `git diff --check && git status --short`

Expected: no whitespace errors and only feature-related files are changed.
