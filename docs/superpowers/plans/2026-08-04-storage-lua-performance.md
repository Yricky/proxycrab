# Storage and Lua Query Performance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove repeated SQLite connection setup and repeated Lua VM/script compilation from capture and log-query hot paths without changing externally visible behavior.

**Architecture:** Each clone of a capture or bypass store shares one mutex-protected SQLite connection configured at store open. Each management query compiles one isolated Lua evaluator per referenced script; evaluations reuse its VM and function while recreating the per-row global environment and resetting instruction and warning state.

**Tech Stack:** Rust 2024, rusqlite, mlua Lua 5.4, Tokio, Cargo tests.

---

### Task 1: Reuse Store SQLite Connections

**Files:**
- Modify: `crates/proxy-crab-mitm/src/storage.rs`
- Modify: `crates/proxy-crab-mitm/src/bypass.rs`

- [x] Add tests asserting cloned stores share the same connection owner.
- [x] Run the focused tests and verify they fail before the struct change.
- [x] Replace stored database paths with `Arc<Mutex<Connection>>`, configure each connection once in `open`, and return a guarded connection from the existing helper.
- [x] Run storage and bypass tests and verify lifecycle, migrations, reads, and writes remain unchanged.

### Task 2: Reuse Isolated Lua Evaluators Per Query

**Files:**
- Modify: `crates/proxy-crab-mitm/src/lua.rs`
- Modify: `crates/proxy-crab-mgr/src/manager.rs`

- [x] Add Lua tests that reuse one evaluator across rows and assert globals, mutable standard-library tables, instruction budgets, and warning counts remain row-local.
- [x] Run the focused Lua tests and verify the reusable evaluator API is absent.
- [x] Add reusable filter and column evaluators that compile once, clone a clean execution environment per row, and reset the instruction counter before every call; retain existing single-evaluation functions as wrappers.
- [x] Compile one evaluator per referenced script in `log_ids` and `log_views`, preserving existing missing-script, invalid-script, non-match, and per-cell exception behavior.
- [x] Run Lua and manager tests.

### Task 3: Document and Verify the Optimization

**Files:**
- Modify: `README.md`
- Modify: `docs/backend-api.md`
- Modify: `skills/proxycrab/references/lua-api.md`

- [x] Document connection reuse and query-scoped Lua evaluator reuse as internal implementation details with unchanged API semantics.
- [x] Run `cargo fmt --all -- --check`, `cargo test -p proxy-crab-mitm --lib`, and `cargo test -p proxy-crab-mgr --lib`.
- [x] Inspect the final diff and confirm no unrelated behavior or files changed.
