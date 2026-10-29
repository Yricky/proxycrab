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
17. [Clash subscription](#clash-subscription)
18. [Redaction library and bindings](#redaction-library-and-bindings)

## Script relative paths

For all six libraries (column, filter, routing, request, response, redaction), `name` is a
relative path without the automatically appended `.lua` extension. For example,
`business/errors` resolves to `scripts/filter/business/errors.lua` in the filter library.
Lists include nested scripts and preserve the full relative path. Creation makes missing
parent directories. Existing root-level names remain valid. Bindings use the full path.

Names are limited to 128 UTF-8 bytes. Each `/`-separated segment may contain letters, numbers,
`.`, `_`, and `-`; empty segments, `.` and `..`, absolute paths and backslashes are rejected.
Script operations do not follow symlinks within the library. Encode the **whole** name as one
URL parameter: `/api/filter-scripts/business%2Ferrors` (and append `/debug` for debugging).
JSON bodies and helper `--name` arguments use the unencoded value `business/errors`.

The script managers and replay workspace-asset picker use the same compact, directory-first file-tree component. Asset entries retain their original extensions, size and content-type metadata; script labels append `.lua` without changing API identities. Directories are derived from
script paths; empty-directory management, rename and drag-and-drop moves are not supported.

## Script display metadata

Script list/detail and interceptor library/Session-chain responses may contain `display_name`.
Use it only as a label; continue addressing scripts by `name` / `script_name`. Set the label by
updating the leading `-- #display_name: ...` comment in `content`. Empty or missing metadata falls
back to the original name. Historical execution labels use their recorded source. Share responses
retain the label but omit script source content. See [Lua metadata rules](lua-api.md#script-display-names).

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

The CLI prints its UI access token for the browser landing page. It generates a per-run
`pcrab_ui_…` token by default; `--ui-key KEY` (with or without `run`) supplies a custom value.
Custom values require nonempty visible ASCII without whitespace/control characters, but no prefix.
There is no environment-variable or persisted-config option. Both forms authorize private UI
routes and trusted UI calls, are not Agent API keys, and must never be copied into
`PROXYCRAB_API_KEY`. A custom UIKey cannot bypass a startup `--redaction-script` policy.

Session sharing uses separate `pcrab_share_…` tokens and `/share-api/*` routes for browsers and
agents. Those tokens are scoped to one read-only Session. Temporary shares last for the current
process; optional persistent shares restore the same token after restart while sharing remains enabled. Agents may use a supplied link directly with the share read API
below, without a management key. Share tokens are not accepted by `/api/*` and are not
`PROXYCRAB_API_KEY` values or Bearer credentials.

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

### UI synchronization

Successful management mutations synchronize into the desktop or CLI browser UI. They preserve the
viewed Session unless it is archived, in which case the first remaining Session is selected.
Open editors preserve unsaved input and warn about external changes; do not discard the user's work.

## Agent instructions

### `GET /api/agents.md`

Read this endpoint immediately after reading the ProxyCrab Skill and before calling any other
ProxyCrab API. It returns the current workspace's active AGENTS.md preset as an unwrapped UTF-8 body
with `Content-Type: text/plain; charset=utf-8`.

The user's current explicit request takes precedence over the returned document. If the endpoint is
unavailable, use the Skill's conservative built-in behavior. This read is stateless and does not
notify or refresh the desktop UI.

Preset management is available in the owner UI, not through the Agent HTTP API.

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
{ "type": "text", "content": "plain text", "size": 10 }
{ "type": "json", "content": { "key": "value" }, "size": 15 }
{ "type": "binary", "size": 2048 }
{ "type": "large", "size": 73400320 }
```

Text/JSON content is embedded only when its decoded representation is at most 64 KiB. `size`
describes the selected final source: original stored bytes may still be content-encoded, string
replacements expose their replacement content, and Asset replacements are read from the workspace
Asset store. Body detail never exposes a local capture path; retrieve complete body bytes through
the raw endpoints below. An empty original request or response is represented as `empty` without
creating a blob file. An empty string replacement is also `empty` and is stored in capture metadata;
ProxyCrab does not create `.modified` capture blobs. The raw body endpoint still returns a
successful zero-byte stream.

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

Returns a `SessionsPayload`:

```json
{
  "sessions": [
    { "id": 3, "name": "checkout-debug", "created_at": 1234567890, "description": null }
  ],
  "active_session_id": 3
}
```

`sessions` contains non-archived `SessionMetadata` entries. `active_session_id` is `null` when no
Session is active, including an empty workspace (`{ "sessions": [], "active_session_id": null }`).
`scripts/session-list.mjs` emits the same object with `columns` added to each Session.

### `POST /api/sessions`

```json
{
  "name": "checkout-debug",
  "description": "Capture checkout failures",
  "view": {
    "columns": [{ "kind": "stage", "width": 91.0 }],
    "filter": { "option": null, "input": "example" }
  },
  "interceptors": {
    "request": [{ "name": "rewrite", "enabled": true }],
    "response": []
  }
}
```

All fields are optional or `null`. ProxyCrab generates a name when omitted. Returns the created
`SessionMetadata`. `view` uses the same shape as the Session view (`columns` plus `filter`); when
omitted, the Session uses the default table columns and an empty filter. `interceptors` presets the
Session interceptor chains by name without validating that the referenced interceptors exist; when
omitted, both chains are empty. Creation never changes the active Session, and an inactive Session
captures nothing: call `PUT /api/active-session` afterwards to make the new Session active.

### `GET /api/session-shares`

Returns enabled Session page-share summaries in one read. Its default permission is `allow` and it
uses ordinary management authentication. It returns only currently non-archived Sessions:

```json
{ "ok": true, "data": { "shares": [{ "session_id": 3, "persistent": true }] } }
```

Presence means sharing is enabled; disabled, archived, and deleted Sessions are omitted. No token,
redaction binding, or unrelated state is returned. Restored Sessions reappear if sharing remains
enabled. Ordering is unspecified. Use `scripts/session-share-list.mjs` to inspect summaries.
The desktop equivalent is `list_session_shares`; both backends expose optional `listSessionShares()`.
List indicators consume this batch rather than querying each Session. A denied/failed read does not
mean all shares are disabled: preserve last known state, or treat it as unknown if none was loaded.
Share changes refresh summaries; Session list refreshes fetch both lists in parallel.

### `POST /api/session-shares`

Enables sharing or updates an enabled share's config for one non-archived Session. This is an ordinary
permission-controlled Agent action (`approval` on desktop, therefore `deny` on CLI unless changed).
The existing POST handles both creation and configuration; there is no separate persistence route.

```json
{
  "session_id": 3,
  "session_share_config": {
    "redaction_script": "mask",
    "persistent": true
  }
}
```

Share redaction is resolved by the backend, not copied by the frontend. For a new owner-created
share, omitting `session_share_config.redaction_script` inherits the current local no-API-key binding.
Explicit `null` selects that same inheritance; `""` explicitly disables share-level redaction; a
nonempty name selects a library script. On updates, an omitted redaction field preserves the current
policy; use explicit null to switch back to local inheritance. A supplied config updates persistence
without rotating the token; an omitted config preserves an existing owner share.

API keys have stricter rules. A Key bound to a nonempty script must omit the share redaction field
and the share follows that Key's binding dynamically, including on updates. An unconfigured Key
(null/missing binding) also cannot supply the field; new shares inherit local. Only a Key whose own
binding is explicitly `""` may supply the field, including null, empty, or a script name. Violations
return HTTP 403 `share_redaction_forbidden` before modifying share state. Owner Tauri IPC, local
management requests and the trusted CLI UI may select policies normally. Ordinary route permission
checks still apply. Existing null Key bindings are not treated as explicit opt-out.

Inherited policies resolve their identity on each share request, so changing local or Key bindings
updates share reads without rotating links. A missing/deleted Key or failed identity lookup fails
closed (`redaction_failed`); it never falls back to original data. Status reports the configured
`redaction_script` and optional `redaction_identity` (a stable Key ID, never an API Key credential).
The frontend shows inheritance separately from explicit no redaction and never reads local defaults.
The startup `--redaction-script` policy still overrides all these settings.

Updates use `POST /api/session-shares` exclusively. The old
`PUT /api/session-shares/{id}/redaction` route and desktop `set_session_share_redaction` command
have been removed; persisted permissions drop that obsolete action.

Persistent share files use version 2 and retain policy references. Version 1 files remain readable:
old null share bindings migrate to explicit `""` to preserve their original unredacted behavior;
named script bindings and tokens are preserved. New inherited shares retain null policy references
across restart and resolve the current binding when read.

The response contains `session_id`, `enabled: true`, `persistent`, `redaction_script`, and the
cleartext `pcrab_share_…` token. Only persistent entries are written to
`<workspace>/session-shares.json` (version 2; version 1 migration is described above), including the token and redaction script binding.
They are restored enabled on desktop/CLI startup. Temporary entries stay in memory. Switching to
temporary removes the durable entry while leaving the current link valid until exit. Save errors
are reported without committing the proposed in-memory state; malformed/unsupported files fail
startup instead of silently overwriting credentials. On Unix, new credential files use mode 0600.
Browsers and agents send the token as exactly one `token` query parameter to `/share-api/*`, not as
a Bearer credential. Address or port changes still require updating the URL.

### `GET /api/session-shares/{id}`

Returns the current share state for one non-archived Session. Its default permission
is `allow`. A disabled response contains `session_id`, `enabled: false`, and `persistent: false`; an enabled response also
contains the current share token, usable by browsers and agents through the share read API below.

### `DELETE /api/session-shares/{id}`

Disables sharing idempotently and immediately invalidates the Session's previous browser token. Its
default permission is `approval` on desktop and therefore `deny` on CLI unless changed. The response
contains `session_id`, `enabled: false`, and `persistent: false`. Disabling also removes the durable
entry before returning success, so restart cannot re-enable it. Re-enabling generates a new token.
Process exit invalidates temporary shares; persistent shares resume on restart. Archived Sessions
are unavailable, but restoring them preserves their still-enabled shares. Permanent Session deletion
also removes its share entry.

The share bootstrap and proxy-status route are projected through that token's Session scope, so
other Session activity and the global bypass activity count are never returned. Its
`POST /share-api/logs/ids` route uses the same read-only request and
`matched_ids`/`in_progress_ids` response as the management route, while forcibly scoping the
Session and exposing no filter-persistence route.

### Session share read API

Given `/session?token=…`, preserve the origin and send the decoded token as exactly one URL-encoded
`token` query parameter on each route below. These routes need no management API key or
`Authorization` header (that header is ignored). POST bodies use `Content-Type: application/json`.
The server fixes the Session from the token; omit `session_id` because any supplied value is ignored.
The page URL may also have `id`, which selects a capture, not a Session. Start with bootstrap and
then query IDs and details; see the runnable example in [SKILL.md](../SKILL.md#read-a-session-sharing-link).

All paths below are relative to the link's origin and require `?token=…` (or `&token=…` when other
query parameters are present). JSON success responses use `{"ok":true,"data":…}`; errors use
`{"ok":false,"error":{"code":"…","message":"…"}}`. Body and interceptor source successes are raw.

| Method and path | Input and result |
| --- | --- |
| `GET /share-api/bootstrap` | Returns `{session, view, proxy_status, api_port}` for the shared Session. |
| `GET /share-api/session-view` | Returns the shared Session view, including columns, saved filter, and optional `redaction_revision`. |
| `GET /share-api/session-interceptors` | Returns `{session_id,request,response}` for the Token's Session, ignoring a supplied `session_id`. Ordered items contain only `{name,display_name?,enabled,valid}`; no library source. GET only; no write methods. |
| `GET /share-api/proxy/status` | Returns scoped proxy status; use `running.active_netlog` for current activity. Other Sessions and global bypass counts are excluded. |
| `GET /share-api/proxy/changes` | Optional `after` revision string. Returns `{revision, changed}`; matching revisions long-poll for up to 25 seconds. Use the returned revision on the next call. |
| `POST /share-api/logs/ids` | Same body/result as `POST /api/logs/ids` below: optional `filter`, `ids`, exclusive `min_id`/`max_id`, `limit` (default/max 10000). Returns `{matched_ids, in_progress_ids}`. `{}` matches all, without changing the saved filter. |
| `POST /share-api/logs/views` | `{"logs":[{"id":42,"updated_at":123}],"view":{"columns":[{"kind":"method","width":50}]}}`. At most 200 rows; timestamps and `view` are optional. Same table payload and conditional row semantics as `/api/logs/views`. |
| `GET /share-api/logs/{id}` | Same `LogDetail` as `/api/logs/{id}`; ID must belong to the shared Session. |
| `GET /share-api/logs/{id}/body` | Required `side=request|response`; optional positive `max_size` (default 16777216), `execution_id` for an interceptor-entry body. Raw bytes with normal Body encoding negotiation; use `Accept-Encoding: identity` for decoded bytes and save to a file. A missing snapshot never falls back to the final Body. |
| `GET /share-api/logs/{id}/websocket/messages` | Required `side=request|response`; optional `after_id`, `limit` (default 500). Same message metadata/pagination as the management equivalent; continue with `next_after_id`. |
| `GET /share-api/logs/{id}/websocket/messages/{message_id}/body` | Required `side`; optional `max_size` (default 16777216). Raw message bytes; respect hidden/unavailable content errors. Message IDs are per direction. |
| `GET /share-api/logs/{id}/interceptors/{execution_id}/content` | Raw executed Lua source, `text/plain`, with `x-proxycrab-script-sha256` header. |
| `GET /share-api/logs/{id}/interceptors/{execution_id}/snapshot` | Same interceptor-entry snapshot JSON as the management equivalent. Use an execution ID from log detail. |
| `GET /share-api/column-scripts` | Script metadata array `{name, display_name, content:""}`; library source is not exposed. |
| `GET /share-api/filter-scripts` | Same metadata-only shape for filter scripts. |
| `POST /share-api/validate-filter-regex` | `{"pattern":"…"}`; data is `null` for a valid Rust regex, otherwise a validation error string. |

The shared UI reuses the interceptor pipeline with operation availability determined by Backend
methods. It can show chain order, display names, enabled/valid state and HAR export, while hiding
add/edit/recreate/remove, toggle, drag, sweep and breakpoint controls. The existing 1.5-second metadata
poll detects chain changes and emits scoped `session_interceptors` updates; stopped subscriptions
emit no further events. Current chain configuration is distinct from historical per-log executions.

Only the routes above exist on the share surface; there are no Session writes, filter persistence,
replay, export, breakpoint control, or `/agents.md` routes. Share reads honor effective redaction.
`invalid_share_token` (401) means missing, duplicate, invalid, or revoked token;
`share_session_unavailable` means the Session is unavailable (for example archived). Disabling sharing invalidates the token. Process exit invalidates temporary shares; persistent shares
restore the same token on restart. Report these errors rather than falling back to management
credentials; report `redaction_failed` without changing the policy.

### Frontend HAR export

The export button beside the sweep action opens a floating window. Its Export HAR button freezes
all matching list IDs and their displayed order, then reads details and bodies through existing
Backend methods (`getLog`, `fetchBody`). No HAR export or sharing HTTP endpoint exists. The shared
Session page uses the existing scoped read APIs and exports their policy-filtered data.

Entries are generated one at a time. Failed and unfinished ordinary HTTP records are retained;
missing responses have status 0 and `_proxyCrab` carries outcome/error metadata. WebSocket connection
rows and synthetic TLS CONNECT records are skipped; HTTP upgrade handshakes remain. Body read
failures (including decode and size errors) become text explaining the failure and `bodyErrors`
metadata, with warnings in the export window. Unavailable details produce a failed placeholder
entry. Empty selections produce valid HAR. Body bytes are read separately rather than reusing JSON
previews, preserving formatting; binary content uses Base64 (`encoding` for responses, `_encoding`
for request postData). Detailed timings/header sizes are unknown (-1).

`saveFile({filename})` returns `FileWriter | null`; null means the desktop picker was cancelled.
The Writer offers async `append(bytes)`, `markAsFinish()`, and `markAsError()`. Desktop chunks use
binary IPC (at most 256 KiB each) and a same-directory temporary file; finish publishes the file,
error discards it. Failed writes cannot be committed. Browser writers buffer Blob parts and start
a download only on finish; this acknowledges download initiation, not completion on disk. Abort
never starts a download. No new management or share HTTP route is needed for writing.

Progress includes processed/exported/skipped records and warning counts. The window retains the
latest 1000 log lines; cancel/close abandons an unfinished export. Each Body still uses the existing
read interface (explicit 1 GiB stored-size ceiling); a single entry is buffered, and the browser
fallback buffers the final file. Export does not provide an atomic snapshot of changing captures
or pin one redaction revision across separate reads. Original content is never used as a fallback
for policy-denied reads. Exported files may contain sensitive data from the authorized read view.

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

### `PUT /api/active-session`

Accepts and returns `{ "session_id": 3 }` or `{ "session_id": null }`. Read the current active ID
through `GET /api/sessions`. A non-null Session ID must exist. Changing or clearing the
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
source is the original network body, a complete replacement string, or an Asset.

Add `execution_id=17` to read the selected interceptor-entry snapshot body. The endpoint can
resolve `original`, `string`, and `asset` sources, including in the snapshot viewer. `side` is required and must be
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

The Session share equivalent, `GET /share-api/logs/{id}/body?token=…&side=request&execution_id=17`,
uses the same snapshot selection, encoding negotiation, and size rules. The share token fixes the
Session; a supplied `session_id` cannot select another Session. A missing snapshot returns a
snapshot error rather than falling back to the final Body. Share tokens remain share-only credentials.

The UI's Body download button saves complete HTTP-decoded bytes independently of preview loading,
formatting, and size limits. Browser resource limits still apply to large downloads. Snapshot
selection does not fall back to the final Body when the snapshot is missing. `body-get.mjs` requires
an output path and uses its explicit stored-size maximum or the 16 MiB default. Applicable read-time
redaction policies still apply.

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

### `GET /ca.crt`

Public raw-file download on the management HTTP port. No proxy setup, running proxy, Bearer key,
subscription token, or active Session is required. It returns only the current public certificate
as PEM (`application/x-x509-ca-cert`, attachment `proxycrab-ca.crt`, `Cache-Control: no-store`).
This is the sole HTTP certificate-read endpoint; it has no management permission entry.
Regenerating the CA updates the file at the same URL. Use a non-loopback IPv4 reachable from
the device downloading the certificate.

```bash
node <skill-dir>/scripts/certificate-download.mjs --out /tmp/proxycrab-ca.crt
```

The script uses the usual base URL options and reports the saved path and byte count. Its shared
CLI connection helper still requires `PROXYCRAB_API_KEY` for remote base URLs; browsers and other
HTTP clients can download this public file without authentication.

## Clash subscription

`GET /api/clash-subscription` reads the current state (default permission: `allow`).
`PUT /api/clash-subscription` takes `{"server":"192.168.1.20"}` to enable or update the selected
non-loopback IPv4; `DELETE` disables it (both default to `approval`, which CLI treats as deny for
Agent identities until allowed). Disabled state is `{"enabled":false}`; enabled state is
`{"enabled":true,"server":"192.168.1.20"}`. No download token is generated, returned, or validated.
State is in memory and resets on restart.

`GET /api/clash-config` returns `{"yaml":"…","default_yaml":"…"}` (default permission:
`allow`). `PUT /api/clash-config` takes `{"yaml":"…"}` and validates before writing; `DELETE`
restores the built-in default (both default permission: `allow`). The file is
`<workspace>/clash/config.yaml` and persists across restarts, independently of the subscription
switch. It is a complete Clash YAML (including `mode`, `proxies`, optional groups, and `rules`) and
must contain both `{{server}}` and `{{port}}`. Saving validates that the YAML parses, the top level
is a mapping, `proxies` is a non-empty list of mappings with non-empty `name` and `type`, `rules` is
a non-empty list of non-empty strings, the final rule is `MATCH,<target>`, both placeholders are
present, and the file is at most 1 MiB. The raw file is a template, so validation substitutes
`192.0.2.1` and `8089` for the placeholders before parsing. Invalid input returns `400 bad_request`
and does not write the file.

`GET /clash.yaml` is a separate public download route, outside management Bearer auth and the
management permission catalog. It is available only while the subscription is enabled; otherwise it
returns `404 not_found`. The URL requires no token. The downloaded YAML replaces `{{server}}` with
the selected IPv4 and `{{port}}` replaced by the current configured proxy port; all other text,
comments, and formatting are preserved. The file is not validated at download time, so external
edits are served as-is. Responses use `Cache-Control: no-store`, `Referrer-Policy: no-referrer`,
`application/yaml; charset=utf-8`, and attachment filename `ProxyCrab.yaml`.

The default file is:

```yaml
# ProxyCrab Clash subscription
# The server and port placeholders are replaced when Clash downloads this file.
mode: rule
proxies:
  - name: ProxyCrab
    type: http
    server: "{{server}}"
    port: {{port}}
rules:
  - MATCH,ProxyCrab
```

The proxy port is read from current configuration on each download. The proxy may be stopped when
enabling or downloading the subscription; neither operation changes its lifecycle. The QR code
contains the HTTP download URL. The import button opens
`clash://install-config?url=ENCODED_HTTP_URL`. Users select the imported profile and enable system
proxy in Clash. Changing the chosen IP changes the URL host, so update the subscription URL in
Clash; changing the proxy port only requires refreshing the subscription. Re-enabling with the same
IP keeps the same URL. Closing the subscription does not remove previously imported configurations
or stop existing proxy traffic.

```bash
node <skill-dir>/scripts/clash-subscription.mjs --action status
node <skill-dir>/scripts/clash-subscription.mjs --action enable --server 192.168.1.20
node <skill-dir>/scripts/clash-subscription.mjs --action disable
node <skill-dir>/scripts/clash-subscription.mjs --action config
node <skill-dir>/scripts/clash-subscription.mjs --action save-config --file /tmp/clash-config.yaml
node <skill-dir>/scripts/clash-subscription.mjs --action reset-config
```

The script prints only subscription state for status/enable/disable and prints `yaml` and
`default_yaml` for config operations. Use an address reachable from the device running Clash;
`127.0.0.0/8`, `0.0.0.0/8`, multicast/broadcast, IPv6, and non-IP values are rejected.

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
- A captured request whose final request-interceptor tag `_crab_upstream_http_version` is exactly
  `HTTP/1.1` or `HTTP/2` forces that protocol only on the ProxyCrab-to-upstream hop. Forced HTTP/2
  is strict and does not fall back to HTTP/1.1. Invalid values warn and preserve default behavior;
  the tag is persisted but never sent as a header or substituted for the captured client version.
- `updated_at` is a monotonic Unix-millisecond version of the complete capture. Metadata and body
  writes advance it even within the same wall-clock millisecond.
- Interceptor changes made before a runtime error remain applied and are recorded.

## Redaction library and bindings

The desktop and CLI owner UI expose a global redaction script library and one script selector for
each local identity/API key, Session share. Normal capture and forwarding are unaffected.

| Endpoint | Contract |
| --- | --- |
| `GET /api/redaction-scripts` | List scripts (default permission: allow) |
| `POST /api/redaction-scripts` | Create `{ "name": "mask", "content": "..." }` (allow) |
| `GET /api/redaction-scripts/{name}` | Read script (deny) |
| `PUT /api/redaction-scripts/{name}` | Replace `{ "content": "..." }` (approval) |
| `DELETE /api/redaction-scripts/{name}` | Delete script; existing bindings remain and fail closed (approval) |

The headless CLI accepts `--redaction-script /path/to/mask.lua` (with or without `run`).
It reads and compiles the file before starting the HTTP API or proxy; a missing/unreadable file or
invalid Lua aborts startup. The source is pinned in memory for the process lifetime, is not added to
the workspace library, and changes require a restart. This policy overrides identity and share
bindings, including missing bindings and the UI access token (`UIKey`). It applies to the existing
redaction-supported reads marked in the permissions UI and to Session share reads; it does not
expand the redaction data contract below or change capture/forwarding.

In this mode, all five library operations above reject authorized calls with HTTP 403,
`redaction_management_disabled`, and message `脱敏接口管理功能已禁用`. Normal authentication and
permission checks still apply. Binding or clearing an identity/share cannot disable or replace the startup policy; selecting a named
library script also fails its library validation while management is disabled. Do not retry with
another identity or treat this error as an upsert conflict.

`POST /api/session-shares` accepts redaction and persistence together in `session_share_config`,
including updates to enabled shares without rotating tokens. Share-state responses include
nullable `redaction_script`.
Owner-only CLI `PUT /ui-api/permissions/identities/{id}/redaction` accepts the same binding object;
the desktop command is `set_http_identity_redaction`. Identity permission reads include the binding,
and permission-table updates preserve it. Without a startup redaction script, the CLI UI access
token and ordinary Tauri IPC reads are unbound. Desktop Body previews (`fetch_body`) and native
downloads (`download_body`) use trusted Tauri IPC and do not use the local HTTP identity binding.

Redaction applies to log details, body reads, snapshot details/bodies, URL/custom-column view cells,
URI/custom filters, filter debugging, breakpoint details/bodies/summary URLs, bypass URLs. Frontend HAR uses those same authorized reads.
It does not sanitize interceptor modification history, script source, tags, system logs or Asset
raw downloads. Column output is not post-processed; column scripts receive sanitized inputs.

Every read resolves a script version. Missing/failed bound scripts return HTTP 500
`redaction_failed` with a fixed message; script-controlled errors are not included. Body execution
is lazy and failures are propagated even if a downstream column/filter catches the body error.
Bound view requests re-render requested rows even when `updated_at` is unchanged. Session-view
responses include `redaction_revision` when bound; share clients invalidate membership, rows and
open detail/body views when it changes. Frontend HAR reads resolve policies independently per
request and replace failed reads with diagnostics; they never retry with original data.

See the Lua reference's **Read-time redaction scripts** section for the three-state context contract.

### Browser secure-context limits

JWT decoding works on HTTP, but signature verification requires a secure context (HTTPS or trusted
localhost). A browser JWT verification failure does not establish a server-side replay failure or
change management API permissions.

## WebSocket message capture

HTTP/1.1 WebSocket 13 upgrades create a separate linked `ws://`/`wss://` capture with
`request.version = "WebSocket/13"`. The handshake remains an HTTP log. `GET /api/logs/{id}`
includes optional `websocket` metadata on both records: `handshake_log_id`,
`websocket_log_id`, `subprotocol`, `extensions`, `opened_at`, `closed_at`, `close_code`,
`close_reason`, and optional `interceptors: [{execution_id, name}]` in callback order.
These source execution IDs refer to the handshake log’s existing interceptor content endpoint;
source code is stored once per execution, not repeated per message. Use runtime active-netlog membership for live state, not missing close time.
Close reason is omitted under a read policy because it is peer-controlled text. Delivery and callback
error text is replaced with generic errors under read policies to avoid exposing payloads through errors.

- `GET /api/logs/{id}/websocket/messages?session_id=S&side=request|response&after_id=0&limit=500`
  returns `{messages, next_after_id, has_more}` inside the standard API envelope.
  Limit is 1–2000. Passive-capture IDs start at 1 independently per direction. Intercepted connections use a global processing sequence as the ID, so each direction may have gaps. `next_after_id` is the last
  returned ID or the input cursor, including when `has_more=false`; poll with that cursor.
  Entries contain `id`, `type` (`text`/`binary`), `started_at`, `completed_at` (Unix ms),
  `size` (decoded byte count), `content_available`, `omitted_reason`.
  Intercepted messages additionally expose optional `sequence` (connection-wide finalized order),
  `status` (`sent`, `dropped`, `unsent`, `failed`), `root` and `parent` references (`{side,id}`),
  `producer` (`{execution_id,name,invocation_id,send_index}`), `dropped_by`
  (`{execution_id,name,invocation_id}`), `error` (delivery failure), and `errors` (callback failures,
  each `{execution_id,name,invocation_id,error}`). `root` is the originating network message;
  `parent` is the directly triggering message, which can itself be generated. Message IDs remain
  per direction; never resolve a reference using ID alone. Legacy passive records have no delivery
  status and do not prove successful delivery. Dropped and failed messages retain readable payloads
  subject to the normal content limits and read policies.
- `GET /api/logs/{id}/websocket/messages/{message_id}/body?session_id=S&side=request|response&max_size=16777216`
  returns policy-filtered bytes (`text/plain; charset=utf-8` or `application/octet-stream`),
  with `Cache-Control: no-store`. Limits apply before and after redaction. Empty messages are valid.
  Omitted capture content produces `message_content_unavailable` (422), policy-hidden content
  `message_content_hidden` (403), failed/legacy policies `redaction_failed` (500).
- Equivalent `/share-api/...` paths use the share-scoped Session and policy; supplied Session IDs
  cannot escape the share. Desktop methods call the same manager implementation.

The request body file stores C2S and response body file S2C. These are internal containers,
not HTTP response bodies: generic body reads are rejected, detail payloads are empty, and
WebSocket connection rows are excluded from HAR. Handshake HAR entries remain eligible.
No historical message backfill or connection-wide export is provided.

Messages are unmasked, reassembled and permessage-deflate decoded before storage. Complete messages
up to 16 MiB are buffered then appended. In passive capture, larger messages retain metadata with
`omitted_reason=size_limit_exceeded`; traffic and later message capture continue. Active interception
rejects oversized or invalid messages and closes the connection, since incomplete payloads cannot
be safely processed by callbacks. Ping/Pong
are not records; Close updates connection metadata. Connections without WS callbacks retain passive forwarding. Connections with callbacks use
message-level interception before forwarding; they support permessage-deflate input, including
context takeover even when a message is dropped. Outgoing frames are uncompressed with RSV1 clear,
which is valid under permessage-deflate negotiation and avoids corrupting peer compression state. Dropped messages never reach the destination;
unsent/failed messages are recorded with reasons. Callback errors cancel only that invocation's
pending sends/drop and pass the original message to the next callback. HTTP/2/3 extended CONNECT is out of scope.

Internal format v1: eight-byte magic `PCWS\x01\0\0\0`, followed by little-endian u32 message ID,
u32 record length, u32 metadata length, UTF-8 JSON metadata, and payload bytes.
`record_length = 4 + metadata_length + payload_length`; total record bytes are `8 + record_length`.
Readers only publish complete records; incomplete tails do not become empty messages. No length backfill.

The dedicated WebSocket UI uses a compact 84px Canvas waterfall (85px including border) with two directional lanes,
viewport-sized high-DPI rendering, Command/Ctrl + wheel pointer-anchored horizontal zoom,
scroll, and message selection. The time row uses adaptive, evenly spaced 1/2/5 time ticks with short vertical marks on its bottom divider;
compact icon toolbars replace text buttons, without a zoom slider, scale ruler, or reserved scrollbar space. Handshake/connection
IDs remain available in the API. WS details show source and direct-trigger links (including across
lanes), generating/dropping callback invocation IDs, send ordinal, delivery failures and callback
errors. Connection source buttons and callback links open the historical handshake script snapshot
through the existing content API; these controls require the corresponding backend capability.
Canvas fill/border is green for original messages and blue for generated messages. Dropped messages
have gray fill and unsent/failed messages red fill; borders always retain origin color, including
when selected. Selection increases border thickness.

Connection metadata and icon actions share one toolbar. Per-direction message counts appear
as a second, smaller line in the left C→S/S→C legends; direction labels align with Canvas bars.

WebSocket messages refresh automatically while the connection is active; there are no manual
follow/refresh buttons. At full extent (no horizontal overflow), incoming data keeps the timeline
fitted. Zoomed views keep their scale and scroll position; each lane shows a right-edge pill for
unseen new messages. Clicking it reveals that direction’s newest message and clears its unread
count without changing zoom. Returning to full extent resumes continuous fitting.

Zoom is absolute: 1× means 100 CSS pixels per second, capped at 64× (6.4 px/ms,
100px = 15.625ms), independent of the captured time range. Full-extent fitting also respects
this cap; a short capture may leave unused space to the right.

## Resource manager UI capabilities

The workspace resource manager requires `listAssets`; its upload actions and those in the replay
asset picker require `uploadAsset`. HTTP upload uses the existing raw-body POST asset endpoint.
The desktop backend exposes `upload_asset` over binary Tauri IPC using `x-asset-id` and
`x-asset-content-type` headers; it uses the same asset store and conflict rules without HTTP.
The shared single-file picker does not upload on selection: users confirm the destination name.
Asset IDs allow lowercase ASCII letters, digits, `_`, `-`, `.`, and `/`; maximum length is 255
bytes, each segment at most 100 bytes, and empty, `.`, `..`, `.metadata` segments are invalid.
No overwrite, move, rename, delete or empty-directory API has been introduced.
