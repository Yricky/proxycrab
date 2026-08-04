# Upstream Connection Pool and Proxy Module Split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reuse ordinary HTTP/HTTPS upstream connections across captured and bypassed requests while preserving dedicated connections for Upgrade and raw CONNECT traffic, and reduce `proxy.rs` by extracting focused upstream and body modules.

**Architecture:** A single `UpstreamClient` owned by `ProxyCrab` wraps a `hyper-util` legacy client and a `hyper-rustls` HTTPS connector, pooling by URI origin with HTTP/1 keep-alive and HTTP/2 multiplexing. Upgrade and cleartext HTTP/2 requests retain the existing one-request connection path; raw CONNECT tunnels remain unchanged. `proxy/upstream.rs` owns upstream transport and pooling, while `proxy/body.rs` owns the boxed and paced body implementations.

**Tech Stack:** Rust 2024, Tokio, Hyper 1, hyper-util 0.1, hyper-rustls 0.27, rustls 0.23, Cargo tests.

---

### Task 1: Add and Test the Shared Upstream Client

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/proxy-crab-mitm/Cargo.toml`
- Create: `crates/proxy-crab-mitm/src/proxy/upstream.rs`

- [x] Add `hyper-rustls` with HTTP/1, HTTP/2, WebPKI roots, TLS 1.2, and the existing rustls crypto provider.
- [x] Add a local HTTP/1 test server and a test that sends two sequential requests through one `UpstreamClient` and asserts the server accepts one TCP connection.
- [x] Implement `UpstreamClient` with one pooled `hyper_util::client::legacy::Client`, a 15-second connector timeout, and cancellation-aware requests.
- [x] Preserve the existing dedicated TCP/TLS/Hyper handshake path for Upgrade and cleartext HTTP/2 requests.
- [x] Run the focused upstream module test.

### Task 2: Wire One Pool Across Captured and Bypassed Requests

**Files:**
- Modify: `crates/proxy-crab-mitm/src/runtime.rs`
- Modify: `crates/proxy-crab-mitm/src/proxy.rs`

- [x] Construct one `UpstreamClient` in `ProxyCrab::new` and expose it internally by shared reference.
- [x] Route both `handle_bypass_http` and `handle_session_http_request` through the shared client.
- [x] Remove the former per-request `send_upstream` and `send_on_io` implementations from `proxy.rs`.
- [x] Verify raw CONNECT still calls `tunnel_connect` and Upgrade requests still use a dedicated upstream connection.

### Task 3: Extract Body Implementations

**Files:**
- Create: `crates/proxy-crab-mitm/src/proxy/body.rs`
- Modify: `crates/proxy-crab-mitm/src/proxy.rs`

- [x] Move `BoxError`, `ProxyBody`, `PacedBody`, and `boxed_full` into `proxy/body.rs` without changing their behavior.
- [x] Keep bypass lifecycle tracking in `proxy.rs`, importing the extracted body types through a narrow module interface.
- [x] Keep the existing paced-body timing test passing after the move.

### Task 4: Verify Behavior and Documentation Scope

**Files:**
- Modify: `Cargo.lock`
- Inspect: `skills/proxycrab/`
- Inspect: `docs/backend-api.md`

- [x] Run `cargo fmt --all -- --check`.
- [x] Run `cargo test -p proxy-crab-mitm --lib`.
- [x] Run `cargo clippy -p proxy-crab-mitm --lib --tests -- -D warnings`.
- [x] Confirm the management API, Session/interceptor behavior, and Skill commands are unchanged, so no ProxyCrab Skill or external API documentation update is required.
- [x] Inspect the final diff and confirm the existing 128-slot user change is preserved and unrelated files are untouched.
