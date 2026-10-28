# WebSocket Capture Implementation Plan

> **For agentic workers:** Use subagent-driven-development for the isolated capture/storage task and review; integrate the API and UI in this session.

**Goal:** Capture HTTP/1.1 WebSocket 13 business messages in independent logs with directional storage, policy-controlled reads, and a two-lane timeline.

**Architecture:** Preserve the HTTP handshake and link a live WebSocket capture. Transparently tee traffic into bounded incremental frame decoders. Persist complete decoded messages in directional body containers; expose only metadata and policy-filtered individual payloads.

**Tech Stack:** Rust/Tokio/flate2/mlua/SQLite, management HTTP, Vue/TypeScript.

## Agreed contract

- ws/wss URI, WebSocket/13 version; handshake and connection IDs linked.
- LE u32 message ID, record length, metadata length, UTF-8 JSON, payload; record length excludes first eight bytes. Versioned magic once per file.
- Complete messages only, per-direction IDs, 16 MiB decoded limit; omission retains metadata, forwarding continues.
- HTTP/1.1 Upgrade only; permessage-deflate; control frames not listed; Close metadata preserved.
- No raw-container read/export. HAR excludes connection records, retains handshakes.
- GET /api/logs/{id}/websocket/messages?session_id=&side=&after_id=&limit= (default 500, max 2000).
- GET /api/logs/{id}/websocket/messages/{message_id}/body?session_id=&side=&max_size=.
- Policy message invocation uses context.kind=websocket_message, req-only for C2S, resp-only for S2C. Body is a single message. Identity/share/startup policy applies; failures never expose originals; old policies require explicit message capability opt-in.
- 84px Canvas shared-time-axis C2S/S2C timeline, zoom/scroll/follow, selection loads body lazily, two-second incremental polling while live.

## Tasks and validation

- [x] Capture/storage: websocket.rs; storage.rs/runtime.rs/model.rs/proxy/mitm.rs. Verify masked fragmented input, bidirectional traffic, compressed context takeover, oversized message recovery, incomplete container tails, live activity and close lifecycle.
- [x] Lua policy: lua/redaction.rs. Verify req/resp exclusivity, context metadata, readonly connection fields, text/JSON/binary replacement, hide distinct from empty, explicit capability, fail closed.
- [x] Management API: dto.rs/manager.rs/http.rs/session_share.rs/permissions and HAR. Verify pagination bounds, payload limits, policy routes and shares, container denial and HAR exclusion.
- [x] UI: backend types and transports, dedicated WebSocket detail component. Verify request/response cursors, selection races, redaction invalidation, active polling, capability gating, zoom and scrolling.
- [x] Docs and scripts: docs/backend-api.md, docs/lua-api.md, skills/proxycrab reference/scripts/evals. Document format, new API and policy opt-in.
- [x] Final validation: cargo test -p proxy-crab-mitm; cargo test -p proxy-crab-mgr; frontend typecheck/tests/build per package scripts; review spec coverage and security read paths. No Gradle.

## Review and verification

Capture/storage spec and quality review completed. Fixed cancellation during upgrade/forwarding,
retained Session pins through blocking writes, finalized shutdown metadata, and bounded offset index
following old-message lookups. API/UI review completed: context uses userdata to prevent rawset bypass;
previous/next navigation reaches coincident timestamp messages. Negotiated metadata follows fully
redacted handshake evaluation. Browser fixture verified JSON, binary Hex, omitted payload and navigation;
temporary fixture/server removed. Final suite totals recorded in the chat completion.

UI follow-up: remove handshake/connection navigation buttons and render the two-lane waterfall with a viewport-sized Canvas at 100px total height.

UI density follow-up: remove the extra 16px below Canvas; use 22px icon buttons and compact toolbars. Replace slider with Command/Ctrl + wheel pointer-anchored zoom and a fixed 100px duration ruler.

Timeline follow-up: remove manual follow/refresh actions; fit mode continuously fits new data, while zoomed mode preserves an absolute scale and uses per-lane unread capsules. 1× = 100px/s, capped at64× independent of duration. Remove the scale ruler; use adaptive1/2/5 time steps and short divider ticks.
