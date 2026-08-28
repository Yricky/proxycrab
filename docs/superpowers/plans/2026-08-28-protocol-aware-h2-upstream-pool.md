# Protocol-Aware H2 Upstream Pool Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restore HTTP/2 connection reuse without reintroducing the HTTP/1.1-version rewrite that removed `:authority` and caused strict gateways to return 400.

**Architecture:** Keep the existing `hyper-util` pool strictly HTTP/1.1. For downstream H2 requests, maintain one lazily established Hyper H2 sender per `(scheme, authority, TLS policy)` and let TLS ALPN select H2 or an HTTP/1.1 fallback. Serialize only first connection establishment per origin; cloned H2 senders retain multiplexing without serializing requests.

**Tech Stack:** Rust 2024, Tokio, Hyper 1, hyper-util 0.1, hyper-rustls 0.27, rustls 0.23, Cargo tests.

---

### Task 1: Reproduce Lost H2 Reuse

**Files:**
- Modify and test: `crates/proxy-crab-mitm/src/proxy/upstream.rs`

- [x] **Step 1: Extend the strict H2 regression server to accept multiple TLS connections and count accepts.**

  Send two sequential H2 requests through the same `UpstreamClient`, retain the existing `:authority`/no-`Host` assertions, and assert `accepted == 1`.

- [x] **Step 2: Run the focused test and verify the current dedicated implementation fails.**

  Run: `cargo test -p proxy-crab-mitm https_http2_reuses_connection_and_reconnects_after_close -- --nocapture`

  Expected: FAIL because two requests create two upstream TLS connections.

### Task 2: Add the Protocol-Aware Pool

**Files:**
- Modify: `crates/proxy-crab-mitm/src/proxy/upstream.rs`

- [x] **Step 1: Split HTTP/1 and H2 pooling responsibilities.**

  Configure the legacy clients with `.enable_http1()` only. Add a shared map keyed by scheme, URI authority, and `tls_insecure` whose entry state is unknown, negotiated HTTP/1.1, or a cloneable `hyper::client::conn::http2::SendRequest<ProxyBody>`.

- [x] **Step 2: Lazily establish one connection per H2 origin.**

  Hold a per-origin async mutex only while checking state or performing the first TCP/TLS/Hyper handshake. On H2 ALPN, store a cloned sender and send with HTTP/2 version plus `:authority` semantics. On HTTP/1.1 ALPN, remember the protocol, send the first request on the negotiated connection, and route later requests through the HTTP/1-only legacy pool.

- [x] **Step 3: Reconnect lazily after a pooled H2 sender closes.**

  Check `SendRequest::is_closed()` before cloning it. Replace a closed sender through the same per-origin single-flight path; do not retry a request after its body has been handed to Hyper.

- [x] **Step 4: Run focused tests.**

  Run: `cargo test -p proxy-crab-mitm proxy::upstream::tests -- --nocapture`

  Expected: all upstream tests pass, including one accepted TLS connection for two H2 requests.

### Task 3: Verify the Package

**Files:**
- Inspect: `crates/proxy-crab-mitm/src/proxy/upstream.rs`
- Preserve: `Cargo.lock`
- Preserve: `src-tauri/src/share_ui.rs`

- [x] **Step 1: Run formatting and tests.**

  Run: `cargo fmt -p proxy-crab-mitm -- --check`

  Run: `cargo test -p proxy-crab-mitm`

- [x] **Step 2: Run lint checks.**

  Run: `cargo clippy -p proxy-crab-mitm --lib --tests -- -D warnings -A clippy::type_complexity`

  Expected: PASS; `type_complexity` is the pre-existing exception in `proxy/body.rs`.

- [x] **Step 3: Inspect the final diff.**

  Run: `git diff --check`

  Confirm only the upstream implementation, its tests, and this plan changed; retain the user's existing `Cargo.lock` and `src-tauri/src/share_ui.rs` modifications.
