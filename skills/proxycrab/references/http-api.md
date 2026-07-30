# ProxyCrab management HTTP API

This reference documents the complete local HTTP surface used by ProxyCrab agents.

## Contents

1. [Connection and envelopes](#connection-and-envelopes)
2. [Shared data types](#shared-data-types)
3. [Workspace and configuration](#workspace-and-configuration)
4. [Proxy lifecycle](#proxy-lifecycle)
5. [Sessions](#sessions)
6. [Capture logs](#capture-logs)
7. [Session views](#session-views)
8. [Column and filter scripts](#column-and-filter-scripts)
9. [Interceptors](#interceptors)
10. [Certificate authority](#certificate-authority)
11. [System logs](#system-logs)
12. [Errors and lifecycle notes](#errors-and-lifecycle-notes)

## Connection and envelopes

Default base URL:

```text
http://127.0.0.1:18089
```

The desktop app starts the management service. It has no authentication, binds to loopback, emits
no permissive CORS policy, requires a loopback/`localhost` Host, and accepts browser Origin values
only from local `http`, `https`, or `tauri` origins.

Every success is:

```json
{
  "ok": true,
  "data": {}
}
```

Every failure is:

```json
{
  "ok": false,
  "error": {
    "code": "not_found",
    "message": "session 1 not found"
  }
}
```

Requests and responses use `application/json`. URL path segments and query values must be
percent-encoded normally.

### Desktop UI side effects

Successful HTTP mutations notify the running Tauri frontend through an internal event channel. The
HTTP envelope does not change and there is no public event endpoint.

- Session changes refresh the sidebar and active marker. Activating a Session does not force the
  user to view it.
- A changed log filter or Session view reloads the table only when that Session is currently viewed.
- Column/filter/interceptor library changes refresh relevant choices, managers, table rendering, and
  the current interceptor pipeline.
- CA regeneration and system-log clearing refresh their open windows.
- Workspace/config changes refresh a clean settings window.
- Open script/settings windows preserve unsaved local input and show an external-change warning
  instead of overwriting it.
- Adjacent changes are coalesced and normal synchronization is silent.

This means Agent operations become visible in the desktop app shortly after the HTTP success
response. Do not assume the user wants their current viewed Session changed merely because another
Session was activated.

## Shared data types

### WorkspacePaths

```json
{
  "current_path": "/absolute/current/workspace",
  "configured_path": "/absolute/next-start/workspace"
}
```

### AppConfig

```json
{
  "proxy_host": "0.0.0.0",
  "proxy_port": 8089,
  "api_host": "127.0.0.1",
  "api_port": 18089,
  "active_session_id": 3
}
```

`active_session_id` can be `null`. The management API host must remain `127.0.0.1` or `::1`.
Changing an API address affects a later service start, not the already-bound listener.

### SessionMetadata

```json
{
  "id": 3,
  "name": "checkout-debug",
  "created_at": 1785380000000,
  "description": "Capture checkout failures"
}
```

`description` can be `null`. Timestamps are Unix milliseconds.

### ProxyStatus

Exactly one of:

```json
{ "status": "stopped" }
{ "status": "starting" }
{ "status": "running", "host": "0.0.0.0", "port": 8089 }
{ "status": "stopping" }
{ "status": "failed", "message": "bind failed" }
```

### Script

```json
{
  "name": "server-error",
  "content": "return entry.resp ~= nil and entry.resp.status >= 500"
}
```

Script names map to `.lua` files and must satisfy ProxyCrab's filename validation. Source is
syntax-checked before save.

### SessionFilter

No filter:

```json
{ "option": null, "input": "" }
```

Built-in or custom-column contains filter:

```json
{
  "option": {
    "kind": "column",
    "column": { "kind": "uri" },
    "case_sensitive": false
  },
  "input": "example.com"
}
```

`column.kind` is `method`, `uri`, `code`, `source`, `stage`, or `script`. A script column adds
`"script_name": "name"`.

Lua filter:

```json
{
  "option": {
    "kind": "script",
    "script_name": "server-error"
  },
  "input": "example.com"
}
```

### Column

Built-in:

```json
{ "kind": "method", "width": 50.0 }
```

Kinds are `method`, `uri`, `code`, `source`, and `stage`. A custom column is:

```json
{ "kind": "script", "width": 120.0, "script_name": "correlation-id" }
```

Widths must be finite and positive.

### BodyPayload

Exactly one of:

```json
{ "type": "empty" }
{ "type": "text", "content": "plain text" }
{ "type": "json", "content": { "key": "value" } }
{ "type": "binary", "size": 2048 }
{ "type": "large", "size": 73400320 }
```

Raw bytes for `binary` and `large` are not available from this API.

### CaptureError

```json
{
  "stage": "upstream",
  "kind": "connection_refused",
  "message": "connection refused"
}
```

`stage` is `connect`, `tls_handshake`, `request_body`, `interceptor`, `upstream`, or
`response_body`. `kind` is stable enough for programmatic classification; `message` is diagnostic
text.

## Workspace and configuration

### `GET /api/workspace`

Returns `WorkspacePaths`.

### `PUT /api/workspace`

Sets the workspace used after the next application launch.

```json
{ "path": "/absolute/writable/workspace" }
```

The path must be absolute. The returned value is `WorkspacePaths`; `current_path` does not change in
the current process.

### `GET /api/config`

Returns `AppConfig`.

### `PUT /api/config`

Replaces the complete `AppConfig` and returns the stored value. Read the current config first and
preserve fields that should not change. A non-existent `active_session_id` returns `not_found`.

## Proxy lifecycle

### `GET /api/proxy/status`

Returns `ProxyStatus`.

### `POST /api/proxy/start`

Starts the MITM listener and returns `ProxyStatus`.

### `POST /api/proxy/stop`

Stops accepting immediately, waits up to five seconds for active exchanges, marks unfinished
captures with `proxy_shutdown`, and returns `ProxyStatus`.

Agents using this skill should leave lifecycle control to the user unless explicitly asked.

## Sessions

### `GET /api/sessions`

Returns `SessionMetadata[]`.

### `POST /api/sessions`

```json
{
  "name": "checkout-debug",
  "description": "Capture checkout failures"
}
```

Both fields are optional or `null`. ProxyCrab generates a name when omitted. Returns the created
`SessionMetadata`. New Sessions use the default table columns, empty filter, and empty interceptor
chains; they do not clone the active Session.

### `PUT /api/sessions/{id}`

```json
{
  "name": "new-name",
  "description": null
}
```

Both fields are optional. Omitting `description` preserves it; explicit `null` clears it. Returns
the updated `SessionMetadata`.

### `DELETE /api/sessions/{id}`

Returns `{}`. Deleting the active Session returns `active_session_delete_forbidden`. Deleting a
Session with pinned in-progress requests returns `session_in_use`.

### `POST /api/sessions/{id}/activate`

Sets the active Session and returns its `SessionMetadata`. New requests pin this Session for their
entire request/response lifecycle.

## Capture logs

All log endpoints accept an optional Session. POST requests use `session_id` in JSON; the detail
endpoint uses the query string. Omitting it selects the active Session. If none exists, the response
is `409 conflict`.

### `POST /api/logs/ids`

Request:

```json
{
  "session_id": 3,
  "filter": {
    "option": {
      "kind": "column",
      "column": { "kind": "uri" },
      "case_sensitive": false
    },
    "input": "/api/orders"
  },
  "min_id": 100,
  "max_id": 10000,
  "limit": 100
}
```

Every field is optional.

- Omitting `filter` reuses the Session's persisted filter.
- Supplying `filter` applies it and persists it only after the scan succeeds.
- `option: null` or an empty `input` matches all captures; an empty input still preserves the
  selected option.
- Built-in and custom-column filters use contains matching. `case_sensitive: false` performs Unicode
  lowercase matching.
- Lua filter input is passed exactly, including whitespace.
- Filter/custom-column errors count as non-matches in this endpoint.
- `min_id` and `max_id` are exclusive.
- Default and maximum `limit` are 10,000. `0` returns no IDs.
- Only `min_id` scans newer IDs. `max_id` or no bounds scans older IDs.
- Response ID order is intentionally unspecified.

Response:

```json
{
  "ids": [1042, 1041],
  "filter": {
    "option": {
      "kind": "column",
      "column": { "kind": "uri" },
      "case_sensitive": false
    },
    "input": "/api/orders"
  }
}
```

If a referenced filter script was removed outside the app, the Session filter is repaired to the
empty filter and returned that way.

### `POST /api/logs/views`

Batch-renders at most 200 table rows.

```json
{
  "session_id": 3,
  "logs": [
    { "id": 1042, "updated_at": 1785380000000 },
    { "id": 1043 }
  ],
  "view": {
    "columns": [
      { "kind": "method", "width": 50.0 },
      { "kind": "script", "width": 120.0, "script_name": "correlation-id" }
    ]
  }
}
```

`view` is optional and defaults to the Session view. Duplicate IDs are deduplicated using the last
timestamp. A row is returned only when no timestamp was supplied or stored `updated_at` is strictly
newer.

```json
{
  "columns": [
    { "key": "method", "name": "method", "kind": "method", "width": 50.0 }
  ],
  "rows": [
    { "id": 1043, "updated_at": 1785380000100, "cells": ["GET"] }
  ],
  "exceptions": [
    {
      "id": 1042,
      "column_index": 1,
      "code": "column_script_error",
      "message": "script execution failed"
    },
    {
      "id": 9999,
      "code": "log_not_found",
      "message": "log 9999 not found"
    }
  ]
}
```

The ID is separate from cells. `column_index` is zero-based. A column error leaves that cell empty;
a missing log has no row or `column_index`. IDs and rows are unordered. Unchanged logs appear in
neither `rows` nor `exceptions`.

### `GET /api/logs/{id}?session_id=3`

Returns:

```json
{
  "id": 1042,
  "session_id": 3,
  "created_at": 1785380000000,
  "updated_at": 1785380000100,
  "source_type": "ip",
  "source_addr": "127.0.0.1:54000",
  "stage": "complete",
  "outcome": "success",
  "error": null,
  "request": {
    "method": "POST",
    "uri": "https://example.com/api/orders",
    "version": "HTTP/1.1",
    "headers": [{ "name": "content-type", "value": "application/json" }],
    "body": { "type": "json", "content": { "sku": "A-1" } }
  },
  "response": {
    "status": 500,
    "status_text": "Internal Server Error",
    "version": "HTTP/1.1",
    "headers": [{ "name": "content-type", "value": "application/json" }],
    "body": { "type": "json", "content": { "error": "failed" } }
  },
  "request_interceptors": [],
  "response_interceptors": []
}
```

`outcome` is `in_progress`, `success`, `failed`, or `tunneled`. `response` and `source_addr` can be
`null`.

Each interceptor execution is:

```json
{
  "phase": "request",
  "position": 0,
  "name": "add-debug-header",
  "script_hash": "lowercase-sha256",
  "content": "req.headers:set(\"x-debug\", \"1\")",
  "modifications": [
    { "kind": "snapshot", "headers": { "host": ["example.com"] } },
    { "kind": "header_set", "name": "x-debug", "value": "1" }
  ],
  "error": null
}
```

Other modification variants are `header_append`, `header_remove` with `values`,
`body_replace_string` with `content`, and `body_replace_file` with `path`.

## Session views

### `GET /api/session-view?session_id=3`

```json
{
  "session_id": 3,
  "columns": [
    { "kind": "method", "width": 50.0 },
    { "kind": "uri", "width": 300.0 }
  ],
  "filter": { "option": null, "input": "" }
}
```

### `PUT /api/session-view?session_id=3`

Atomically replaces all columns and preserves the current filter:

```json
{
  "columns": [
    { "kind": "method", "width": 50.0 },
    { "kind": "script", "width": 120.0, "script_name": "correlation-id" }
  ]
}
```

A new view cannot reference a missing custom-column script. Deleting a referenced script later is
allowed and rendering then returns `column_script_error`. Renaming a script updates Session view and
filter references.

## Column and filter scripts

### Column endpoints

```text
GET    /api/column-scripts
POST   /api/column-scripts
GET    /api/column-scripts/{name}
PUT    /api/column-scripts/{name}
DELETE /api/column-scripts/{name}
```

List returns `Script[]`; detail returns `Script`.

Create:

```json
{ "name": "correlation-id", "content": "return entry.req.headers:get(\"x-request-id\")" }
```

`content` defaults to an empty string when omitted. Update accepts optional `name` and `content`; an
omitted field remains unchanged. Successful create/update/delete returns `{}`.

### Filter endpoints

CRUD shapes and responses match column scripts:

```text
GET    /api/filter-scripts
POST   /api/filter-scripts
GET    /api/filter-scripts/{name}
PUT    /api/filter-scripts/{name}
DELETE /api/filter-scripts/{name}
```

Renaming updates every Session filter reference. Deleting a referenced filter or custom-column
filter resets those Session filters to no selection.

### `POST /api/filter-scripts/{name}/debug`

```json
{
  "session_id": 3,
  "log_id": 1042,
  "input": "example.com"
}
```

`session_id` is optional. Returns a boolean. Runtime errors and non-boolean results return
`bad_request`, unlike normal list filtering where they count as non-matches.

## Interceptors

### `GET /api/interceptors?kind=request`

`kind` is `request` or `response`. Returns:

```json
{
  "kind": "request",
  "items": [
    { "name": "add-debug-header", "usage_count": 2 }
  ]
}
```

### `POST /api/interceptors`

```json
{
  "kind": "request",
  "name": "add-debug-header",
  "content": "req.headers:set(\"x-debug\", \"1\")"
}
```

`content` defaults to an empty string. Returns `{}`.

### Interceptor detail, update, delete

```text
GET    /api/interceptors/{kind}/{name}
PUT    /api/interceptors/{kind}/{name}
DELETE /api/interceptors/{kind}/{name}
```

Detail returns:

```json
{
  "kind": "request",
  "name": "add-debug-header",
  "content": "req.headers:set(\"x-debug\", \"1\")"
}
```

Update accepts optional `name` and `content`. Successful update/delete returns `{}`. Renaming
updates all Session references. App-driven deletion removes all Session references; deleting a file
directly can leave an invalid reference.

### `GET /api/session-interceptors?session_id=3`

```json
{
  "session_id": 3,
  "request": [
    { "name": "add-debug-header", "enabled": true, "valid": true }
  ],
  "response": [
    { "name": "missing-script", "enabled": true, "valid": false }
  ]
}
```

### `PUT /api/session-interceptors?session_id=3`

Atomically replaces both chains:

```json
{
  "request": [
    { "name": "add-debug-header", "enabled": true }
  ],
  "response": []
}
```

Each chain allows at most 12 unique names. Missing names are accepted so externally changed files
remain editable. Returns the resolved payload including `valid`.

At request start, ProxyCrab pins the Session and snapshots the exact UTF-8 content of all enabled,
present scripts in both chains. The response phase uses that snapshot even if scripts or chains
change mid-flight. Missing and disabled nodes are skipped.

## Certificate authority

### `GET /api/ca`

Returns:

```json
{ "pem": "-----BEGIN CERTIFICATE-----\n..." }
```

The same CA is available through the proxy at `http://proxy.crab/ca.crt`.

### `POST /api/ca`

Regenerates the workspace CA and returns the new PEM. Regeneration is rejected while the proxy is
running. This is disruptive because clients must trust the new CA.

## System logs

### `GET /api/system-logs?after_seq=100&limit=1000`

Both query parameters are optional. Default limit is 1,000 and maximum is 10,000. Returns:

```json
[
  {
    "seq": 101,
    "timestamp": 1785380000000,
    "level": "ERROR",
    "message": "interceptor failed"
  }
]
```

The in-memory ring retains at most the newest 10,000 entries. Entries are not persisted.

### `DELETE /api/system-logs`

Clears the in-memory ring and returns `{}`.

## Errors and lifecycle notes

HTTP status mapping:

| Error code | HTTP status | Meaning |
| --- | ---: | --- |
| `bad_request` | 400 | Invalid JSON, arguments, script, filter, or operation |
| `forbidden_origin` | 403 | Non-local browser Origin |
| `not_found` | 404 | Missing endpoint, Session, log, or script |
| `conflict` | 409 | State conflict, including no active Session |
| `active_session_delete_forbidden` | 409 | Tried to delete the active Session |
| `session_in_use` | 409 | Tried to delete a Session pinned by active requests |
| `internal_error` | 500 | Storage, runtime, I/O, or other internal failure |

Capture lifecycle details:

- A normal request is inserted before forwarding. If insertion fails, traffic is not forwarded.
- Request/response bodies are bounded to 64 MiB with a 60-second read timeout.
- ProxyCrab allows at most four concurrent body-materializing exchanges and 256 client connections.
- CONNECT creates a provisional capture before the tunnel is acknowledged.
- A TLS failure updates that CONNECT capture.
- Successful TLS MITM retains the CONNECT row as a successful `tls_mitm` capture and stores decrypted
  HTTP requests as additional captures.
- `updated_at` is a monotonic Unix-millisecond version of the complete capture. Metadata and body
  writes advance it even within the same wall-clock millisecond.
- Interceptor changes made before a runtime error remain applied and are recorded.
