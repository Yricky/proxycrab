# WebSocket Interceptors Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Implement the agreed connection-scoped Lua WebSocket callbacks, deterministic FIFO interception, provenance and Canvas presentation.

**Architecture:** Retain successful request Lua registrations and immutable source execution references for the upgraded connection. Decode incoming messages with compression context, process one root and its FIFO descendants serially, persist every disposition, and encode outgoing uncompressed RFC6455 frames (valid with negotiated permessage-deflate). Keep passive capture for connections without callbacks.

**Tech Stack:** Rust, mlua Lua 5.4, Tokio, flate2, existing capture containers/SQLite, Vue/TypeScript.

---

- [x] Lua registration and lifecycle: `crates/proxy-crab-mitm/src/lua.rs`, `lua/websocket.rs`. Test duplicate registration caught/uncaught, failed initialization, captured request aliases read-only, persistent closure state, transactional drop/send and sticky queue overflow.
- [x] Transport: `crates/proxy-crab-mitm/src/websocket/transport.rs`. Test fragmented and masked messages, compression takeover/reset including dropped input, controls and malformed frames; outbound masking and uncompressed encoding.
- [x] Chain and persistence: `websocket/interception.rs`, `websocket.rs`, `storage.rs`, `proxy/mitm.rs`. Queue descendants after current message in actual send order, next callback only, 16384 waiting items including staged sends. Test BFS, cross-direction history, dropped/failed persistence and source refs. Reuse source scripts from handshake execution snapshots.
- [x] Frontend and skill: `src/components/WebSocketDetail.vue`, `src/utils/websocket-timeline.ts`, `src/api/types.ts`, `skills/proxycrab/`. Verify source border colors, dropped/failed fill, provenance navigation, code snapshots and callback errors.
- [x] Integration: run `cargo test -p proxy-crab-mitm`, `cargo test -p proxy-crab-mgr`, `pnpm test:unit`, `pnpm build`, relevant Node skill tests, formatting and diff review. No Gradle or external device operations.

## Agreed behavior

Text/binary only; controls stay at protocol layer. A request interceptor registers at most once; second call throws (caught preserves first, uncaught cancels registration). Callback failure discards staged sends/drop and continues original message; later invocations continue. Request aliases read-only after request execution. History contains finalized original/generated/dropped/failed messages both ways; provenance includes root, immediate parent, source interceptor execution, invocation and send ordinal. Queue overflow is current callback failure. Dropped messages are persisted and stop the chain. Closed connection queued messages persist unsent/failed. FIFO root chain finishes before the next network root. Original source green, generated blue, dropped fill gray, unsent/failed red, source border retained. Snapshot references are fixed at registration and survive later edits.

## Validation results

- `cargo test -p proxy-crab-mitm -p proxy-crab-mgr --offline`: 285 tests passed (140 MITM unit, 48 proxy integration, 87 manager unit, 3 permission contract, 7 redaction).
- `pnpm test:unit`: 102 tests across 21 files passed.
- `node --test skills/proxycrab/scripts/*.test.mjs`: 12 passed.
- `pnpm run build`: typecheck and production build passed; existing large Monaco chunk warning remains.
- `cargo fmt --all` and `git diff --check`: clean.
- Added real HTTP Upgrade regression for retained closure state, read-only captured request body, script edits leaving existing connection unchanged, and original source snapshot retrieval.
- Read-only review findings resolved: unsupported temporary registration cannot freeze parent state; writer/reader metadata bounds agree; repeated payload sends share backing storage; completed request body readers retain final result; close/cancellation persists pending and decoded buffered messages.
