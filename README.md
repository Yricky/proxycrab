# ProxyCrab

ProxyCrab is a macOS-first HTTP/HTTPS MITM capture application. The same Vue management UI runs in the Tauri desktop application and from the headless CLI's loopback HTTP server through target-specific backends.

## Rust workspace

- `crates/proxy-crab-mitm`: proxy lifecycle, CA/TLS, sessions, SQLite captures, request/response bodies, Lua scripts, diagnostic failures, and the in-memory system-log ring.
- `crates/proxy-crab-mgr`: the object-safe `ProxyCrabManager` trait, its MITM adapter, typed DTOs, stable errors, the local management HTTP server, and a route-level permission contract.
- `src-tauri`: Tauri initialization, application lifecycle, and thin commands that call the same management trait as HTTP handlers.
- `cli-app`: headless proxy runner, embedded browser UI, per-run UI authentication, and CLI-specific allow/deny permission management.

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
per-route permissions. The desktop target supports `allow`, `approval`, and `deny`; approval waits
for a desktop decision for up to 30 seconds. The CLI target exposes only `allow` and `deny`, and
evaluates any persisted `approval` value as `deny`. API keys and permissions are managed under
Settings > 管理接口 through target-private operations, not the public agent API. Direct Tauri
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

## CLI browser UI

Build the CLI frontend before compiling the binary so Cargo can embed the generated assets:

```bash
pnpm build:cli
cargo build -p proxycrab-cli
```

Run the CLI with a workspace:

```bash
target/debug/proxycrab-cli run --workspace /path/to/workspace
```

At startup the CLI prints its browser URL and a 256-bit, per-run `pcrab_ui_…` Access Token. Open
the URL and enter that token on the landing page. The cleartext token is not persisted; the process
keeps only its SHA-256 digest. It authorizes the browser UI's internal `/ui-api/*` operations and
trusted calls to the shared `/api/*` management resources for that process lifetime.

The CLI browser backend deliberately has no ProxyCrab Skill installation capability or HTTP route.
This remains true even if the CLI frontend bundle is hosted outside the local binary. Users who
want to install the bundled Skill must explicitly run the local
`proxycrab-cli install-skill` command; a browser page cannot trigger it.

## Development

```bash
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
pnpm build:tauri
pnpm build:cli
```

See [backend API](docs/backend-api.md) and [Lua API](docs/lua-api.md).
