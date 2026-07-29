# ProxyCrab Backend API

## Shared management contract

HTTP handlers and Tauri commands call the same `ProxyCrabManager` trait and use the same request/response DTOs. HTTP successes use:

```json
{ "ok": true, "data": {} }
```

HTTP failures use:

```json
{
  "ok": false,
  "error": {
    "code": "not_found",
    "message": "session 1 not found"
  }
}
```

Tauri commands return the data DTO directly or reject with the same `{code,message}` error.

## HTTP resources

The server listens on loopback by default at `http://127.0.0.1:18089`. It has no authentication and emits no permissive CORS policy. Requests must use a loopback/`localhost` Host, and browser Origin values must also be local (including `tauri://localhost`) to prevent DNS-rebinding and CSRF access.

| Resource | Operations |
| --- | --- |
| `/api/workspace` | `GET` current/configured paths; `PUT` next-start path |
| `/api/config` | `GET`, `PUT` |
| `/api/proxy/status` | `GET` |
| `/api/proxy/start`, `/api/proxy/stop` | `POST` |
| `/api/sessions` | `GET`, `POST` |
| `/api/sessions/{id}` | `PUT`, `DELETE` |
| `/api/sessions/{id}/activate` | `POST` |
| `/api/sessions/{id}/logs` | `GET` |
| `/api/sessions/{id}/logs/{log_id}` | `GET` |
| `/api/sessions/{id}/logs/filter` | `POST` with Lua |
| `/api/logs`, `/api/logs/{id}`, `/api/logs/filter` | compatible active-session routes |
| `/api/column-scripts`, `/api/column-scripts/{name}` | script CRUD |
| `/api/columns`, `/api/columns/{index}` | visible-column CRUD |
| `/api/interceptors`, `/api/interceptors/{kind}/{name}` | interceptor CRUD |
| `/api/interceptors/{kind}/{name}/enable` | `POST` |
| `/api/interceptors/{kind}/{name}/disable` | `POST` |
| `/api/interceptors/order` | `PUT` |
| `/api/filter-history` | `GET`, `POST`, `DELETE` |
| `/api/ca` | `GET`, `POST` to regenerate while the proxy is stopped |
| `/api/system-logs` | `GET`, `DELETE` |

List routes accept `limit`; capture lists additionally accept `after_id`, and system logs accept `after_seq`. HTTP list limits are capped at 1,000 captures and 10,000 system-log entries.

## Capture lifecycle

An ordinary HTTP request is inserted into its active Session database before it is forwarded. The Session is pinned for the entire request/response lifecycle even if another Session becomes active.

Capture outcomes are:

- `in_progress`
- `success`
- `failed`
- `tunneled`

Errors contain a stable `kind`, an execution `stage`, and the underlying message. A provisional `CONNECT` row is inserted before the proxy acknowledges the tunnel, so even an idle connection is represented. TLS failures update that row; after a successful TLS handshake the provisional row is removed and only decrypted requests are stored, avoiding duplicate successful CONNECT rows.

If the initial database insert fails, traffic is not forwarded. When the proxy stops it stops accepting immediately, waits up to five seconds, and marks unfinished rows with `proxy_shutdown`.

Request and response bodies are bounded to 64 MiB with a 60-second read timeout. The proxy allows at most four exchanges to materialize bodies concurrently and at most 256 client connections, preventing many clients from multiplying per-request resource bounds without limit. Lua file body replacements use the same size bound.

## Workspace and CA

The workspace is locked exclusively for the process lifetime. Active Session ID, proxy/API addresses, interceptor order, columns, and filter history are stored in workspace configuration. Deleting the active Session returns `active_session_delete_forbidden`.

Each workspace has an independent generated CA. Missing or corrupt CA files are regenerated with a warning, the per-host certificate cache is bounded, and the CA private key is restricted to owner-only permissions on macOS/Unix. Regeneration is serialized with proxy start/stop and rejected while the proxy is running.

## System logs

The unified in-memory ring stores only:

- `seq`
- `timestamp`
- `level`
- `message`

Entries are not persisted and are consumed through incremental polling.
