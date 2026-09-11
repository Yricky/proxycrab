# ProxyCrab Backend API

## Shared management contract

HTTP handlers and ordinary proxy Tauri commands call the same `ProxyCrabManager` trait and use the
same request/response DTOs. Trusted host commands such as workspace selection remain Tauri-local.
Except for raw `GET /api/agents.md`, body reads, and raw Asset downloads,
HTTP successes use:

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
Direct Tauri calls are trusted desktop operations and do not pass through HTTP authentication.
`ProxyCrabManager` remains unaware of credentials; the HTTP Router accepts a separate mandatory
permission-manager implementation.

The exposed operations are split into four boundaries: trusted host operations (Tauri commands or
explicit local CLI actions), permission-controlled `/api/*` proxy operations, token-scoped read-only
`/share-api/*` operations, and target-private `/ui-api/*` browser operations. Host operations include
workspace switching, full config replacement, CA regeneration, and desktop Skill path management.
Desktop Skill paths are application-data-owned and synchronize the bundled Skill at startup, on
save, and on explicit manual sync; these operations are not part of `ProxyCrabManager`. Workspace
selection has no management HTTP resource; read-only config and CA resources remain on `/api/*`.

## Management HTTP authentication and permissions

The management server binds `0.0.0.0:18089`. A request may omit `Authorization` only when its TCP
peer is loopback, its Host is local, and its Origin is absent or local; it then uses the workspace's
independently configurable `本机无 API Key` (`LocalLoopback`) identity. Missing connection metadata
fails closed.
An API key is accepted only through exactly one `Authorization: Bearer <key>` header. `X-API-Key`,
query-string credentials, duplicate Authorization headers, malformed schemes, and unknown or
deleted keys are rejected. A valid Bearer header always selects the Bearer identity, even on
loopback. API-key management itself is target-private and is not exposed through `/api/*`.

Each identity has one persisted permission for every HTTP method and route template:

- `allow` executes immediately;
- `approval` holds the complete original request for up to 30 seconds while the desktop asks;
- `deny` returns `403 permission_denied`.

Desktop approval supports allow or deny once, or for 5 minutes, 30 minutes, or 1 hour. A timed
decision applies to the same identity plus route action, settles matching pending requests, stays
in memory only, and disappears on restart. No decision within 30 seconds returns
`403 approval_timeout`. Closing the approval window does not cancel pending requests.

API keys and their permission tables follow the active workspace in
`http_api_permissions.json`. Full keys are shown only once; the file stores a salted SHA-256 digest
and is owner-only on Unix. A corrupt or unreadable permissions file prevents the HTTP service from
starting and never falls back to allow-all. Existing requests already waiting for approval are not
recomputed when permissions change or a key is deleted.

Public CLI UI and read-only Session assets can load through a non-loopback address, but every remote
`/api/*` request must include `Authorization`. Remote CORS
preflight is accepted only when it requests the `Authorization` header; the authenticated request
is then evaluated by the normal UI-token or API-key permission path. This preserves anonymous local
access without exposing anonymous management access remotely.

### CLI browser authentication and target capabilities

When the CLI starts its management server, it also prints a 256-bit, per-run `pcrab_ui_…` token.
The browser landing page verifies the token through `GET /ui-api/bootstrap`, installs an HTTP
Backend in `window.proxyCrabBackend`, and then mounts the shared application. The cleartext token is
not written to the workspace; the CLI stores only a SHA-256 digest in memory.

The token protects target-private `/ui-api/*` routes for bootstrap, long-poll UI changes, local IP
and regex helpers, Agent preset management, and API-key/permission management. It is also accepted
as a trusted credential for the shared `/api/*` resources so the UI remains usable even when the
ordinary local identity denies an action. These routes and this token are UI internals, not public
agent API credentials.

The CLI permission editor offers only `allow` and `deny`. Existing persisted `approval` entries are
displayed and evaluated as `deny`, and CLI updates reject `approval`. The Tauri target retains its
approval workflow. The CLI workspace path is read-only in the UI because `--workspace` selects it
at process startup.

The CLI Backend intentionally does not define a Skill path manager, and the CLI HTTP router has no
Skill-management endpoint. CLI Skill installation remains a local, explicit `install-skill`
subcommand; desktop path management remains a trusted Tauri operation. Neither can be initiated by
the browser bundle, which may be hosted remotely.

### Read-only Session sharing

Both trusted targets can inspect, enable, and disable one in-memory share per Session. Tauri uses
`get_session_share`, `enable_session_share`, and `disable_session_share`; HTTP clients, including the
CLI browser, use permission-controlled `GET /api/session-shares/{id}`, `POST /api/session-shares`,
and `DELETE /api/session-shares/{id}`. Enable accepts only `session_id`, is idempotent, and returns
the same cleartext `pcrab_share_…` token while enabled. The token and its SHA-256 digest are retained
only in process memory so the owner UI can reopen the same links; disable removes them immediately.

The browser bundle serves `/session?token=…`, plus an optional `id` query parameter that focuses the
matching record (scrolls the list to that row and opens its inline detail panel). The shared frontend
output uses `index.html` as the
application entry and `session.html` as the dedicated read-only sharing entry; both reuse the same
emitted chunks. Tauri and CLI consume this same output, while the application entry selects its
Landing from the runtime host. In Tauri, the Session entry and assets
come from the same embedded `frontendDist` as the desktop WebView through Tauri's asset resolver;
the desktop binary does not embed a second CLI bundle. Every `/share-api/*` request must carry
exactly one non-empty `token` query parameter; `Authorization` is ignored. Responses set
`Cache-Control: no-store`, and the page uses `Referrer-Policy: no-referrer`. The server derives the
Session scope on every request and overwrites any client Session ID. The surface
contains bootstrap, scoped proxy status and proxy-change long polling, Session view, stateless log-ID queries, rendered log rows, detail,
body, name-only column/filter lists, and regex validation. It has no write, export, interceptor,
breakpoint, config, permission, routing, CA, bypass, or system-log route. Disable or process restart
invalidates it. An archived Session is unavailable; restoring it in the same process preserves its
still-enabled share. The token is not accepted by `/api/*` and is not an Agent credential.

## HTTP changes and desktop UI synchronization

Successful HTTP operations that mutate application-visible state and runtime proxy lifecycle or
activity transitions publish an internal change event. Tauri also subscribes to runtime status
directly, so activity updates remain available when the management HTTP service cannot start; the
CLI browser receives them through its private long-poll route. The frontend coalesces adjacent
events and refreshes only affected resources. During `stopping`, it retains the final running
activity snapshot until `stopped`, so final filtering and row/detail hydration happen after drain
has persisted terminal capture state. The read-only share page uses an authenticated,
revision-based proxy-change long poll and then reloads its Session-scoped status.

| HTTP operation | UI resources affected | Desktop behavior |
| --- | --- | --- |
| Proxy lifecycle or active netlog/bypass transition | Proxy status | Refreshes lifecycle controls and exact activity badges |
| Session create/update/archive/restore | Active and archived Session lists | Archiving the viewed Session selects the first remaining Session |
| Archived Session delete | Archived Session list | Refreshes the archived Session window |
| `PUT /api/active-session` | Active Session and settings | Refreshes the active indicator without changing the viewed Session |
| Session share enable/disable | Target Session share state | Refreshes an open “导出和分享” window; enabling refreshes all such windows because the Session ID is carried in the request body |
| HAR share enable/disable | Target HAR share state | Refreshes the independent HAR card in an open “导出和分享” window |
| Routing-script create/update/delete/selection | Routing library and selection | Refreshes the routing manager |
| Bypass delete/batch delete/clear | Bypass table | Refreshes the bypass window |
| `PUT /api/sessions/{id}/filter` | Target Session filter and visible log set | Reloads the table only when that Session is being viewed; ID queries stay silent |
| `PUT /api/session-view` | Target Session columns | Reloads the table only when that Session is being viewed |
| Column-script create/update/delete | Column/filter choices and rendered custom columns | Refreshes script lists and the current table view |
| Filter-script create/update/delete | Filter choices and filtered results | Refreshes script lists and the current table view |
| Interceptor create/update/delete | Global interceptor library and Session chains | Refreshes the library and current pipeline |
| `PUT /api/session-interceptors` | Target Session pipeline | Refreshes the pipeline only when that Session is being viewed |
| `DELETE /api/system-logs` | System-log viewer | Clears and reloads an open log window |

If HTTP archives the viewed Session, the frontend selects the first remaining Session.

The log table separates ID membership from row rendering. On every Session switch or filter
change, it captures the newest unfiltered ID as a fixed upper bound, pages every matching ID up to
that bound, then resumes incremental unfiltered discovery from its frontend-only `max_id`. With a
filter, newly discovered IDs and `active_netlog ∪ in_progress_ids` candidates are re-evaluated
through the same read-only ID endpoint; leaving `active_netlog` triggers one final filter and row
refresh. The virtual table requests row values only for the visible window plus 500 rows on each
side, in batches of 200, and retains at most 5,000 non-protected rows for the current Session.

Open script and settings windows automatically reload when clean. If they contain unsaved input,
the frontend preserves it, displays an external-change warning, and lets the user explicitly reload
instead of overwriting local edits.

## HTTP resources

The server listens on every IPv4 interface at `0.0.0.0:18089` and applies the authentication
and permission policy above. Local requests accept loopback/`localhost` Host values and local
browser origins, including `tauri://localhost`. A non-local Host or Origin requires Bearer
authorization, preventing anonymous DNS-rebinding and CSRF access while supporting the remote CLI
browser UI.

| Resource | Operations |
| --- | --- |
| `/api/agents.md` | `GET` active workspace Agent instructions as raw `text/plain` |
| `/api/config` | `GET` |
| `/api/proxy/status` | `GET` |
| `/api/proxy/start`, `/api/proxy/stop` | `POST` |
| `/api/sessions` | `GET`, `POST` |
| `/api/sessions/{id}` | `PUT` |
| `/api/sessions/{id}/filter` | `PUT` save only the Session filter |
| `/api/sessions/{id}/archive` | `POST` inactive Session archive |
| `/api/archived-sessions` | `GET` archived metadata |
| `/api/archived-sessions/{id}/restore` | `POST` restore |
| `/api/archived-sessions/{id}` | `DELETE` permanent archived-only deletion |
| `/api/active-session` | `GET`, `PUT` nullable active Session |
| `/api/session-shares` | `POST` idempotently enable a scoped read-only link (default: `approval`) |
| `/api/session-shares/{id}` | `GET` current state (default: `allow`); `DELETE` disable immediately (default: `approval`) |
| `/api/session-har-shares` | `POST` idempotently enable a frozen HAR download link (default: `approval`) |
| `/api/session-har-shares/{id}` | `GET` current state (default: `allow`); `DELETE` disable immediately (default: `approval`) |
| `/api/assets` | `GET` list every Asset's metadata sorted by id |
| `/api/assets/{id}` | `POST` immutable raw upload; `GET` metadata or raw bytes with `format=raw` |
| `/api/replay?session={id}` | `POST` send a fully specified request through one Session's pipeline (source `ProxyCrabRequest`) |
| `/api/logs/ids` | `POST` bounded/filterable log ID query |
| `/api/logs/views` | `POST` batch incremental table-view rendering |
| `/api/logs/{id}` | `GET` complete log detail |
| `/api/logs/{id}/body` | `GET` raw stored or streaming-decoded request/response body |
| `/api/logs/{id}/interceptors/{execution_id}/content`, `/snapshot` | `GET` one execution's executed source or entry snapshot |
| `/api/breakpoints` | `GET` active breakpoints for one Session |
| `/api/breakpoints/{id}` | `GET` live detail at the paused interceptor |
| `/api/breakpoints/{id}/body` | `GET` raw stored or streaming-decoded live body |
| `/api/breakpoints/{id}/extend`, `/release`, `/execute` | `POST` breakpoint controls |
| `/api/session-view` | `GET`, `PUT` whole-session table view |
| `/api/session-interceptors` | `GET`, `PUT` whole-session interceptor chains |
| `/api/column-scripts`, `/api/column-scripts/{name}` | script CRUD |
| `/api/filter-scripts`, `/api/filter-scripts/{name}` | global filter-script CRUD |
| `/api/filter-scripts/{name}/debug` | `POST` debug against one log and input |
| `/api/routing-scripts`, `/api/routing-scripts/{name}` | global routing-script CRUD |
| `/api/routing-script-selection` | `GET`, `PUT` nullable current selection |
| `/api/interceptors`, `/api/interceptors/{kind}/{name}` | global interceptor script CRUD |
| `/api/bypass`, `/api/bypass/{id}`, `/api/bypass/delete` | query/delete transparent forwarding metadata |
| `/api/ca` | `GET` |
| `/api/system-logs` | `GET`, `DELETE` |

System logs accept `after_seq` and are capped at 10,000 entries.

## Workspace Assets

Assets are immutable files shared by every Session and Lua interceptor in the active workspace.
`POST /api/assets/{id}` streams the request body into a new Asset. Send its media type in
`Content-Type`; omission stores `application/octet-stream`. A successful upload returns HTTP 201
with the normal envelope and metadata:

```json
{
  "ok": true,
  "data": {
    "id": "fixtures/example.json",
    "size": 13,
    "content_type": "application/json",
    "sha256": "...lowercase hex...",
    "created_at": 1785380000000
  }
}
```

`GET /api/assets` lists every Asset's metadata (`id`, `size`, `content_type`, `sha256`,
`created_at`) sorted by id inside the normal envelope; build trees from the `/`-separated ids.

`GET /api/assets/{id}` returns the same metadata. `GET /api/assets/{id}?format=raw` streams the
stored bytes without the JSON envelope and sets `Content-Type`, `Content-Length`,
`Content-Disposition`, and `X-ProxyCrab-Asset-SHA256`. No listing, update, or delete operation is
provided. Reusing an ID returns `409 asset_already_exists`; a file/directory hierarchy collision
returns `409 asset_path_conflict`.

IDs allow only lowercase ASCII letters, digits, `_`, `-`, `.`, and `/`. They cannot start or end with
`/`, contain `//`, or contain `.`, `..`, or `.metadata` as a complete path segment. The total limit
is 255 bytes and each segment is limited to 100 bytes. Invalid IDs return
`400 invalid_asset_id`; unsupported `format` values return `400 invalid_asset_format`; missing
Assets return `404 asset_not_found`; storage failures return `500 asset_store_failed`. Uploads are
stored below `<workspace>/assets`, with sidecar metadata below `assets/.metadata`. SHA-256 is
computed while uploading; manually changing workspace files later is outside the API contract.

## Workspace Agent instructions

The manager initializes workspace-scoped Agent presets when `agents/config.json` is absent:

```text
agents/config.json
agents/presets/full-capability.md
agents/presets/quiet-investigation.md
```

`充分使用能力` is active by default. The desktop AI menu manages, edits, deletes, activates, and
reimports presets through Tauri commands. Reimport overwrites same-name defaults without changing
the active preset. At least one preset must remain; deleting the active preset selects the next
available item.

`GET /api/agents.md` reads the active Markdown file from disk for every request and returns its raw
UTF-8 content with `Content-Type: text/plain; charset=utf-8`. It does not use the JSON success
envelope and does not trigger desktop UI synchronization.

## Network log queries

Log operations accept an optional Session. POST operations accept `session_id` in their JSON body;
detail/body operations accept `session_id` in their query string. Omitting it selects the active
Session. If no Session is active, the operation returns `409 conflict`.

### Query log IDs

`POST /api/logs/ids` accepts:

```json
{
  "session_id": 1,
  "filter": {
    "option": {
      "kind": "column",
      "column": { "kind": "uri" },
      "regex": false
    },
    "input": "example.com"
  },
  "ids": null,
  "min_id": 100,
  "max_id": 10000,
  "limit": 10000
}
```

Every field is optional. The endpoint is always read-only. Omitting `filter`, using `option: null`,
or using an empty `input` matches all logs. A supplied filter is evaluated only for this request and
never changes the Session's saved filter.

A column option supports `method`, `uri`, `code`, `source`, `stage`, or `{ "kind": "script", "script_name": "..." }`. With `regex: false`, built-in and custom-column output use case-sensitive contains matching. With `regex: true`, `input` uses Rust `regex` syntax and substring matching unless the pattern is anchored; inline flags such as `(?i)` control case folding. Invalid patterns return `400 bad_request`. A script option has the form `{ "kind": "script", "script_name": "..." }` and passes `input` to that global Lua filter script. Custom-column and filter-script execution errors silently count as non-matches.

`min_id` and `max_id` are exclusive (`id > min_id && id < max_id`). `ids` re-evaluates an explicit
candidate set and cannot be combined with range bounds. The default and maximum range page size,
and the maximum explicit candidate count, are 10,000.

When only `min_id` is supplied, the database scans toward newer IDs. When `max_id` or neither bound is supplied, it scans toward older IDs. Callers page in either direction by passing the relevant edge ID from their current list. Response order is intentionally unspecified:

```json
{
  "matched_ids": [9999, 9998],
  "in_progress_ids": [9999]
}
```

The arrays may overlap: matching controls display membership, while `in_progress_ids` contains only
records that are both persisted as in-progress and present in the current run's authoritative
`active_netlog`, telling clients which candidates may still change. Stale unfinished records from
an earlier proxy run are never returned there. For filtered range
queries, the server scans until it has collected `limit` matches or exhausted the bounded range;
therefore clients can paginate with the smallest returned match as the next exclusive `max_id` and
stop when a page contains fewer than `limit` matches. When an ID leaves `in_progress_ids`, clients
perform one final explicit-ID evaluation and use its final `matched_ids` membership.

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

The response columns and cells do not contain the ID column; `row.id` is a separate field which the frontend displays as its fixed first column. `columns` are the Session view's raw `Column` entries (kind/width, plus `script_name` for custom-column scripts); the frontend derives display metadata (label, key) from them:

```json
{
  "columns": [
    { "kind": "method", "width": 50.0 },
    { "kind": "created_at", "width": 200.0 },
    { "kind": "updated_at", "width": 200.0 }
  ],
  "rows": [
    {
      "id": 124,
      "created_at": 1720000000000,
      "updated_at": 1720000000100,
      "outcome": "success",
      "cells": ["GET", "1720000000000", "1720000000100"]
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

`column_index` is zero-based and aligns with `columns` and `cells`. Every returned row includes its immutable capture creation time and persisted `outcome` (`in_progress`, `success`, `failed`, or `tunneled`). A custom-column error leaves that cell empty while preserving the rest of the row. A missing log has no row and no `column_index`. Unchanged logs appear in neither `rows` nor `exceptions`.

Built-in table column kinds are `method`, `uri`, `code`, `source`, `stage`, `created_at`, and
`updated_at`. The two time-column cells are decimal Unix-millisecond strings so the existing
string-cell protocol remains unchanged; the numeric `row.created_at` and `row.updated_at` fields
remain the canonical frontend values. The desktop formats them in the computer's current timezone
as `YYYY-MM-DD HH:mm:ss GMT±HH:mm`. Time columns are not filter options.

`updated_at` is a monotonic Unix-millisecond version for the complete log. Metadata transitions and request/response body writes advance it, even when multiple updates happen in the same wall-clock millisecond.

### Share logs as HAR

HAR sharing is independent from the read-only Session page share. Both trusted targets expose
`get_har_share`, `enable_har_share`, and `disable_har_share`; HTTP management clients use
permission-controlled `GET /api/session-har-shares/{id}`, `POST /api/session-har-shares`, and
`DELETE /api/session-har-shares/{id}` with the same allow/approval/approval defaults as Session page
sharing. Enable accepts a strict body:

```json
{
  "session_id": 1,
  "scope": "filtered",
  "log_ids": [123, 124]
}
```

`scope` is `all` or `filtered` and records which option the owner selected. `log_ids` is the exact
ID snapshot collected by the UI through the existing log-ID query. The service deduplicates and
sorts it, then retains it with an independent `pcrab_har_…` token in process memory. Re-enabling an
existing share is idempotent and preserves its original scope, IDs, and token. State responses
include `scope` and `log_count` while enabled. Disable immediately invalidates the old token.

The owner UI produces one `http://<local-ip>:<api-port>/session.har?token=…` link per usable local
IPv4 address. `GET /session.har` authenticates only that query token, checks that the Session remains
active, and generates the HAR from the frozen IDs. An empty array creates a valid HAR with no
entries. IDs are emitted in ascending order; if a frozen ID no longer exists in that Session, the
download returns `404 log_not_found`.

Only captures with `outcome: success` and a response are eligible. Synthetic `CONNECT` captures at
the `tls_mitm` stage are skipped; ordinary HTTP responses and `101` upgrade handshakes remain
eligible. Ineligible frozen IDs are silently skipped. The download generates the complete file
as an entry-granular stream after validating the Session and frozen IDs. Each entry is loaded,
serialized, and released before the next one is generated, and the response has no `Content-Length`.
An error before streaming starts returns the normal structured HTTP error; a body or storage error
after HTTP 200 interrupts the connection and leaves a partial, invalid HAR. There is no
application-level body or export-size limit.

The successful response is the raw UTF-8 HAR 1.2 JSON document. Headers include:

```text
Content-Type: application/json; charset=utf-8
Content-Disposition: attachment; filename="proxycrab-session-1.har"
```

Request and response bodies are complete. gzip, br, deflate, zstd, and stacked content encodings
are decoded first. Textual content is written as text; binary response content uses HAR
`encoding: "base64"`, while binary request `postData` uses `_encoding: "base64"`. Unsupported or
malformed content encoding falls back to raw Base64 and sets the corresponding
`_proxyCrab.requestBodyDecoded` or `_proxyCrab.responseBodyDecoded` field to `false`.
`request.bodySize` and `response.bodySize` are captured stored byte counts; `response.content.size`
is the exported decoded/fallback byte count. Header sizes and all detailed timings are `-1`, while
entry `time` is `updated_at - created_at`.

Each entry includes `_proxyCrab` with the log/Session IDs, request tags, stage, client source
address, and original millisecond timestamps. Interceptor source and execution history are not
embedded. Query parameters, redirects, and cookies are populated on a best-effort basis while the
original headers are always retained.

HAR generation performs no redaction. Authorization headers, Cookie/Set-Cookie values, and request and
response bodies can contain credentials or personal data; treat the file as sensitive.

### Read one complete log

`GET /api/logs/{id}?session_id=1` returns request/response metadata and bodies, error/outcome state,
plus `created_at`, `updated_at`, request `tags`, and ordered `request_interceptors` /
`response_interceptors` execution arrays. Every execution contains a unique `execution_id`,
`origin` (`saved` or `temporary`), `completed`, the historical script name, phase, zero-based
position, SHA-256 hash, `has_snapshot`, its own non-snapshot modifications, and an optional runtime
error. The exact source and the entry snapshot are not embedded: `GET
/api/logs/{id}/interceptors/{execution_id}/content` returns the UTF-8 source as `text/plain` with an
`X-ProxyCrab-Script-SHA256` header, and `GET /api/logs/{id}/interceptors/{execution_id}/snapshot`
returns the captured request/response state (`404 snapshot_not_found` when `has_snapshot` is false).
Request-line changes use `method_set` with `method` and `uri_set` with `uri`, status changes use
`status_set` with `status`, and tag changes use `tag_set` with `key` and `value`. Scripts that
executed without changes are still present.
Disabled and missing scripts are not recorded.

The persisted tag map may include `_crab_skip`, `_crab_req_speed`, `_crab_resp_speed`,
`_crab_req_timeout`, `_crab_resp_bodyframe_timeout`, and `_crab_tls_insecure`. These are proxy-local controls rather than HTTP
headers. For ordinary captured
HTTP/HTTPS traffic, positive ASCII decimal speed values pace final outbound body bytes per second,
and `_crab_req_timeout` overrides the 60,000 ms upstream timeout. Invalid final values are ignored
with a runtime warning. `_crab_resp_bodyframe_timeout` is a positive millisecond idle timeout from
response headers to the first upstream body frame and between later frames. The exact final value
`_crab_tls_insecure=true` disables upstream HTTPS
certificate-chain and hostname verification for that request, including Upgrade/WebSocket; other
values and HTTP requests retain normal verification behavior. Verified and insecure HTTPS
connections use separate pools. Bypass, raw CONNECT, Upgrade/WebSocket, the local CA endpoint, and
proxy-generated errors retain their existing unpaced behavior.

Log detail embeds decoded text/JSON only through 64 KiB. `captures.req_modifications` and
`resp_modifications` each store a `final_body` source: `original`, `string` with complete `content`,
or `asset` with `asset_id`. This is the complete Body constructed after interceptors and attempted
for outbound delivery, not the number of bytes delivered before a network interruption. The
`blob/` directory stores only original network request and response bodies; ProxyCrab never creates
`.modified` files.

`GET /api/logs/{id}/body?session_id=1&side=request&max_size=16777216` resolves that final source and
negotiates its response from `Accept-Encoding`. `max_size` applies to the selected stored file or
replacement size before decoding or recompressing. If the client accepts every captured content
encoding, the original bytes and encoding stack are preserved. Otherwise ProxyCrab streams one
decode pass and falls back in fixed `gzip`, `deflate`, then identity order; nonzero q weights do not
reorder that preference, while `q=0` forbids an encoding. Responses include
`Vary: Accept-Encoding`. An empty original request or response has no blob file and the endpoint
returns a successful zero-byte stream.

The default stored-file limit is 16 MiB and there is no server maximum. Transcoded and identity
responses omit decoded `Content-Length`; original responses retain the stored length. Adding
`execution_id` reads the selected `original`, `string`, or `asset` interceptor-entry snapshot Body
for that log. The desktop snapshot window intentionally displays only the Asset ID for Asset
snapshots. Active breakpoints expose the same final-body contract at
`GET /api/breakpoints/{id}/body` and include current replacements for the paused phase.

### Replay a request

`POST /api/replay?session=1` sends a fully specified request through the target Session's
interceptor pipeline and outbound chain, recording it as a normal capture whose source is
`ProxyCrabRequest`. The proxy must be running (`proxy_not_running` otherwise). The response
`{ "log_id": 12 }` returns as soon as the capture is created, without waiting for the upstream
exchange. Request body: `method`, absolute `url`, ordered `headers` pairs (sent as-is, including
`content-length`), and an optional `body` of `{ "type": "text", "text", "charset": "utf8" }`,
`{ "type": "body_ref", "session_id", "log_id", "side": "request" | "response" }` (reuse a
stored capture blob body), or `{ "type": "asset", "asset_id" }` (reuse a workspace Asset).

## Session table views

`GET /api/session-view?session_id=1` returns:

```json
{
  "session_id": 1,
  "columns": [
    { "kind": "method", "width": 50.0 },
    { "kind": "uri", "width": 300.0 }
  ],
  "filter": {
    "option": null,
    "input": ""
  }
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

The ID column is not part of the view model. `PUT` changes only `columns` and preserves the Session
filter. Built-in column kinds are `method`, `uri`, `code`, `source`, `stage`, `created_at`, and
`updated_at`. Column widths must be positive. A new/updated view cannot reference a missing global
column script. Deleting a referenced script removes matching table columns and resets filters that
reference it. Script names cannot be changed.

`PUT /api/sessions/1/filter` independently saves only the Session filter and preserves columns:

```json
{ "option": { "kind": "column", "column": { "kind": "uri" }, "regex": false }, "input": "/api" }
```

New Sessions always use the fixed built-in method, URI, status-code, and source columns plus an
empty filter. The first Session created in an empty workspace becomes active. Later creation does
not replace or restore an active Session.

Inactive Sessions can be archived while the proxy runs with
`POST /api/sessions/{id}/archive`. The whole directory moves to `sessions_archived/<id>`; archived
Sessions disappear from all existing Session, log, view, interceptor, and export APIs. The active
Session and Sessions with pinned requests return `409 conflict`. `GET /api/archived-sessions`
lists metadata only, `POST /api/archived-sessions/{id}/restore` moves it back without activating
it, and `DELETE /api/archived-sessions/{id}` permanently removes it. Permanent deletion therefore
requires archiving the Session first.

## Global filter scripts

Filter scripts are stored globally under `scripts/filter`. CRUD follows the same `ScriptRequest` and `UpdateScriptRequest` shapes as custom-column scripts:

```text
GET    /api/filter-scripts
POST   /api/filter-scripts
GET    /api/filter-scripts/{name}
PUT    /api/filter-scripts/{name}
DELETE /api/filter-scripts/{name}
```

Updates replace only script content. Deleting a referenced filter script, or deleting a
custom-column script used as a filter target, resets those Session filters to no selection.

## Routing scripts and active Session

`AppConfig.routing_script_name` stores an optional selected routing script. CRUD lives under
`/api/routing-scripts`; `PUT /api/routing-script-selection` accepts `{"name":"route"}` or
`{"name":null}`. There is no rename or debug endpoint.

For direct HTTP, routing runs per request. For HTTPS, it runs once on CONNECT and pins the selected
Session for the whole tunnel. Lua returns `true` to capture into the current active Session and
`false` or `nil` to bypass. If `true` is returned while no Session is active, the request bypasses.
Invalid returns and runtime errors emit a system warning and bypass. With no selected script, the
active Session captures all traffic; with no active Session, traffic bypasses.

`GET /api/active-session` returns `{"session_id":1}` or `{"session_id":null}`. `PUT` accepts the
same shape, validates non-null IDs, and is allowed while the proxy runs. `AppConfig.active_session_id`
is read through `GET /api/config`; changing it over HTTP uses `PUT /api/active-session`. An actual
active-Session change closes every
established HTTP connection, CONNECT/MITM tunnel, Upgrade/WebSocket, transparent tunnel, and
upstream pool before the call returns while leaving the listener running. Changing routing
selection, changing selected routing content, or deleting the selected rule uses the same reset;
identical values and unselected routing-script edits do not.

`POST /api/filter-scripts/{name}/debug` evaluates one script without suppressing errors:

```json
{
  "session_id": 1,
  "log_id": 123,
  "input": "example.com"
}
```

The success payload is a boolean. Runtime errors and non-boolean results return `bad_request`.

## Session interceptors

Interceptor Lua files are global and separated into request and response libraries.
`GET /api/interceptors?kind=request` (or `response`) returns the scripts and the number of Sessions
referencing each script. CRUD under `/api/interceptors/{kind}/{name}` changes this global library.
Updates replace only content. Application-driven deletion removes every Session reference; a file
removed directly from disk leaves its references in a missing state.

`GET /api/session-interceptors?session_id=1` returns:

```json
{
  "session_id": 1,
  "request": [
    { "name": "set-env", "enabled": true, "valid": true }
  ],
  "response": [
    { "name": "missing-script", "enabled": true, "valid": false }
  ]
}
```

`PUT /api/session-interceptors?session_id=1` atomically replaces both chains:

```json
{
  "request": [
    { "name": "set-env", "enabled": true }
  ],
  "response": []
}
```

Each side may contain at most 12 unique names. The API rejects duplicates and oversized chains with `bad_request`. Missing names are accepted so references remain editable if files are changed outside the application.

New Sessions start with empty chains. At the beginning of every routed request, the proxy pins the
selected Session and snapshots the name and exact UTF-8 content of every enabled, present script.
The response stage uses the same snapshot even if the Session, chain, or source files change in the
meantime. Missing and disabled nodes are skipped.

Executed content is stored in the Session capture database by lowercase SHA-256.
`interceptor_script_contents` stores one copy of each unique source, while
`capture_interceptor_runs` links captures to ordered executions and per-script modifications. An
execution that changes Method, URI, Status, Headers, or Body starts its modifications with a full
entry snapshot. Request snapshots contain Method, URI, Version, Headers, and Body; response
snapshots contain Status, Version, Headers, and Body. Tags and the response script's read-only
request are excluded. Snapshot Body sources are `original`, complete `string`, or `asset_id`.
Executions with no modifications or only Tag changes do not store a snapshot. Log detail omits that
snapshot entry and the HTTP API exposes it only through the dedicated snapshot endpoint.

## Active interceptor breakpoints

`GET /api/breakpoints?session_id=1&phase=request&interceptor_name=set-env` lists only active,
in-memory breakpoints for the selected Session; `phase` and `interceptor_name` are optional.
`GET /api/breakpoints/{id}` returns `{ "breakpoint": ..., "log": ... }`, where `log` is overlaid
with the current live headers, body replacement, and tags at the paused point.

Controls are:

- `POST /api/breakpoints/{id}/extend` with `{ "timeout_ms": 60000 }`; cumulative requested wait is
  silently clipped to 1,800,000 ms.
- `POST /api/breakpoints/{id}/execute` with `{ "content": "..." }`; applies mutations, stores one
  independent temporary execution, and keeps the breakpoint paused even when Lua reports an error.
- `POST /api/breakpoints/{id}/release`; resumes saved-script execution after `breakpoint()`.

Breakpoints automatically release when their deadline expires. They are process-local and are not
restored after restart.

## Capture lifecycle

While the listener is running, `GET /api/proxy/status` includes the runtime-authoritative activity
snapshot:

```json
{
  "status": "running",
  "host": "0.0.0.0",
  "port": 8089,
  "started_at": 1785380000000,
  "active_netlog": { "1786333525006": [1, 2, 3, 4, 5] },
  "active_bypass_count": 2
}
```

`active_netlog` keys are Session IDs and its values are exact active capture IDs. This map is the
only source for deciding whether a persisted netlog row is currently active; `outcome` remains the
persisted result and `stage` remains its execution/failure phase. A share-token status response
retains only the authorized Session key and omits `active_bypass_count`.

A routed HTTP request is inserted into its selected Session database before it is forwarded. The
Session is pinned for the entire request/response lifecycle even if the active Session changes.

Capture outcomes are:

- `in_progress`
- `success`
- `failed`
- `tunneled`

Errors contain a stable `kind`, an execution `stage`, and the underlying message. A provisional `CONNECT` row is inserted before the proxy acknowledges the tunnel, so even an idle connection is represented. TLS failures update that row. After a successful TLS handshake, the row is retained and finalized as `success` at stage `tls_mitm` with an HTTP 200 response; decrypted requests are stored as additional captures.

If the initial database insert fails, traffic is not forwarded. Starting or stopping the proxy does not rewrite persisted outcomes. When the proxy stops it stops accepting immediately and waits up to five seconds; a connection generation that must be drained or aborted still marks only its own unfinished rows with `proxy_shutdown`.

Request and response interceptors run once at their respective header boundary. Body getters may
wait for the original body to finish downloading and being captured, after which the unmodified
body is replayed from disk. Without a getter, original request and response bodies continue
streaming directly to append-only capture files without an application-level size limit.
Replacement strings are sent from memory and immutable Assets are streamed from disk while any
unused raw body drains independently. Capture-storage failures are recorded without interrupting
the business transfer when possible. The proxy accepts at most 256 client connections.

`_crab_resp_bodyframe_timeout` optionally bounds idle time before each upstream response-body frame.
For an unmodified response, expiry terminates the downstream stream and fails the capture. If a
replacement response is already being returned, raw-body timeout/read/storage failures only leave
diagnostic error metadata; the replacement response and successful capture outcome are retained.

Capture/bypass storage queries and management-side Lua evaluations run outside Tokio worker
threads and share an eight-task concurrency limit. Additional heavy management operations wait for
a permit. Each opened capture or bypass store reuses one configured SQLite connection across its
clones and serializes access to it. Session capture databases retain SQLite WAL as their persisted
journal format.

Within one log-ID or log-view query, each referenced filter or custom-column script is compiled
once and reuses one Lua VM. Every capture evaluation still receives a fresh sandbox environment,
instruction budget, and JSON warning count, so globals and standard-library table changes do not
carry between rows or scripts.
Filter and custom-column scripts can lazily read the effective persisted request/response body via
the read-only `entry.req.body` and `entry.resp.body` getters. Decoded bytes are cached per capture
within the query, while bodies on `in_progress` captures remain unavailable to prevent matches on
partial files. See `lua-api.md` for text, encoding, and size-limit semantics.

Bypassed metadata is persisted in `<workspace>/bypass.db` without headers or bodies. Each row stores
timestamps, source, method, URI, version, routing reason, outcome, optional HTTP status/error, and
nullable upload/download byte counts. `GET /api/bypass` pages newest first; single, batch, and
deletable-record clearing are supported. A current-run `in_progress` entry cannot be deleted. An
`in_progress` entry is stale and deletable when the proxy is not running or its `created_at` is
earlier than the current `ProxyStatus.running.started_at`. Opening the database and starting or
stopping the proxy do not rewrite its outcome; generation-scoped forced draining can still mark
that generation's unfinished rows with `proxy_shutdown`.

## Workspace and CA

The workspace is locked exclusively for the process lifetime. Routing selection, active Session,
and proxy/API addresses are stored in workspace configuration. Each Session stores metadata,
its table view and filter in `sessions/<id>/view.json`, and interceptor chains in
`sessions/<id>/interceptors.json`. Archived Session directories live under
`sessions_archived/<id>` and are not opened by normal Session APIs.
The root `workspace_schema.json` is the version label for the whole workspace. Workspace open runs
the centralized, ordered migration chain before any stores are used and atomically advances the
label after each successful version. Every migration function documents that version's storage
model changes; newer unsupported labels are rejected instead of being opened.
Schema v2 changes Session capture databases from rollback journals to WAL without changing their
logical tables or blob layout. Schema v3 removes legacy header-only execution snapshots and
`body_replace_file`, records final Body sources in the capture rows, and imports every legacy
`.modified` file into `assets/mig/<session-id>/`. The original filename is preferred as the Asset
ID; collisions append `-1`, `-2`, and so on until creation succeeds. The database reference is
written before the old file is deleted, and a failed Session migration does not advance the
workspace version.
An active ID that references a missing Session is cleared and persisted when the workspace opens.

Each workspace has an independent generated CA. Missing or corrupt CA files are regenerated with a warning, the per-host certificate cache is bounded, and the CA private key is restricted to owner-only permissions on macOS/Unix. Regeneration is serialized with proxy start/stop and rejected while the proxy is running.
`http://proxy.crab/ca.crt` is always served locally without routing, Session capture, or bypass
persistence.

## System logs

The unified in-memory ring stores only:

- `seq`
- `timestamp`
- `level`
- `message`

Entries are not persisted and are consumed through incremental polling.
