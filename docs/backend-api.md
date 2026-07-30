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
| `/api/logs/ids` | `POST` bounded/filterable log ID query |
| `/api/logs/views` | `POST` batch incremental table-view rendering |
| `/api/logs/{id}` | `GET` complete log detail |
| `/api/session-view` | `GET`, `PUT` whole-session table view |
| `/api/column-scripts`, `/api/column-scripts/{name}` | script CRUD |
| `/api/interceptors`, `/api/interceptors/{kind}/{name}` | interceptor CRUD |
| `/api/interceptors/{kind}/{name}/enable` | `POST` |
| `/api/interceptors/{kind}/{name}/disable` | `POST` |
| `/api/interceptors/order` | `PUT` |
| `/api/filter-history` | `GET`, `POST`, `DELETE` |
| `/api/ca` | `GET`, `POST` to regenerate while the proxy is stopped |
| `/api/system-logs` | `GET`, `DELETE` |

System logs accept `after_seq` and are capped at 10,000 entries.

## Network log queries

All three log operations accept an optional Session. The POST operations accept `session_id` in their JSON body; the detail operation accepts `session_id` in its query string. Omitting it selects the currently active Session. If no active Session exists, the operation returns `409 conflict`.

### Query log IDs

`POST /api/logs/ids` accepts:

```json
{
  "session_id": 1,
  "filter": "entry.req().uri().host().contains(\"example.com\")",
  "min_id": 100,
  "max_id": 10000,
  "limit": 10000
}
```

Every field is optional. An absent or blank `filter` matches all logs. The filter uses the same Lua log object documented in the Lua API and must evaluate successfully for every scanned log. `min_id` and `max_id` are exclusive (`id > min_id && id < max_id`). The default and maximum page size are both 10,000.

When only `min_id` is supplied, the database scans toward newer IDs. When `max_id` or neither bound is supplied, it scans toward older IDs. Callers page in either direction by passing the relevant edge ID from their current list. Response order is intentionally unspecified:

```json
{
  "ids": [9999, 9998]
}
```

### Batch-render log views

`POST /api/logs/views` accepts at most 200 log items:

```json
{
  "session_id": 1,
  "logs": [
    { "id": 123, "updated_at": 1720000000000 },
    { "id": 124 }
  ],
  "view": {
    "columns": [
      { "kind": "method", "width": 50.0 },
      { "kind": "script", "width": 120.0, "script_name": "host" }
    ]
  }
}
```

`view` is optional and defaults to the selected Session's view. Duplicate IDs are deduplicated using the last supplied timestamp. A row is returned only when no timestamp was supplied or the stored `updated_at` is strictly greater than the supplied timestamp. IDs and response rows are unordered.

The response columns and cells do not contain the ID column; `row.id` is a separate field which the frontend displays as its fixed first column:

```json
{
  "columns": [
    { "key": "method", "name": "method", "kind": "method", "width": 50.0 }
  ],
  "rows": [
    {
      "id": 124,
      "updated_at": 1720000000100,
      "cells": ["GET"]
    }
  ],
  "exceptions": [
    {
      "id": 123,
      "column_index": 1,
      "code": "column_script_error",
      "message": "script execution failed"
    },
    {
      "id": 125,
      "code": "log_not_found",
      "message": "log 125 not found"
    }
  ]
}
```

`column_index` is zero-based and aligns with `columns` and `cells`. A custom-column error leaves that cell empty while preserving the rest of the row. A missing log has no row and no `column_index`. Unchanged logs appear in neither `rows` nor `exceptions`.

`updated_at` is a monotonic Unix-millisecond version for the complete log. Metadata transitions and request/response body writes advance it, even when multiple updates happen in the same wall-clock millisecond.

### Read one complete log

`GET /api/logs/{id}?session_id=1` returns request/response metadata and bodies, modifications, error/outcome state, plus `created_at` and `updated_at`.

## Session table views

`GET /api/session-view?session_id=1` returns:

```json
{
  "session_id": 1,
  "columns": [
    { "kind": "method", "width": 50.0 },
    { "kind": "uri", "width": 300.0 }
  ]
}
```

`PUT /api/session-view?session_id=1` atomically replaces the complete view:

```json
{
  "columns": [
    { "kind": "method", "width": 50.0 },
    { "kind": "script", "width": 120.0, "script_name": "host" }
  ]
}
```

The ID column is not part of the view model. Column widths must be positive. A new/updated view cannot reference a missing global column script. Deleting a referenced script is allowed and deliberately leaves the stale reference; rendering then returns `column_script_error`. Renaming a script updates references in every Session view.

New Sessions clone the active Session's view. When no active/readable view exists, they use the fixed built-in method, URI, status-code, and source columns.

## Capture lifecycle

An ordinary HTTP request is inserted into its active Session database before it is forwarded. The Session is pinned for the entire request/response lifecycle even if another Session becomes active.

Capture outcomes are:

- `in_progress`
- `success`
- `failed`
- `tunneled`

Errors contain a stable `kind`, an execution `stage`, and the underlying message. A provisional `CONNECT` row is inserted before the proxy acknowledges the tunnel, so even an idle connection is represented. TLS failures update that row. After a successful TLS handshake, the row is retained and finalized as `success` at stage `tls_mitm` with an HTTP 200 response; decrypted requests are stored as additional captures.

If the initial database insert fails, traffic is not forwarded. When the proxy stops it stops accepting immediately, waits up to five seconds, and marks unfinished rows with `proxy_shutdown`.

Request and response bodies are bounded to 64 MiB with a 60-second read timeout. The proxy allows at most four exchanges to materialize bodies concurrently and at most 256 client connections, preventing many clients from multiplying per-request resource bounds without limit. Lua file body replacements use the same size bound.

## Workspace and CA

The workspace is locked exclusively for the process lifetime. Active Session ID, proxy/API addresses, interceptor order, and filter history are stored in workspace configuration. Each Session stores its table view in `sessions/<id>/view.json`. Deleting the active Session returns `active_session_delete_forbidden`.

Each workspace has an independent generated CA. Missing or corrupt CA files are regenerated with a warning, the per-host certificate cache is bounded, and the CA private key is restricted to owner-only permissions on macOS/Unix. Regeneration is serialized with proxy start/stop and rejected while the proxy is running.

## System logs

The unified in-memory ring stores only:

- `seq`
- `timestamp`
- `level`
- `message`

Entries are not persisted and are consumed through incremental polling.
