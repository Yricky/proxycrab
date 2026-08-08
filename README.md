# ProxyCrab Tauri

ProxyCrab is a macOS-first Tauri application backend for HTTP/HTTPS MITM capture. The frontend is intentionally maintained separately; this repository's Rust workspace exposes the complete backend through matching management HTTP and Tauri command surfaces.

## Rust workspace

- `crates/proxy-crab-mitm`: proxy lifecycle, CA/TLS, sessions, SQLite captures, request/response bodies, Lua scripts, diagnostic failures, and the in-memory system-log ring.
- `crates/proxy-crab-mgr`: the object-safe `ProxyCrabManager` trait, its MITM adapter, typed DTOs, stable errors, the local management HTTP server, and a route-level permission contract.
- `src-tauri`: Tauri initialization, application lifecycle, and thin commands that call the same management trait as HTTP handlers.

Neither reusable crate depends on Tauri.

## Runtime defaults

- MITM proxy: `0.0.0.0:8089`, stopped when the app launches.
- Management API: `127.0.0.1:18089`, started with the app.
- CA download through the proxy: `http://proxy.crab/ca.crt`.
- System log buffer: the newest 10,000 entries in memory.
- Captured request/response bodies stream directly to disk without an application-level size limit; at most 256 client connections are accepted.
- Blocking capture/bypass queries and management Lua evaluations: at most eight concurrently; excess work waits without occupying Tokio worker threads.

The Tauri application data directory contains `config.json`, which points at the workspace. When the pointer is absent or invalid, `app_data_dir/workspace` is used and persisted. A changed pointer takes effect only on the next application launch.

The management API is loopback-only and supports workspace-scoped API keys through exactly one
`Authorization: Bearer <key>` header. Every API key and the local no-key identity have independent
per-route `allow`, `approval`, or `deny` permissions. Approval waits for a desktop decision for up
to 30 seconds; the toolbar hand indicator opens the pending list. API keys and permissions are
managed under Settings > 管理接口 and are not exposed as HTTP management endpoints. Direct Tauri
commands remain trusted and bypass HTTP authentication.

Permission state is stored in the active workspace's `http_api_permissions.json`; a corrupt file
stops only the management HTTP service rather than widening access. Full API keys are displayed once
at creation and are never persisted. Bundled ProxyCrab Skill scripts accept a key only through the
`PROXYCRAB_API_KEY` environment variable.

Each workspace root has a `workspace_schema.json` version label. All persisted-data upgrades are
registered in the centralized migration module and run before workspace stores are opened.
Workspace schema v2 enables SQLite WAL for Session capture databases so management readers can
recover and interoperate with the established workspace format. Each opened capture or bypass
store reuses one configured SQLite connection across its clones and serializes access to it.
Active Sessions live under `sessions/`; archived Sessions move intact to `sessions_archived/` and
can only be permanently deleted from there. Effective active-Session or selected-routing changes
rotate all downstream connections, tunnels/upgrades, and upstream pools while keeping the proxy
listener running; identical updates do not rotate connections.

Workspace-scoped Agent behavior presets are owned by the management layer and stored under
`agents/`. The desktop AI menu edits and activates them, while `GET /api/agents.md` returns the
active Markdown as raw `text/plain`. The bundled ProxyCrab Skill reads this endpoint before making
other management calls. A supplied log filter can be kept stateless with
`persist_filter: false`.

The Tauri command surface also reports the management HTTP service's `running`, `host`, `port`, and startup error state through `get_http_service_status`.

## Development

```bash
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

The frontend source under `src/` is not part of the backend implementation.

See [backend API](docs/backend-api.md) and [Lua API](docs/lua-api.md).
