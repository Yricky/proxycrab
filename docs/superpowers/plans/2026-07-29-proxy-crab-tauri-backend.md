# Proxy Crab Tauri Backend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the template Tauri backend with a macOS-ready ProxyCrab backend split into reusable MITM and management crates, using Lua scripts and exposing matching HTTP and Tauri capabilities.

**Architecture:** `proxy-crab-mitm` owns the fixed-workspace runtime, CA, sessions, SQLite capture storage, Lua execution, proxy lifecycle, diagnostic captures, and bounded application log buffer. `proxy-crab-mgr` defines object-safe typed management operations, adapts the MITM runtime, and maps the same operations to the local HTTP API. `src-tauri` resolves the startup workspace once, initializes tracing and both services, and contains only thin commands over `Arc<dyn ProxyCrabManager>`.

**Tech Stack:** Rust 2024, Tokio, Hyper/Hyper-Util, Rustls, rcgen, rusqlite, mlua Lua 5.4, Axum, async-trait, serde, tracing, Tauri 2.

---

### Task 1: Cargo workspace and crate boundaries

**Files:**
- Create: `Cargo.toml`
- Create: `crates/proxy-crab-mitm/Cargo.toml`
- Create: `crates/proxy-crab-mgr/Cargo.toml`
- Modify: `src-tauri/Cargo.toml`

- [x] Define a resolver-3 workspace containing both library crates and `src-tauri`.
- [x] Put shared dependency versions in `[workspace.dependencies]`.
- [x] Keep Tauri out of both reusable library crates.
- [x] Run `cargo metadata --no-deps` and verify all three members resolve.

### Task 2: MITM domain model, log buffer, and workspace

**Files:**
- Create: `crates/proxy-crab-mitm/src/lib.rs`
- Create: `crates/proxy-crab-mitm/src/model.rs`
- Create: `crates/proxy-crab-mitm/src/log_buffer.rs`
- Create: `crates/proxy-crab-mitm/src/workspace.rs`
- Test: inline unit tests in those modules

- [x] Define serializable configuration, session, capture, error, body, column, interceptor, proxy-status, and log-entry DTOs.
- [x] Implement a 10,000-entry `VecDeque` log buffer with monotonic sequence numbers and incremental querying.
- [x] Implement a minimal tracing layer that stores only sequence, timestamp, level, and rendered message.
- [x] Resolve `app_data_dir/config.json` once; use `app_data_dir/workspace` when missing or invalid and persist the fallback pointer.
- [x] Lock the selected workspace exclusively for the process lifetime.
- [x] Load or create workspace directories and JSON configuration atomically.
- [x] Persist active session ID and reject deletion of the active session.
- [x] Test ring eviction, pointer fallback, workspace locking, session activation, and active-session deletion.

### Task 3: CA, capture database, and Lua scripts

**Files:**
- Create: `crates/proxy-crab-mitm/src/ca.rs`
- Create: `crates/proxy-crab-mitm/src/storage.rs`
- Create: `crates/proxy-crab-mitm/src/lua.rs`
- Test: inline unit tests in those modules

- [x] Generate one ECDSA P-256 CA per workspace and regenerate missing or corrupt files.
- [x] Generate and cache host certificates suitable for rustls server use.
- [x] Store captures in per-session SQLite databases with request/response metadata, outcome, stage, stable error kind, raw message, modifications, and body files.
- [x] Insert captures before forwarding ordinary requests and create diagnostic CONNECT captures for TLS rejection or tunneling.
- [x] Expose Lua userdata for read-only log data and mutable request/response headers and bodies.
- [x] Remove unsafe Lua standard libraries and enforce a 100,000-instruction execution limit.
- [x] Validate scripts on create/update; require boolean filters and scalar-or-nil column results.
- [x] Test CA round trips, diagnostic rows, body details, Lua filtering, columns, mutations, and instruction exhaustion.

### Task 4: MITM proxy runtime

**Files:**
- Create: `crates/proxy-crab-mitm/src/proxy.rs`
- Create: `crates/proxy-crab-mitm/src/runtime.rs`
- Test: integration tests under `crates/proxy-crab-mitm/tests/`

- [x] Implement HTTP/1.1 proxying, CONNECT, HTTPS MITM, HTTP/2 inside MITM, upgrades, and non-TLS tunnels.
- [x] Serve only `http://proxy.crab/ca.crt` locally.
- [x] Pin every request to the active session selected when that request is first recognized.
- [x] Record normalized failures for malformed requests, TLS rejection, upstream errors, body errors, and shutdown.
- [x] Refuse to forward when the initial capture insert fails.
- [x] Stop accepting immediately, drain for five seconds, then cancel and mark unfinished captures as `proxy_shutdown`.
- [x] Test plain HTTP, trusted HTTPS, untrusted CA failure capture, session hot-switching, and start/stop status.

### Task 5: Management trait and HTTP API

**Files:**
- Create: `crates/proxy-crab-mgr/src/lib.rs`
- Create: `crates/proxy-crab-mgr/src/dto.rs`
- Create: `crates/proxy-crab-mgr/src/manager.rs`
- Create: `crates/proxy-crab-mgr/src/http.rs`
- Test: `crates/proxy-crab-mgr/tests/api.rs`

- [x] Define one `Send + Sync` object-safe async management trait using typed DTOs and a stable `{code,message}` error.
- [x] Implement it with an adapter around `Arc<ProxyCrab>`.
- [x] Preserve current log, column-script, column, and interceptor routes and response envelopes.
- [x] Add workspace, config, proxy, session, session-log, filter-history, CA, interceptor-order, and system-log routes.
- [x] Bind management HTTP to loopback and preserve Tauri operation if HTTP binding fails.
- [x] Test route status codes, envelopes, compatibility payloads, and equivalence to direct trait results.

### Task 6: Tauri lifecycle and commands

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Keep unchanged: all files below `src/`

- [x] Resolve app data during Tauri setup and initialize the fixed workspace.
- [x] Install the shared tracing/log-buffer layer.
- [x] Store `Arc<dyn ProxyCrabManager>` and HTTP service status in Tauri managed state.
- [x] Add thin commands for every trait operation; commands return the same data DTOs and `{code,message}` errors.
- [x] Start management HTTP on app startup, leave MITM stopped, and gracefully stop services on exit.
- [x] Verify `cargo check -p proxy-crab-t`.

### Task 7: Documentation and final verification

**Files:**
- Modify: `README.md`
- Create: `docs/backend-api.md`
- Create: `docs/lua-api.md`

- [x] Document crate ownership, startup workspace semantics, service lifecycle, routes, commands, error envelope, capture outcomes, and the CA URL.
- [x] Document the complete Lua object model and examples.
- [x] Run `cargo fmt --check`.
- [x] Run `cargo check --workspace`.
- [x] Run `cargo test --workspace`.
- [x] Run `cargo clippy --workspace --all-targets -- -D warnings`.
- [x] Run a macOS Tauri backend build/check without changing or testing frontend source.
