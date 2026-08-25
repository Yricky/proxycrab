# ProxyCrab

ProxyCrab is a macOS-first HTTP/HTTPS MITM capture application. The same Vue management UI runs in the Tauri desktop application and from the headless CLI's HTTP server through target-specific backends.

## Rust workspace

- `crates/proxy-crab-mitm`: proxy lifecycle, CA/TLS, sessions, SQLite captures, request/response bodies, Lua scripts, diagnostic failures, and the in-memory system-log ring.
- `crates/proxy-crab-mgr`: the object-safe `ProxyCrabManager` trait, its MITM adapter, typed DTOs, stable errors, the local management HTTP server, and a route-level permission contract.
- `src-tauri`: Tauri initialization, application lifecycle, trusted host commands, and workspace-pointer ownership; ordinary proxy commands call the same management trait as HTTP handlers.
- `cli-app`: headless proxy runner, embedded browser UI, per-run UI authentication, and CLI-specific allow/deny permission management.

Neither reusable crate depends on Tauri.

## Runtime defaults

- MITM proxy: `0.0.0.0:8089`, stopped when the app launches.
- Management API: `0.0.0.0:18089`, started with the app. Remote management calls still require Bearer authentication.
- CA download through the proxy: `http://proxy.crab/ca.crt`.
- System log buffer: the newest 10,000 entries in memory.
- Captured request/response bodies stream directly to disk without an application-level size limit; at most 256 client connections are accepted.
- Blocking capture/bypass queries and management Lua evaluations: at most eight concurrently; excess work waits without occupying Tokio worker threads.

The Tauri application data directory contains `config.json`, which points at the workspace. When the pointer is absent or invalid, `app_data_dir/workspace` is used and persisted. A changed pointer takes effect only on the next application launch.

The management API listens on every IPv4 interface and explicitly authenticates each `/api/*`
request as either `LocalLoopback` or `Bearer`. `LocalLoopback` requires a loopback TCP peer, a local
Host, and no Origin or a local Origin. Exactly one `Authorization: Bearer <key>` header always selects
the Bearer identity, including on loopback. Every API key and the local no-key identity have independent
per-route permissions. The desktop target supports `allow`, `approval`, and `deny`; approval waits
for a desktop decision for up to 30 seconds. The CLI target exposes only `allow` and `deny`, and
evaluates any persisted `approval` value as `deny`. API keys and permissions are managed under
Settings > 管理接口 through target-private operations, not the public agent API. Direct Tauri
commands remain trusted and bypass HTTP authentication.

Host mutations such as changing the next-start workspace, replacing global configuration
(including the proxy port), regenerating the CA, and installing the bundled Skill are not management
HTTP resources. They are available only through trusted Tauri commands or explicit local CLI options
and subcommands. Workspace selection is not exposed through HTTP; the API retains read-only config
and CA resources.

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

## Read-only Session links

The desktop and CLI management UIs can create a link from a Session's context menu. Each action
creates a new 256-bit `pcrab_share_…` token scoped to that one active Session, stores only its
SHA-256 digest in process memory, and accepts an integer lifetime from 1 through 720 hours (24 by
default). Restarting ProxyCrab, expiry, or archiving the Session invalidates the link.

The `/session?token=…` page renders only the main traffic content. It follows new/finished captures
and owner column changes, starts with the Session's current filter, and keeps every viewer's later
filter stateless. Sorting, copying, log details, complete bodies, historical interceptor results,
and script snapshots remain readable. Toolbar/sidebar management, column editing, interceptors,
breakpoints, exports, and every other write surface are absent. Its token is accepted only by the
dedicated Session-scoped `/share-api/*` routes as exactly one `token` query parameter and is never an
Agent API key. These routes ignore `Authorization` and return `Cache-Control: no-store`; the share UI
uses a no-referrer policy.

The UI generates one URL for every non-loopback local IPv4 address. These links use plain HTTP by
default, so the token and captured data must be shared only on a trusted network or protected by a
TLS reverse proxy.

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

For access from another machine, the safer setup is an SSH local port forward or authenticated TLS.
The browser can then open `http://127.0.0.1:18089` on the client. Direct access uses one of the
server's reachable IP addresses. Remote `/api/*` requests require the Bearer token, and
the token travels in cleartext over plain HTTP, so direct exposure should be limited to a trusted
network or placed behind authenticated TLS.

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
