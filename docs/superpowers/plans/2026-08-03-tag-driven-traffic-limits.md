# Tag-Driven Traffic Limits Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add per-capture request upload pacing, response download pacing, and configurable upstream request timeout through `_crab_req_speed`, `_crab_resp_speed`, and `_crab_req_timeout` request tags.

**Architecture:** Keep capture/interceptor buffering unchanged and apply pacing only at the final outbound Hyper body boundary. Parse the final request tags immediately before ordinary upstream forwarding and the final response tag after response interceptors; use a custom `Body` whose scheduled per-chunk deadlines prevent initial or catch-up bursts. Preserve all existing bypass, raw CONNECT, upgrade, CA-download, proxy-error, and `_crab_skip` semantics.

**Tech Stack:** Rust, Tokio time, Hyper/http-body, Lua interceptor tags, integration tests, Markdown Skill documentation.

---

### Task 1: Specify parsing and paced-body behavior with unit tests

**Files:**
- Modify: `crates/proxy-crab-mitm/src/proxy.rs`

- [x] **Step 1: Add parser tests for the exact unsigned decimal grammar**

Cover missing tags, `1`, `001`, `0`, empty strings, signs, surrounding whitespace, decimal/scientific/unit suffixes, non-ASCII digits, and values above `u64::MAX`. Assert that missing speed means unlimited and missing timeout resolves to 60,000 ms.

- [x] **Step 2: Run the proxy module tests and verify the parser tests fail**

Run: `cargo test -p proxy-crab-mitm proxy::tests --lib`

Expected: compilation fails because the parsing helper does not exist.

- [x] **Step 3: Add a minimal private parser**

Use an ASCII-digit check followed by `u64` parsing and a positive-value check. Return `None` for missing tags separately from an invalid present value so callers can warn only for an invalid final value.

- [x] **Step 4: Add paced-body timing tests**

Construct a paced body over known bytes, poll it through `BodyExt::collect`, and assert that it does not yield its first data frame before the scheduled byte deadline, finishes no faster than `body_len / bytes_per_second`, preserves the exact size hint and bytes, and does not catch up with a burst after a late poll.

- [x] **Step 5: Implement the minimal paced body**

Store the body bytes, current offset, configured bytes per second, pending chunk size, and an optional pinned sleep. Select bounded chunks, schedule each chunk from the time it is polled, wait before emitting it, and retain an exact remaining size hint so late consumers cannot trigger catch-up bursts.

- [x] **Step 6: Run the proxy module tests**

Run: `cargo test -p proxy-crab-mitm proxy::tests --lib`

Expected: all proxy module unit tests pass.

### Task 2: Apply request speed and timeout to ordinary upstream forwarding

**Files:**
- Modify: `crates/proxy-crab-mitm/src/proxy.rs`
- Modify: `crates/proxy-crab-mitm/tests/proxy_integration.rs`

- [x] **Step 1: Add integration tests for paced request upload**

Create a raw HTTP upstream that timestamps receipt of request-body bytes. Configure a request interceptor with `_crab_req_speed`, send a small POST through a Session, and assert that headers arrive before the body completes, no initial body burst occurs, the final bytes are unchanged, and elapsed upload time meets the configured lower bound.

- [x] **Step 2: Add integration tests for request timeout**

Use a request speed whose required upload duration exceeds `_crab_req_timeout`; assert the client receives 504 and the capture is `failed / upstream / upstream_timeout`. Add a companion request with a sufficient timeout that succeeds.

- [x] **Step 3: Run the new integration tests and verify failure**

Run the new tests by exact test names with `cargo test -p proxy-crab-mitm --test proxy_integration <name> -- --nocapture`.

Expected: elapsed-time and timeout assertions fail before request pacing/tag timeout are wired in.

- [x] **Step 4: Wire the request tags into ordinary forwarding**

After `_crab_skip` and local CA handling, but before building the upstream request, parse `_crab_req_speed` and `_crab_req_timeout`. Warn once per invalid present final value with capture ID and tag name. Pass valid speed to `request_from_data`; wrap `send_upstream` with the parsed timeout or 60,000 ms. Skip all three new tag behaviors for upgrade requests.

- [x] **Step 5: Run the request integration tests**

Expected: paced success, configured timeout failure, and default behavior all pass.

### Task 3: Apply response speed after response interceptors

**Files:**
- Modify: `crates/proxy-crab-mitm/src/proxy.rs`
- Modify: `crates/proxy-crab-mitm/tests/proxy_integration.rs`

- [x] **Step 1: Add response pacing integration tests**

Set `_crab_resp_speed` in a response interceptor that also replaces the body. Read response headers separately, timestamp body chunks, and assert headers are immediate, the final modified bytes are exact, the first body bytes are delayed, and total elapsed time meets the configured lower bound.

- [x] **Step 2: Cover `_crab_skip` and empty body semantics**

Extend the skip integration scenario so a response interceptor produces a paced body. Add an empty-body case proving no artificial delay. Preserve generic tag persistence.

- [x] **Step 3: Run the response tests and verify failure**

Expected: response body currently arrives as a single immediate frame.

- [x] **Step 4: Wire the final response tag into response construction**

After all response interceptors and capture completion, parse `_crab_resp_speed`, warn for an invalid present final value even when the body is empty, and return a paced body only for a valid speed and non-empty body. Keep proxy-generated errors, CA responses, bypass responses, and upgrade tunnels on their existing unpaced paths.

- [x] **Step 5: Run all proxy integration tests**

Run: `cargo test -p proxy-crab-mitm --test proxy_integration`

Expected: all integration tests pass.

### Task 4: Document and evaluate the Lua tag contract

**Files:**
- Modify: `docs/lua-api.md`
- Modify: `docs/backend-api.md`
- Modify: `skills/proxycrab/SKILL.md`
- Modify: `skills/proxycrab/references/lua-api.md`
- Modify: `skills/proxycrab/evals/evals.json`

- [x] **Step 1: Document exact tag formats and stage boundaries**

Explain units, defaults, final-value semantics, per-request isolation, body-only accounting, uniform no-burst pacing, timeout scope, invalid-value warnings, persistence/upstream invisibility, and all excluded paths.

- [x] **Step 2: Add a Skill eval**

Add an evaluation prompt that asks the agent to create an interceptor applying a concrete request speed, response speed, and sufficiently large request timeout, preserve the opposite chain, and verify tag persistence without claiming the tags are upstream headers.

- [x] **Step 3: Validate JSON and search for stale tag documentation**

Run: `jq empty skills/proxycrab/evals/evals.json`

Run: `rg -n "_crab_(skip|req_speed|resp_speed|req_timeout)" docs skills/proxycrab`

Expected: JSON is valid and every user-facing tag description is consistent.

### Task 5: Final verification

**Files:**
- Verify all modified files.

- [x] **Step 1: Format Rust**

Run: `cargo fmt --all -- --check`

Expected: exit 0; if formatting is required, run `cargo fmt --all` and repeat the check.

- [x] **Step 2: Run focused crate verification**

Run: `cargo test -p proxy-crab-mitm`

Expected: all unit and integration tests pass.

- [x] **Step 3: Run workspace verification**

Run: `cargo test --workspace`

Expected: all workspace tests pass with zero failures.

- [x] **Step 4: Review the diff against every confirmed requirement**

Run: `git diff --check`

Run: `git status --short`

Confirm every changed line maps to parsing, pacing, timeout, tests, documentation, or eval coverage. Do not stage or commit unless the user separately requests it.
