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
8. [Column, filter, and routing scripts](#column-filter-and-routing-scripts)
9. [Interceptors](#interceptors)
10. [Bypass traffic](#bypass-traffic)
11. [Certificate authority](#certificate-authority)
12. [System logs](#system-logs)
13. [Errors and lifecycle notes](#errors-and-lifecycle-notes)

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

- Session changes refresh the sidebar and tags without forcing the user to view another Session.
- Routing-library and selection changes refresh the routing manager.
- Bypass deletion and clearing refresh the bypass window.
- A changed log filter or Session view reloads the table only when that Session is currently viewed.
- Column/filter/interceptor library changes refresh relevant choices, managers, table rendering, and
  the current interceptor pipeline.
- CA regeneration and system-log clearing refresh their open windows.
- Workspace/config changes refresh a clean settings window.
- Open script/settings windows preserve unsaved local input and show an external-change warning
  instead of overwriting it.
- Adjacent changes are coalesced and normal synchronization is silent.

This means Agent operations become visible in the desktop app shortly after the HTTP success
response. Do not assume the user wants their currently viewed Session changed.

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
  "routing_script_name": "route-by-host"
}
```

`routing_script_name` can be `null`. The management API host must remain `127.0.0.1` or `::1`.
Changing an API address affects a later service start, not the already-bound listener.

### SessionMetadata

```json
{
  "id": 3,
  "name": "checkout-debug",
  "created_at": 1785380000000,
  "description": "Capture checkout failures",
  "tags": ["checkout", "default"]
}
```

`description` can be `null`. Tags are globally unique, stored sorted, and must match
`^[a-z0-9_]{1,64}$`. Timestamps are Unix milliseconds.

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
preserve fields that should not change. A non-existent `routing_script_name` returns `not_found`.

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
chains. The first manually created Session in an empty workspace receives the `default` tag.

### `PUT /api/sessions/{id}`

```json
{
  "name": "new-name",
  "description": null,
  "tags": ["checkout", "default"]
}
```

All fields are optional. Omitting `description` preserves it; explicit `null` clears it. Supplying
`tags` replaces the complete tag set. A tag already bound elsewhere is atomically moved from the
old Session. Returns the updated `SessionMetadata`.

### `DELETE /api/sessions/{id}`

Returns `{}`. Session deletion is rejected while the proxy is starting, running, or stopping.
Deletion is allowed while stopped or failed. A Session with pinned in-progress requests cannot be
deleted.

## Capture logs

All log endpoints accept an optional Session. POST requests use `session_id` in JSON; the detail
endpoint uses the query string. Omitting it selects the Session tagged `default`. If no Session owns
that tag, the response is `409 conflict`.

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

A new view cannot reference a missing custom-column script. Deleting a referenced custom-column
script removes matching table columns and resets filters that reference it.

## Column, filter, and routing scripts

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

`content` defaults to an empty string when omitted. Update requires `{ "content": "..." }`.
Script names cannot be changed. Successful create/update/delete returns `{}`.

### Filter endpoints

CRUD shapes and responses match column scripts:

```text
GET    /api/filter-scripts
POST   /api/filter-scripts
GET    /api/filter-scripts/{name}
PUT    /api/filter-scripts/{name}
DELETE /api/filter-scripts/{name}
```

Deleting a referenced filter or custom-column filter resets those Session filters to no selection.

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

### Routing endpoints

```text
GET    /api/routing-scripts
POST   /api/routing-scripts
GET    /api/routing-scripts/{name}
PUT    /api/routing-scripts/{name}
DELETE /api/routing-scripts/{name}
GET    /api/routing-script-selection
PUT    /api/routing-script-selection
```

Routing script CRUD uses the same create and content-only update shapes. Selection is nullable:

```json
{ "name": "route-by-host" }
```

Use `{ "name": null }` to clear it. Deleting the selected routing script also clears the
selection. There is no rename or routing-debug endpoint.

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

Update requires `{ "content": "..." }`. Script names cannot be changed. Successful update/delete
returns `{}`. App-driven deletion removes all Session references; deleting a file directly can
leave an invalid reference.

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

## Bypass traffic

Bypass metadata is persisted in `<workspace>/bypass.db`. Headers and bodies are never stored.

### `GET /api/bypass?before_id=100&limit=200`

Returns newest-first pagination:

```json
{
  "rows": [
    {
      "id": 99,
      "created_at": 1785380000000,
      "updated_at": 1785380000100,
      "source": "127.0.0.1:54000",
      "method": "CONNECT",
      "uri": "example.com:443",
      "version": "HTTP/1.1",
      "reason": "script_nil",
      "outcome": "success",
      "response_status": null,
      "error": null,
      "upload_bytes": 1200,
      "download_bytes": 4300
    }
  ],
  "has_more": false
}
```

Default limit is 200; valid range is 1–1000. Outcomes are `in_progress`, `success`, and `failed`.

```text
DELETE /api/bypass/{id}
POST   /api/bypass/delete      body: {"ids":[1,2,3]}
DELETE /api/bypass             clears all terminal entries
```

Single delete returns `{}`. Batch and clear return `{ "deleted": N }`. In-progress entries cannot
be deleted; clearing skips them. Proxy shutdown marks unfinished rows failed with
`error: "proxy_shutdown"`.

## Certificate authority

### `GET /api/ca`

Returns:

```json
{ "pem": "-----BEGIN CERTIFICATE-----\n..." }
```

The same CA is available through the proxy at `http://proxy.crab/ca.crt`. This local URL bypasses
routing, Session capture, and bypass persistence.

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
| `conflict` | 409 | State conflict, including no `default` Session or deleting a Session while proxy runs |
| `proxy_running` | 409 | Session deletion attempted while proxy is starting/running/stopping |
| `session_in_use` | 409 | Tried to delete a Session pinned by active requests |
| `internal_error` | 500 | Storage, runtime, I/O, or other internal failure |

Routing and capture lifecycle details:

- With no selected routing script, a bound `default` tag captures; otherwise traffic bypasses.
- Routing runs once per direct HTTP request and once per CONNECT tunnel.
- A routing script returns a tag string or `nil`. `nil` bypasses. An explicit unbound tag creates
  exactly one Session named after that tag with a description naming the routing script.
- Invalid returns and runtime errors emit a system warning, then use a bound `default` Session or
  bypass when none exists.
- A routed normal request is inserted before forwarding. If insertion fails, traffic is not
  forwarded.
- A bypassed CONNECT is transparently tunneled without TLS decryption.
- Request/response bodies are bounded to 64 MiB with a 60-second read timeout.
- ProxyCrab allows at most four concurrent body-materializing exchanges and 256 client connections.
- CONNECT creates a provisional capture before the tunnel is acknowledged.
- A TLS failure updates that CONNECT capture.
- Successful TLS MITM retains the CONNECT row as a successful `tls_mitm` capture and stores decrypted
  HTTP requests as additional captures.
- `updated_at` is a monotonic Unix-millisecond version of the complete capture. Metadata and body
  writes advance it even within the same wall-clock millisecond.
- Interceptor changes made before a runtime error remain applied and are recorded.
