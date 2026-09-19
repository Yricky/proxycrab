# ProxyCrab management HTTP API

This reference documents the complete local HTTP surface used by ProxyCrab agents.

## Contents

1. [Connection, authentication, and envelopes](#connection-authentication-and-envelopes)
2. [Agent instructions](#agent-instructions)
3. [Shared data types](#shared-data-types)
4. [Configuration](#configuration)
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

The desktop app or CLI binds the management service to every IPv4 interface. Agents running on the
same machine should still use the loopback base URL above. Requests with no `Authorization` use
the independently configurable `本机无 API Key` identity only when the TCP peer, Host, and optional
Origin are all local. API-key requests must send
exactly one `Authorization: Bearer <key>` header; query credentials and `X-API-Key` are unsupported.
Bearer authentication always wins on loopback, and missing peer metadata fails closed.
Bundled scripts read the key from `PROXYCRAB_API_KEY`, never a command argument.

Every method and route template has a permission for each identity. The desktop target supports
`allow`, `approval`, and `deny`; approval holds the original request for up to 30 seconds while the
desktop window asks the user. A decision can apply once or for 5, 30, or 60 minutes to the same
identity and route action. Temporary decisions are in-memory and disappear on restart. The CLI
target supports only `allow` and `deny`, treats a persisted `approval` as deny, and never creates an
approval request. API-key creation, deletion, and permission editing are management-UI-only
operations under Settings > 管理接口; they are not public Agent HTTP resources.

The CLI also prints a per-run `pcrab_ui_…` token for its browser landing page. It authorizes private
UI routes and trusted UI calls, is not an Agent API key, and must never be copied into
`PROXYCRAB_API_KEY`.

Session sharing uses separate `pcrab_share_…` tokens and `/share-api/*` browser routes. Those tokens
are scoped to one read-only Session, live only for the current process while sharing remains
enabled, are not accepted by the Agent API, and must never be copied into `PROXYCRAB_API_KEY`, Agent
commands, logs, or reports.

HAR sharing uses an independent `pcrab_har_…` browser token for a frozen set of capture IDs. It is
also process-local, is accepted only by `/session.har`, and must not be copied into Agent commands,
logs, reports, or `Authorization`.

The service binds to every IPv4 interface. Public CLI UI and Session-share assets can load remotely,
but a remote Host or Origin on `/api/*` requires Bearer authorization. Remote CORS preflight is
accepted only when it requests `Authorization`; the subsequent request still passes normal token
authentication and route permissions. Bundled Agent scripts accept a remote `--base-url` or
`PROXYCRAB_API_URL` only when `PROXYCRAB_API_KEY` is set. Prefer authenticated TLS or an SSH local
port forward because plain HTTP exposes the Bearer key and captured data.

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

Successful HTTP mutations notify the running management frontend through a target-private channel.
The HTTP envelope does not change and there is no public Agent event endpoint.

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

This means Agent operations become visible in the open desktop or CLI browser UI shortly after the HTTP success
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
Preset CRUD and selection are management-UI operations, not public Agent HTTP endpoints.

## Shared data types

### AppConfig

```json
{
  "proxy_host": "0.0.0.0",
  "proxy_port": 8089,
  "api_port": 18089,
  "routing_script_name": "route-by-host",
  "active_session_id": 3
}
```

`routing_script_name` and `active_session_id` can be `null`. A non-null active Session ID must
exist. The management API listener is fixed to `0.0.0.0`; only `api_port` is configurable.

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
{
  "status": "running",
  "host": "0.0.0.0",
  "port": 8089,
  "started_at": 1785380000000,
  "active_netlog": { "1786333525006": [1, 2, 3, 4, 5] },
  "active_bypass_count": 2
}
{ "status": "stopping" }
{ "status": "failed", "message": "bind failed" }
```

`started_at` is a Unix-millisecond timestamp and is exposed only while the listener is running.
`active_netlog` maps Session IDs to exact active capture IDs and is the sole activity source for
netlogs. `active_bypass_count` is the total number of active transparent-forwarding records.
Persisted `outcome` and `stage` continue to represent result and execution phase; they do not imply
current activity.

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

Kinds are `method`, `uri`, `code`, `source`, `stage`, `created_at`, and `updated_at`. The time
columns are table-only and are not valid filter columns. A custom column is:

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
`path` describe the selected final source: original stored bytes may still be content-encoded,
string replacements use `path: null`, and Asset replacements use the Asset path. Retrieve complete
body bytes through the raw endpoints below. An empty original request or response is represented as
`empty` without creating a blob file. An empty string replacement is also `empty` and is stored in
capture metadata; ProxyCrab does not create `.modified` capture blobs. The raw body endpoint still
returns a successful zero-byte stream.

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

## Configuration

### `GET /api/config`

Returns `AppConfig`.

## Proxy lifecycle

### `GET /api/proxy/status`

Returns `ProxyStatus`.

Runtime lifecycle and activity transitions publish the existing `proxy` UI-change resource. The
management UI reads this endpoint initially and after those notifications rather than polling it.
Share-token browser responses use the same status contract internally but retain only their
authorized Session in `active_netlog` and omit `active_bypass_count`; `/share-api/proxy/changes`
provides the authenticated revision-based long-poll invalidation.

### `POST /api/proxy/start`

Starts the MITM listener and returns `ProxyStatus`.

### `POST /api/proxy/stop`

Stops accepting immediately, waits up to five seconds for active exchanges, and returns
`ProxyStatus`. Starting and stopping do not rewrite persisted outcomes. A connection generation
that must be drained or aborted can still mark only its own unfinished captures with
`proxy_shutdown`.

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

### `POST /api/session-shares`

Enables a read-only browser link for one non-archived Session. This operation is idempotent: when
sharing is already enabled, it returns the existing process-local token. It is an ordinary
permission-controlled Agent API action whose default mode is `approval` on desktop and therefore
`deny` on CLI unless the permission is changed.

```json
{ "session_id": 3 }
```

The response contains `session_id`, `enabled: true`, and the cleartext `pcrab_share_…` token. The
owner management service retains that token in process memory so the UI can reopen the same link.
Never use the token as a Bearer credential or print it in a report: the share browser sends it only
as exactly one `token` query parameter to the read-only `/share-api/*` surface.

### `GET /api/session-shares/{id}`

Returns the current process-local share state for one non-archived Session. Its default permission
is `allow`. A disabled response contains `session_id` and `enabled: false`; an enabled response also
contains the current browser token. Treat the token as sensitive and do not expose it in output.

### `DELETE /api/session-shares/{id}`

Disables sharing idempotently and immediately invalidates the Session's previous browser token. Its
default permission is `approval` on desktop and therefore `deny` on CLI unless changed. The response
contains `session_id` and `enabled: false`. Process exit also resets sharing to disabled. An archived
Session is unavailable to the share browser; restoring it in the same process does not rotate its
still-enabled token.

The share bootstrap and proxy-status route are projected through that token's Session scope, so
other Session activity and the global bypass activity count are never returned. Its
`POST /share-api/logs/ids` route uses the same read-only request and
`matched_ids`/`in_progress_ids` response as the management route, while forcibly scoping the
Session and exposing no filter-persistence route.

### Session HAR shares

HAR download sharing is independent from the read-only browser share. Use
`GET /api/session-har-shares/{id}` to inspect its state, `POST /api/session-har-shares` to enable it,
and `DELETE /api/session-har-shares/{id}` to disable it. The defaults are `allow`, `approval`, and
`approval`, respectively. Enabling accepts one Session, a display scope, and the exact capture IDs
to freeze:

```json
{
  "session_id": 3,
  "scope": "filtered",
  "log_ids": [1041, 1042]
}
```

`scope` is `all` or `filtered`; it describes the owner UI selection and does not cause a later
query. IDs are deduplicated and frozen for the lifetime of the share. Re-enabling is idempotent and
keeps the original scope, IDs, and token until the share is disabled. The enabled state includes
`scope`, `log_count`, and a sensitive process-local `pcrab_har_…` token; disabled state has
`log_count: 0` and no token.

The owner UI builds `http://<local-ip>:<api-port>/session.har?token=…`. A GET downloads
`proxycrab-session-<id>.har` directly. Only successful captures with responses are emitted;
in-progress, failed, and synthetic TLS CONNECT records in the frozen selection are skipped. An
empty selection downloads a valid empty HAR. The download contains complete stored headers and
bodies with no redaction, so treat the URL and file as sensitive. Disabling, process exit, or
archiving the Session makes the link unavailable. This browser token is not an Agent Bearer
credential. HAR JSON is streamed one capture entry at a time and has no `Content-Length`; a body or
storage failure after HTTP 200 interrupts the download and leaves a partial file that must be
discarded.

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
`SessionMetadata`. The active Session cannot be archived. The active Session and a Session with
pinned in-progress requests return `409 session_in_use`; archive is otherwise allowed while the
proxy runs. Archived
Sessions are excluded from every existing Session, log, view, interceptor, and export endpoint.

### `GET /api/archived-sessions`

Returns archived `SessionMetadata[]`, ordered by creation time.

### `POST /api/archived-sessions/{id}/restore`

Moves the archived directory back to `sessions/<id>` and returns its unchanged `SessionMetadata`.
Restore does not make the Session active.

### `DELETE /api/archived-sessions/{id}`

Permanently deletes an archived Session and returns `{}`.

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
  "ids": null,
  "min_id": 100,
  "max_id": 10000,
  "limit": 100
}
```

Every field is optional.

- The endpoint is always read-only. Omitting `filter` matches all captures and never reads or
  changes the Session's saved filter.
- `option: null` or an empty `input` matches all captures; an empty input still preserves the
  selected option.
- With `regex: false`, built-in and custom-column filters use case-sensitive contains matching.
- With `regex: true`, input uses Rust `regex` syntax and substring matching unless anchored. Inline
  flags such as `(?i)` control case folding; invalid patterns return `bad_request`.
- Lua filter input is passed exactly, including whitespace.
- Filter/custom-column errors count as non-matches in this endpoint.
- `min_id` and `max_id` are exclusive.
- `ids` re-evaluates an explicit candidate set and cannot be combined with range bounds.
- Default and maximum `limit` are 10,000. `0` returns no IDs.
- Only `min_id` scans newer IDs. `max_id` or no bounds scans older IDs.
- For filtered range queries, the server scans until it has collected `limit` matches or exhausted
  the bounded range. Therefore a page shorter than `limit` means that range is exhausted; paginate
  a full result by passing the smallest returned match as the next exclusive `max_id`.
- Response ID order is intentionally unspecified.

Response:

```json
{
  "matched_ids": [1042, 1041],
  "in_progress_ids": [1042]
}
```

The arrays may overlap. Use `matched_ids` for result membership. `in_progress_ids` contains only
persisted in-progress records that are also active in the current run's authoritative status; stale
unfinished records from earlier runs are excluded. Keep re-evaluating those IDs until they complete.
When an ID leaves `in_progress_ids`, re-evaluate it once more and use that final `matched_ids`
membership; current UI activity still comes only from `ProxyStatus.running.active_netlog`.

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
    { "kind": "method", "width": 50.0 },
    { "kind": "created_at", "width": 200.0 },
    { "kind": "updated_at", "width": 200.0 }
  ],
  "rows": [
    {
      "id": 1043,
      "created_at": 1785380000000,
      "updated_at": 1785380000100,
      "outcome": "success",
      "cells": ["GET", "1785380000000", "1785380000100"]
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

The ID, numeric `created_at`/`updated_at`, and persisted capture `outcome` (`in_progress`, `success`,
`failed`, or `tunneled`) are separate from cells. When `created_at` or `updated_at` is selected as a
table column, its aligned cell is the same Unix-millisecond value encoded as a decimal string, so
the string-cell protocol remains unchanged. `column_index` is zero-based. A column error leaves that
cell empty; a missing log has no row or `column_index`. IDs and rows are unordered. Unchanged logs
appear in neither `rows` nor `exceptions`.

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
  "has_snapshot": true,
  "modifications": [
    { "kind": "header_set", "name": "x-debug", "value": "1" }
  ],
  "error": null
}
```

The exact executed Lua source is not embedded. Read it from
`GET /api/logs/{id}/interceptors/{execution_id}/content`. The entry snapshot is also not embedded:
`has_snapshot` is `true` only when that execution changed Method, URI, Status, Headers, or Body,
and the complete entry state is available at
`GET /api/logs/{id}/interceptors/{execution_id}/snapshot`. Modification variants are `method_set`
with `method`, `uri_set` with `uri`, `status_set` with `status`, `header_append`, `header_set`,
`header_remove` with `values`, `body_replace_string` with `content`, `body_replace_asset` with
`asset_id`, and `tag_set` with `key` and `value`. Multiple temporary executions may share the same
phase and position; use `execution_id` and array order rather than treating position as unique.

### `GET /api/logs/{id}/interceptors/{execution_id}/content?session_id=3`

Returns the exact UTF-8 source executed by that interceptor execution as an unwrapped
`text/plain; charset=utf-8` stream. The `X-ProxyCrab-Script-SHA256` response header carries the
lowercase SHA-256 that also appears as the execution's `script_hash`. A missing execution returns
404 `execution_not_found`.

### `GET /api/logs/{id}/interceptors/{execution_id}/snapshot?session_id=3`

Returns the entry snapshot captured for that execution:

```json
{
  "ok": true,
  "data": {
    "request": {
      "method": "GET",
      "uri": "https://example.com/",
      "version": "HTTP/1.1",
      "headers": { "host": ["example.com"] },
      "body": { "type": "original" }
    },
    "response": null
  }
}
```

The absent side is `null`. Snapshot bodies are `original`, `string` with complete `content`, or
`asset` with `asset_id`; tags and the response interceptor's read-only request are not included.
When `has_snapshot` is false or the execution does not exist the endpoint returns 404
`snapshot_not_found`.

### `GET /api/logs/{id}/body?session_id=3&side=request&max_size=16777216`

Returns the final Body constructed after interceptors as an unwrapped raw byte stream. The final
source is the original network Blob, a complete replacement string, or an Asset. `blob/` contains
only original network bodies; ProxyCrab does not create `.modified` files.

Add `execution_id=17` to read the selected interceptor-entry snapshot body. The endpoint can
resolve `original`, `string`, and `asset` sources; the desktop snapshot window intentionally shows
only `asset_id` for Asset snapshots instead of fetching their bytes. `side` is required and must be
`request` or `response`.
`max_size` defaults to 16 MiB, has no server maximum, and checks the selected final Body source
before any decoding or recompression. Oversized bodies return 413 with the stored sizes:

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

ProxyCrab negotiates the response from `Accept-Encoding`. If every captured content encoding is
accepted, including through `*`, it streams the original bytes and preserves the complete captured
`Content-Encoding` stack and stored `Content-Length`. A specific `q=0` prohibition overrides `*`.
Otherwise ProxyCrab decodes gzip, br, deflate, zstd, or stacked encodings and chooses the first
allowed output from the fixed `gzip`, `deflate`, identity fallback chain. Unlisted `identity`
follows `*`: `*;q=0` forbids identity exactly like `identity;q=0` (RFC 9110 §12.5.3). Nonzero q
weights do not change that order; malformed entries are ignored. Missing or empty `Accept-Encoding` selects
identity. If no output is allowed, the endpoint returns `406 not_acceptable_encoding`.

Transcoded and identity responses omit `Content-Length`; gzip and deflate responses set their
selected `Content-Encoding`. Every response includes `Vary: Accept-Encoding`, and
`X-ProxyCrab-Body-Size` always reports the original stored byte count. A malformed encoded stream
can terminate after HTTP 200 has begun. Empty body files return 200 with zero bytes.
Missing/not-yet-produced bodies return `body_not_found`; unreadable files return
`body_read_failed`; an original encoding that cannot be decoded for fallback returns
`body_decode_failed`.

### Replaying a request: `POST /api/replay?session=3`

Sends a fully specified request through the target Session's interceptor pipeline and outbound
chain, recording it as a normal capture whose source is `ProxyCrabRequest`. The proxy must be
running. The response returns as soon as the capture is created — it does not wait for the
upstream exchange to finish.

```json
{
  "method": "POST",
  "url": "https://example.com/api?x=1",
  "headers": [["content-type", "application/json"]],
  "body": { "type": "text", "text": "{\"a\":1}", "charset": "utf8" }
}
```

- `headers` is an ordered array of `[name, value]` pairs; duplicates are preserved and sent
  as-is (including `content-length` — the caller is responsible for consistency).
- `body` is optional (omit for no body) and is one of:
  - `{ "type": "text", "text": "...", "charset": "utf8" }` — only `utf8` is supported.
  - `{ "type": "body_ref", "session_id": 1, "log_id": 2, "side": "request" | "response" }` —
    reuse the stored blob body of an existing capture.
  - `{ "type": "asset", "asset_id": "dir/file.bin" }` — reuse a workspace asset.
- Success: `{ "ok": true, "data": { "log_id": 12 } }`. Errors: `not_found` (session/body/asset),
  `proxy_not_running`, `bad_request` (validation), `replay_failed`.

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

### `PUT /api/sessions/3/filter`

Saves only the Session filter and preserves its columns:

```json
{ "option": { "kind": "column", "column": { "kind": "uri" }, "regex": false }, "input": "/api" }
```

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

Assets are immutable files shared across the active workspace. They have no update or delete
endpoint. `GET /api/assets` lists every asset's metadata (`id`, `size`, `content_type`,
`sha256`, `created_at`) sorted by id inside the normal envelope; build trees from the
`/`-separated ids. Upload with a raw request body:

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

IDs allow `[a-z0-9_./-]`, up to 255 bytes total and 100 bytes per segment. They cannot start/end with
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

### `GET /api/breakpoints/{id}/body?side=request&max_size=16777216`

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
DELETE /api/bypass             clears all terminal and stale entries
```

Single delete returns `{}`. Batch and clear return `{ "deleted": N }`. A current-run in-progress
entry cannot be deleted and clearing skips it. An in-progress entry is stale and deletable when the
proxy is not running or its `created_at` is earlier than `ProxyStatus.running.started_at`. Database
open and proxy start/stop preserve its outcome; generation-scoped forced draining may still mark
that generation's unfinished rows failed with `error: "proxy_shutdown"`.

## Certificate authority

### `GET /api/ca`

Returns:

```json
{ "pem": "-----BEGIN CERTIFICATE-----\n..." }
```

The same CA is available through the proxy at `http://proxy.crab/ca.crt`. This local URL bypasses
routing, Session capture, and bypass persistence.

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
| `remote_auth_required` | 401 | A non-local Host or Origin omitted Bearer authorization |
| `permission_denied` | 403 | The identity or an approval decision denied this route action |
| `approval_timeout` | 403 | No desktop decision arrived within 30 seconds |
| `not_found` | 404 | Missing endpoint, Session, log, or script |
| `log_not_found` | 404 | Missing capture for a body request |
| `body_not_found` | 404 | Requested side has not produced a body file |
| `execution_not_found` | 404 | Missing interceptor execution for a content request |
| `snapshot_not_found` | 404 | Missing interceptor execution or snapshot |
| `snapshot_body_not_found` | 404 | Missing snapshot body for an interceptor execution |
| `not_acceptable_encoding` | 406 | `Accept-Encoding` prohibits the original and every fallback encoding |
| `conflict` | 409 | State conflict, including no active Session or other state conflicts |
| `session_in_use` | 409 | Archiving the active Session or one with pinned in-progress requests |
| `proxy_running` | 409 | Proxy start requested while it is already running or changing state |
| `proxy_not_running` | 409 | Replay or another proxy-dependent operation while the proxy is stopped |
| `body_too_large` | 413 | Stored body exceeds `max_size`; includes both sizes |
| `body_decode_failed` | 422 | Original encoding cannot be decoded for a negotiated fallback |
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
- Filter/custom-column `entry.req.body` and `entry.resp.body` getters lazily read and cache effective
  persisted bodies. In-progress bodies return `nil`; see `lua-api.md` for decoding and size limits.
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
