# Lua Body Reading and Assets Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let request and response interceptors read the current effective body as text or JSON, replace it with immutable workspace assets, and manage those assets through the authenticated HTTP API.

**Architecture:** Add a workspace-wide `AssetStore` that streams uploads into `assets/`, stores metadata under `assets/.metadata/`, and exposes validated read-only handles to Lua. Add a deferred body coordinator that owns each inbound Hyper body: getters request a complete raw capture and block only the Lua worker, while exchanges with no getter retain the existing bounded streaming path. The effective body is the latest successful string/asset replacement, falling back to the raw inbound body.

**Tech Stack:** Rust 2024, Tokio, Hyper/Axum, mlua 5.4, serde/serde_json, SHA-256, Node.js Skill scripts.

---

### Task 1: Workspace Asset Store

**Files:**
- Create: `crates/proxy-crab-mitm/src/asset.rs`
- Modify: `crates/proxy-crab-mitm/src/lib.rs`
- Modify: `crates/proxy-crab-mitm/src/runtime.rs`

- [ ] Add tests for valid nested IDs, forbidden `.metadata`/dot segments, immutable conflicts, empty uploads, metadata, SHA-256, and nested path conflicts.
- [ ] Implement streamed temporary uploads, atomic no-overwrite publication, sidecar metadata, and read-only asset lookup.
- [ ] Run `cargo test -p proxy-crab-mitm asset::tests`.

### Task 2: Management Asset API

**Files:**
- Modify: `crates/proxy-crab-mgr/src/dto.rs`
- Modify: `crates/proxy-crab-mgr/src/manager.rs`
- Modify: `crates/proxy-crab-mgr/src/http.rs`
- Modify: `crates/proxy-crab-mgr/tests/permission_contract.rs`
- Modify: `src/utils/permission-meta.ts`

- [ ] Test `POST /api/assets/{*asset_id}`, metadata GET, raw GET, invalid formats, conflicts, and permission catalog coverage.
- [ ] Stream request bodies without an application size limit and return 201 metadata envelopes.
- [ ] Stream raw downloads with stored media type, length, filename, and SHA-256 headers.
- [ ] Run manager and permission tests.

### Task 3: Lua Asset and Naming API

**Files:**
- Modify: `crates/proxy-crab-mitm/src/lua.rs`
- Modify: `crates/proxy-crab-mitm/src/lua/codec.rs`
- Modify: `crates/proxy-crab-mitm/src/model.rs`
- Modify: `crates/proxy-crab-mitm/src/breakpoint.rs`

- [ ] Replace `getTag`/`setTag` with `get_tag`/`set_tag` and remove `replace_with_file`.
- [ ] Install interceptor-only `get_asset(id)` returning immutable userdata with `id`, `size`, `content_type`, `sha256`, and `created_at`.
- [ ] Add `replace_with_asset(asset)`, `as_string()`, and `as_json()` with current-header format checks and the 16 MiB decoded limit.
- [ ] Preserve historical `body_replace_file` deserialization and add `body_replace_asset`.
- [ ] Run focused Lua and breakpoint tests.

### Task 4: Deferred Raw Body Capture and Replay

**Files:**
- Modify: `crates/proxy-crab-mitm/src/proxy/body.rs`
- Modify: `crates/proxy-crab-mitm/src/proxy/mitm.rs`
- Test: `crates/proxy-crab-mitm/tests/proxy_integration.rs`

- [ ] Add staged request/response tests proving the first raw getter waits for complete capture, subsequent reads reuse it, and no-getter traffic still streams.
- [ ] Add replacement-chain tests proving string and asset changes are visible immediately and failed replacement retains the last successful body.
- [ ] Implement a command-driven inbound body coordinator with raw capture, trailer preservation, cancellation/error wakeups, and file replay.
- [ ] Pass the same coordinator through saved and temporary breakpoint executions.
- [ ] Run the complete proxy integration suite.

### Task 5: Skill and Documentation

**Files:**
- Modify: `docs/lua-api.md`
- Modify: `docs/backend-api.md`
- Modify: `skills/proxycrab/SKILL.md`
- Modify: `skills/proxycrab/references/lua-api.md`
- Modify: `skills/proxycrab/references/http-api.md`
- Create: `skills/proxycrab/scripts/asset-upload.mjs`
- Modify: `skills/proxycrab/evals/evals.json`
- Modify: `skills/proxycrab/evals/fixtures/*.lua`

- [ ] Document blocking/streaming semantics, effective-body precedence, text/JSON rules, asset immutability, and every HTTP response/error.
- [ ] Add and test the dependency-free upload script.
- [ ] Migrate fixtures and eval expectations to snake_case.

### Task 6: Verification

- [ ] Run `cargo fmt --all -- --check`.
- [ ] Run `cargo test -p proxy-crab-mitm`.
- [ ] Run `cargo test -p proxy-crab-mgr`.
- [ ] Run `node --test skills/proxycrab/scripts/lib/common.test.mjs`.
- [ ] Run the frontend type check/build command from `package.json`.
- [ ] Inspect `git diff --check` and confirm the unrelated untracked SOCKS5 document is untouched.
