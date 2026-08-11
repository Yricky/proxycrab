# ProxyCrab management HTTP API

This reference documents the complete local HTTP surface used by ProxyCrab agents.

## Contents

1. [Connection, authentication, and envelopes](#connection-authentication-and-envelopes)
2. [Agent instructions](#agent-instructions)
3. [Shared data types](#shared-data-types)
4. [Workspace and configuration](#workspace-and-configuration)
5. [Proxy lifecycle](#proxy-lifecycle)
6. [Sessions](#sessions)
7. [Capture logs](#capture-logs)
8. [Session views](#session-views)
9. [Column, filter, and routing scripts](#column-filter-and-routing-scripts)
10. [Workspace Assets](#workspace-assets)
11. [Interceptors](#interceptors)
12. [Active breakpoints](#active-breakpoints)
13. [Bypass traffic](#bypass-traffic)
14. [Certificate authority](#certificate-authority)
15. [System logs](#system-logs)
16. [Errors and lifecycle notes](#errors-and-lifecycle-notes)

## Connection, authentication, and envelopes

Default base URL:

```text
http://127.0.0.1:18089
```

The desktop app starts the management service on loopback only. Requests with no `Authorization`
header use the independently configurable `本机无 API Key` identity. API-key requests must send
exactly one `Authorization: Bearer <key>` header; query credentials and `X-API-Key` are unsupported.
Bundled scripts read the key from `PROXYCRAB_API_KEY`, never a command argument.

Every method and route template has an `allow`, `approval`, or `deny` permission for each identity.
Approval holds the original request for up to 30 seconds while the desktop window asks the user. A
decision can apply once or for 5, 30, or 60 minutes to the same identity and route action. Temporary
decisions are in-memory and disappear on restart. API-key creation, deletion, and permission editing
are desktop-only operations under Settings > 管理接口; they are not HTTP resources.

The server requires a loopback/`localhost` Host and accepts browser Origin values only from local
`http`, `https`, or `tauri` origins. Valid local CORS preflight permits `Authorization` and
`Content-Type`; remote Host or Origin values remain forbidden.

Except for the raw AGENTS.md, body, and Asset download endpoints documented below, every success is:

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

- Session and active-Session changes refresh the sidebar without forcing the user to view another
  Session.
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

## Agent instructions

### `GET /api/agents.md`

Read this endpoint immediately after reading the ProxyCrab Skill and before calling any other
ProxyCrab API. It returns the current workspace's active AGENTS.md preset as an unwrapped UTF-8 body
with `Content-Type: text/plain; charset=utf-8`.

The user's current explicit request takes precedence over the returned document. If the endpoint is
unavailable, use the Skill's conservative built-in behavior. This read is stateless and does not
notify or refresh the desktop UI.

The desktop app manages multiple workspace-scoped presets under:

```text
agents/config.json
agents/presets/<preset-id>.md
```

Newly initialized workspaces contain `充分使用能力` and `静默排查`, with `充分使用能力` active.
Preset CRUD and selection are desktop management operations, not public HTTP endpoints.

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
  "routing_script_name": "route-by-host",
  "active_session_id": 3
}
```

`routing_script_name` and `active_session_id` can be `null`. A non-null active Session ID must
exist. The management API host must remain `127.0.0.1` or `::1`. Changing an API address affects a
later service start, not the already-bound listener.

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
    "regex": false
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
{ "type": "text", "content": "plain text", "size": 10, "path": "/workspace/sessions/3/blob/42-request.body" }
{ "type": "json", "content": { "key": "value" }, "size": 15, "path": "/workspace/sessions/3/blob/42-response.body" }
{ "type": "binary", "size": 2048, "path": "/workspace/sessions/3/blob/42-response.body" }
{ "type": "large", "size": 73400320, "path": "/workspace/sessions/3/blob/42-response.body" }
```

Text/JSON content is embedded only when its decoded representation is at most 64 KiB. `size` and
`path` describe stored bytes, which may still be content-encoded. Active breakpoint string
replacements use `path: null` until persisted. Retrieve body bytes through the raw endpoints below.

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
preserve fields that should not change. A non-existent `routing_script_name` or
`active_session_id` returns `not_found`.

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
chains. The first Session created in an empty workspace becomes active; later Session creation does
not change the active Session.

### `PUT /api/sessions/{id}`

```json
{
  "name": "new-name",
  "description": null
}
```

All fields are optional. Omitting `description` preserves it; explicit `null` clears it. Returns the
updated `SessionMetadata`.

### `POST /api/sessions/{id}/archive`

Moves an inactive Session and all of its data to `sessions_archived/<id>` and returns its
`SessionMetadata`. The active Session cannot be archived. A Session with pinned in-progress
requests returns `409 conflict`; archive is otherwise allowed while the proxy runs. Archived
Sessions are excluded from every existing Session, log, view, interceptor, and export endpoint.

### `GET /api/archived-sessions`

Returns archived `SessionMetadata[]`, ordered by creation time.

### `POST /api/archived-sessions/{id}/restore`

Moves the archived directory back to `sessions/<id>` and returns its unchanged `SessionMetadata`.
Restore does not make the Session active.

### `DELETE /api/archived-sessions/{id}`

Permanently deletes an archived Session and returns `{}`. This is the only Session deletion API;
`DELETE /api/sessions/{id}` does not exist.

### `GET /api/active-session`

Returns `{ "session_id": 3 }` or `{ "session_id": null }`.

### `PUT /api/active-session`

Accepts and returns the same shape. A non-null Session ID must exist. Changing or clearing the
active Session is allowed while the proxy runs. An actual change closes every established proxy
connection, CONNECT tunnel, Upgrade/WebSocket, and upstream pooled connection before returning;
the listener remains running. Setting the current value again is a no-op for connections.

## Capture logs

Log endpoints accept an optional Session. POST requests use `session_id` in JSON; detail/body
requests use the query string. Omitting it selects the active Session. If no Session is active, the
response is `409 conflict`.

### `POST /api/logs/ids`

Request:

```json
{
  "session_id": 3,
  "filter": {
    "option": {
      "kind": "column",
      "column": { "kind": "uri" },
      "regex": false
    },
    "input": "/api/orders"
  },
  "min_id": 100,
  "max_id": 10000,
  "limit": 100,
  "persist_filter": false
}
```

Every field is optional.

- Omitting `filter` reuses the Session's persisted filter.
- Supplying `filter` applies it and persists it only after the scan succeeds. Set
  `persist_filter: false` to use it for this query without changing the Session view or notifying
  the desktop UI. Omitting `persist_filter` preserves the compatible default of `true`.
- `option: null` or an empty `input` matches all captures; an empty input still preserves the
  selected option.
- With `regex: false`, built-in and custom-column filters use case-sensitive contains matching.
- With `regex: true`, input uses Rust `regex` syntax and substring matching unless anchored. Inline
  flags such as `(?i)` control case folding; invalid patterns return `bad_request`.
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
      "regex": false
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
    { "kind": "method", "width": 50.0 }
  ],
  "rows": [
    {
      "id": 1043,
      "updated_at": 1785380000100,
      "outcome": "success",
      "cells": ["GET"]
    }
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

The ID and capture `outcome` (`in_progress`, `success`, `failed`, or `tunneled`) are separate from
cells. `column_index` is zero-based. A column error leaves that cell empty; a missing log has no row
or `column_index`. IDs and rows are unordered. Unchanged logs appear in neither `rows` nor
`exceptions`.

### `POST /api/logs/export`

Exports HAR 1.2:

```json
{
  "format": "har",
  "session_id": 3,
  "log_ids": [1041, 1042]
}
```

- `format` is required and currently only accepts `har`. Unsupported values return
  `unsupported_export_format`; unknown fields return `bad_request`.
- Omit `session_id` to select the active Session.
- Omit `log_ids` to snapshot and export all eligible captures currently in the Session. Captures
  added after the snapshot are not included.
- An empty `log_ids` array returns a valid empty HAR.
- Explicit IDs are deduplicated and entries are ordered by ascending log ID. Any missing explicit
  ID fails the complete request with `404 log_not_found`; existing but ineligible IDs are skipped.
- Eligibility requires `outcome: success` and a response. The synthetic `CONNECT / tls_mitm`
  record is excluded; ordinary HTTP and `101` upgrade handshakes are included.

The response is raw UTF-8 JSON rather than the standard API envelope:

```text
Content-Type: application/json; charset=utf-8
Content-Disposition: attachment; filename="proxycrab-session-3.har"
```

ProxyCrab reads and validates the complete export before returning HTTP 200. There is no configured
body/export size limit; a storage error fails the whole request with HTTP 500.

Bodies are complete. gzip, br, deflate, zstd, and stacked encodings are decoded. Text/JSON is
stored as text, binary response content uses standard `encoding: "base64"`, and binary request
`postData` uses `_encoding: "base64"`. If decoding is unsupported or malformed, raw bytes are
Base64-encoded and the entry's `_proxyCrab.requestBodyDecoded` or `responseBodyDecoded` is `false`.
HAR body sizes retain the stored/compressed byte count while response `content.size` is the
decoded/fallback byte count.

The exporter parses repeated query parameters, Cookie/Set-Cookie, and redirects on a best-effort
basis while preserving every stored header. `headersSize` and all detailed timing fields are `-1`;
entry `time` is `updated_at - created_at`. `_proxyCrab` also includes log and Session IDs, request
tags, stage, client source address, `createdAt`, and `updatedAt`. Interceptor source/history is not
included.

**Sensitive data:** nothing is redacted. Authorization, Cookie, Set-Cookie, and full body data may
contain credentials or personal information.

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
    "tags": { "environment": "staging" },
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
  "execution_id": 17,
  "origin": "saved",
  "completed": true,
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

Other modification variants are `method_set` with `method`, `uri_set` with `uri`, `status_set` with
`status`, `header_append`, `header_remove` with `values`, `body_replace_string` with `content`,
`body_replace_file` with `path`, and `tag_set` with `key` and `value`. Multiple temporary executions
may share the same phase and position; use `execution_id` and array order rather than treating
position as unique.

### `GET /api/logs/{id}/body?session_id=3&side=request&decompress=false&max_size=16777216`

Returns an unwrapped raw byte stream. `side` is required and must be `request` or `response`.
`decompress` defaults to `false`.

With `decompress=false`, ProxyCrab streams the stored bytes without decoding, preserves the captured
`Content-Encoding`, and limits the stored/compressed size. `max_size` defaults to 16 MiB and has no
server maximum. Oversized bodies return 413 with the stored sizes:

```json
{
  "ok": false,
  "error": {
    "code": "body_too_large",
    "message": "body size 20971520 exceeds the requested 16777216 byte limit",
    "actual_size": 20971520,
    "max_size": 16777216
  }
}
```

Browsers and Node fetch normally decode the preserved `Content-Encoding` themselves without using
ProxyCrab CPU. With `decompress=true`, `max_size` is forbidden and ProxyCrab performs one streaming
decode pass for gzip, br, deflate, zstd, or stacked encodings. That response omits both
`Content-Encoding` and `Content-Length`; a malformed stream can terminate after HTTP 200 has begun.
`X-ProxyCrab-Body-Size` always reports the stored byte count. Empty body files return 200 with zero
bytes. Missing/not-yet-produced bodies return `body_not_found`; unreadable files return
`body_read_failed`; unsupported encodings requested with decompression return `body_decode_failed`.

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
selection. Changing the selection, editing the selected script to different content, or deleting
the selected script resets all established downstream and upstream connections before returning.
Updating an unselected script or saving identical selected content does not. There is no rename or
routing-debug endpoint.

## Workspace Assets

Assets are immutable files shared across the active workspace. They have no list, update, or delete
endpoint. Upload with a raw request body:

```text
POST /api/assets/fixtures/example.json
Content-Type: application/json

{"fixture":true}
```

The success status is 201 and the normal envelope contains:

```json
{
  "id": "fixtures/example.json",
  "size": 16,
  "content_type": "application/json",
  "sha256": "lowercase SHA-256 hex",
  "created_at": 1785380000000
}
```

`GET /api/assets/{id}` returns that metadata. `GET /api/assets/{id}?format=raw` streams raw bytes
without an envelope and returns `Content-Type`, `Content-Length`, `Content-Disposition`, and
`X-ProxyCrab-Asset-SHA256`. If upload `Content-Type` is absent it defaults to
`application/octet-stream`.

IDs allow `[a-z0-9_./]`, up to 255 bytes total and 100 bytes per segment. They cannot start/end with
`/`, contain `//`, or use `.`, `..`, or `.metadata` as a complete segment. Errors are
`invalid_asset_id` (400), `invalid_asset_format` (400), `asset_not_found` (404),
`asset_already_exists` (409), `asset_path_conflict` (409), and `asset_store_failed` (500). Uploads
never overwrite existing Assets. SHA-256 is calculated during upload; later manual workspace file
changes are outside the API contract.

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

## Active breakpoints

### `GET /api/breakpoints`

Lists active breakpoints in the selected/active Session. Optional query parameters are
`session_id`, `phase=request|response`, and `interceptor_name`. A summary contains `id`, Session and
capture IDs, phase/position/name, method/URI, timestamps, and `remaining_ms`.

### `GET /api/breakpoints/{id}`

Returns `{ "breakpoint": <summary>, "log": <live LogDetail> }`. The log reflects mutations up to
the current paused point.

### `GET /api/breakpoints/{id}/body?side=request&decompress=false&max_size=16777216`

Uses the same raw response and query rules as the log body endpoint. It reads the persisted original
body unless the paused phase has a live replacement. String replacements come from breakpoint
memory; Asset replacements are read from their immutable workspace file. The result is a
point-in-time snapshot.

### Breakpoint controls

```text
POST /api/breakpoints/{id}/extend   body: {"timeout_ms":60000}
POST /api/breakpoints/{id}/execute  body: {"content":"req:set_tag('debug','1')"}
POST /api/breakpoints/{id}/release  no body
```

Extension is cumulative and silently clips total requested wait to 1,800,000 ms. Execute keeps the
request paused and returns the independent temporary execution plus refreshed breakpoint summary;
Lua errors are returned in `execution.error` after partial mutations/history are persisted.
Breakpoints auto-release at timeout and are not persisted across process restarts.

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
      "reason": "script_bypass",
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
| `invalid_api_key` | 401 | Invalid Bearer syntax, or an unknown/deleted API key |
| `forbidden_origin` | 403 | Non-local browser Origin |
| `permission_denied` | 403 | The identity or an approval decision denied this route action |
| `approval_timeout` | 403 | No desktop decision arrived within 30 seconds |
| `not_found` | 404 | Missing endpoint, Session, log, or script |
| `log_not_found` | 404 | Missing capture for a body request |
| `body_not_found` | 404 | Requested side has not produced a body file |
| `conflict` | 409 | State conflict, including no active Session or archiving an active/in-use Session |
| `body_too_large` | 413 | Stored body exceeds `max_size`; includes both sizes |
| `body_decode_failed` | 422 | Unsupported encoding when server decompression is requested |
| `internal_error` | 500 | Storage, runtime, I/O, or other internal failure |
| `permission_check_failed` | 500 | Permission storage or approval infrastructure failed closed |
| `body_read_failed` | 500 | Body file cannot be opened or read |

Routing and capture lifecycle details:

- With no selected routing script, the active Session captures; if none is active, traffic bypasses.
- Routing runs once per direct HTTP request and once per CONNECT tunnel.
- A routing script returns `true` to capture into the active Session and `false` or `nil` to bypass.
  Returning `true` while no Session is active also bypasses.
- Invalid returns and runtime errors emit a system warning and bypass.
- A routed normal request is inserted before forwarding. If insertion fails, traffic is not
  forwarded.
- A bypassed CONNECT is transparently tunneled without TLS decryption.
- Request/response bodies stream to capture files without an application-level size limit.
- Interceptors execute once at the request/response header boundary. Body getters may wait for the
  complete original body and then switch the normal forwarding path to disk replay.
- `_crab_resp_bodyframe_timeout` optionally bounds idle milliseconds before each upstream response
  body frame; normal responses fail on expiry, while replacement responses retain success and record
  the raw-drain timeout as diagnostic metadata.
- ProxyCrab allows at most 256 client connections.
- Capture/bypass queries and management Lua evaluations allow at most eight concurrent tasks;
  additional tasks wait for capacity.
- CONNECT creates a provisional capture before the tunnel is acknowledged.
- A TLS failure updates that CONNECT capture.
- Successful TLS MITM retains the CONNECT row as a successful `tls_mitm` capture and stores decrypted
  HTTP requests as additional captures.
- A captured request whose final request-interceptor tag is exactly `_crab_tls_insecure=true` skips
  upstream HTTPS certificate-chain and hostname verification. The tag is persisted but never sent as
  a header; verified and insecure requests use isolated connection pools. Restrict it to controlled
  test hosts.
- `updated_at` is a monotonic Unix-millisecond version of the complete capture. Metadata and body
  writes advance it even within the same wall-clock millisecond.
- Interceptor changes made before a runtime error remain applied and are recorded.
