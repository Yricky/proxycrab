# ProxyCrab Backend API

## Shared management contract

HTTP handlers and Tauri commands call the same `ProxyCrabManager` trait and use the same request/response DTOs. Except for raw `GET /api/agents.md` and body reads, HTTP successes use:

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

## HTTP changes and desktop UI synchronization

Successful HTTP operations that mutate application-visible state publish an internal change event
to the Tauri frontend. This event is not a public HTTP endpoint and does not change HTTP response
envelopes. The frontend coalesces adjacent events, refreshes only affected resources, and keeps
normal Agent/API activity silent.

| HTTP operation | UI resources affected | Desktop behavior |
| --- | --- | --- |
| `PUT /api/workspace` | Workspace settings | Refreshes an open clean settings window |
| `PUT /api/config` | Settings, routing selection, and active Session | Refreshes global config state |
| `POST /api/proxy/start`, `POST /api/proxy/stop` | Proxy status | Refreshes the toolbar state |
| Session create/update/archive/restore | Active and archived Session lists | Archiving the viewed Session selects the first remaining Session |
| Archived Session delete | Archived Session list | Refreshes the archived Session window |
| `PUT /api/active-session` | Active Session and settings | Refreshes the active indicator without changing the viewed Session |
| Routing-script create/update/delete/selection | Routing library and selection | Refreshes the routing manager |
| Bypass delete/batch delete/clear | Bypass table | Refreshes the bypass window |
| `POST /api/logs/ids` with a changed persisted `filter` | Target Session filter and visible log set | Reloads the table only when that Session is being viewed; `persist_filter: false` stays silent |
| `PUT /api/session-view` | Target Session columns | Reloads the table only when that Session is being viewed |
| Column-script create/update/delete | Column/filter choices and rendered custom columns | Refreshes script lists and the current table view |
| Filter-script create/update/delete | Filter choices and filtered results | Refreshes script lists and the current table view |
| Interceptor create/update/delete | Global interceptor library and Session chains | Refreshes the library and current pipeline |
| `PUT /api/session-interceptors` | Target Session pipeline | Refreshes the pipeline only when that Session is being viewed |
| `POST /api/ca` | CA manager | Reloads an open CA window |
| `DELETE /api/system-logs` | System-log viewer | Clears and reloads an open log window |

If HTTP archives the viewed Session, the frontend selects the first remaining Session.

Open script and settings windows automatically reload when clean. If they contain unsaved input,
the frontend preserves it, displays an external-change warning, and lets the user explicitly reload
instead of overwriting local edits.

## HTTP resources

The server listens on loopback by default at `http://127.0.0.1:18089`. It has no authentication and emits no permissive CORS policy. Requests must use a loopback/`localhost` Host, and browser Origin values must also be local (including `tauri://localhost`) to prevent DNS-rebinding and CSRF access.

| Resource | Operations |
| --- | --- |
| `/api/agents.md` | `GET` active workspace Agent instructions as raw `text/plain` |
| `/api/workspace` | `GET` current/configured paths; `PUT` next-start path |
| `/api/config` | `GET`, `PUT` |
| `/api/proxy/status` | `GET` |
| `/api/proxy/start`, `/api/proxy/stop` | `POST` |
| `/api/sessions` | `GET`, `POST` |
| `/api/sessions/{id}` | `PUT` |
| `/api/sessions/{id}/archive` | `POST` inactive Session archive |
| `/api/archived-sessions` | `GET` archived metadata |
| `/api/archived-sessions/{id}/restore` | `POST` restore |
| `/api/archived-sessions/{id}` | `DELETE` permanent archived-only deletion |
| `/api/active-session` | `GET`, `PUT` nullable active Session |
| `/api/logs/ids` | `POST` bounded/filterable log ID query |
| `/api/logs/views` | `POST` batch incremental table-view rendering |
| `/api/logs/{id}` | `GET` complete log detail |
| `/api/logs/{id}/body` | `GET` raw stored or streaming-decoded request/response body |
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
| `/api/ca` | `GET`, `POST` to regenerate while the proxy is stopped |
| `/api/system-logs` | `GET`, `DELETE` |

System logs accept `after_seq` and are capped at 10,000 entries.

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
      "case_sensitive": false
    },
    "input": "example.com"
  },
  "min_id": 100,
  "max_id": 10000,
  "limit": 10000,
  "persist_filter": false
}
```

Every field is optional. Omitting `filter` reuses the Session's persisted filter. Supplying it
applies the draft to this query and persists it only after the ID scan succeeds. Set
`persist_filter: false` to filter without changing the Session view or emitting a UI synchronization
event; omitting the field retains the compatible default of `true`. `option: null` or an empty
`input` matches all logs; an empty input still preserves the selected option.

A column option supports `method`, `uri`, `code`, `source`, `stage`, or `{ "kind": "script", "script_name": "..." }`. Built-in and custom-column output are matched with contains; `case_sensitive` controls Unicode case folding. A script option has the form `{ "kind": "script", "script_name": "..." }` and passes `input` to that global Lua filter script. Custom-column and filter-script execution errors silently count as non-matches.

`min_id` and `max_id` are exclusive (`id > min_id && id < max_id`). The default and maximum page size are both 10,000.

When only `min_id` is supplied, the database scans toward newer IDs. When `max_id` or neither bound is supplied, it scans toward older IDs. Callers page in either direction by passing the relevant edge ID from their current list. Response order is intentionally unspecified:

```json
{
  "ids": [9999, 9998],
  "filter": {
    "option": {
      "kind": "column",
      "column": { "kind": "uri" },
      "case_sensitive": false
    },
    "input": "example.com"
  }
}
```

The response always includes the effective persisted filter. If a referenced script was removed outside the application, it is repaired to `{ "option": null, "input": "" }` and returned that way.

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

### Export logs as HAR

`POST /api/logs/export` accepts a strict JSON body:

```json
{
  "format": "har",
  "session_id": 1,
  "log_ids": [123, 124]
}
```

`format` is required and currently only accepts `har`; unsupported values return
`400 unsupported_export_format`, and unknown fields return `400 bad_request`. `session_id` is
optional and defaults to the active Session. Omitting `log_ids` snapshots the Session's current
maximum log ID and exports all eligible records up to that point. An empty array creates a valid HAR
with no entries. Explicit IDs are deduplicated and emitted in ascending order; if any explicit ID
does not exist in that Session, the whole request returns `404 log_not_found`.

Only captures with `outcome: success` and a response are eligible. Synthetic `CONNECT` captures at
the `tls_mitm` stage are skipped; ordinary HTTP responses and `101` upgrade handshakes remain
eligible. Ineligible explicit IDs are silently skipped. The endpoint generates the complete file
before sending HTTP 200, so a storage read failure returns a structured HTTP 500 instead of a
partial HAR. There is no application-level body or export-size limit.

The response is the raw UTF-8 HAR 1.2 JSON document, without the normal `{ "ok": true, "data": ... }`
envelope. Headers include:

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

HAR export performs no redaction. Authorization headers, Cookie/Set-Cookie values, and request and
response bodies can contain credentials or personal data; treat the file as sensitive.

### Read one complete log

`GET /api/logs/{id}?session_id=1` returns request/response metadata and bodies, error/outcome state,
plus `created_at`, `updated_at`, request `tags`, and ordered `request_interceptors` /
`response_interceptors` execution arrays. Every execution contains a unique `execution_id`,
`origin` (`saved` or `temporary`), `completed`, the historical script name, phase, zero-based
position, SHA-256 hash, exact source content, its own modifications, and an optional runtime error.
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

Log detail embeds decoded text/JSON only through 64 KiB. Every non-empty body includes the stored
byte size and selected absolute capture path; compressed bodies therefore report their compressed
file size. `GET /api/logs/{id}/body?session_id=1&side=request&decompress=false&max_size=16777216`
returns the stored byte stream, preserves `Content-Encoding`, and applies `max_size` to stored bytes.
The default limit is 16 MiB and there is no server maximum. `decompress=true` forbids `max_size` and
performs one streaming server decode for gzip, br, deflate, zstd, or stacked encodings, without a
pre-scan or decoded `Content-Length`. Active breakpoints expose the same contract at
`GET /api/breakpoints/{id}/body` and include current replacements for the paused phase.

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
filter. Column widths must be positive. A new/updated view cannot reference a missing global column
script. Deleting a referenced script removes matching table columns and resets filters that
reference it. Script names cannot be changed.

New Sessions always use the fixed built-in method, URI, status-code, and source columns plus an
empty filter. The first Session created in an empty workspace becomes active. Later creation does
not replace or restore an active Session.

Inactive Sessions can be archived while the proxy runs with
`POST /api/sessions/{id}/archive`. The whole directory moves to `sessions_archived/<id>`; archived
Sessions disappear from all existing Session, log, view, interceptor, and export APIs. The active
Session and Sessions with pinned requests return `409 conflict`. `GET /api/archived-sessions`
lists metadata only, `POST /api/archived-sessions/{id}/restore` moves it back without activating
it, and `DELETE /api/archived-sessions/{id}` permanently removes it. There is no direct deletion
operation for an unarchived Session.

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
uses the same validation through `PUT /api/config`. An actual active-Session change closes every
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

Executed content is stored in the Session capture database by lowercase SHA-256. `interceptor_script_contents` stores one copy of each unique source, while `capture_interceptor_runs` links captures to ordered executions and per-script modifications.

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

A routed HTTP request is inserted into its selected Session database before it is forwarded. The
Session is pinned for the entire request/response lifecycle even if the active Session changes.

Capture outcomes are:

- `in_progress`
- `success`
- `failed`
- `tunneled`

Errors contain a stable `kind`, an execution `stage`, and the underlying message. A provisional `CONNECT` row is inserted before the proxy acknowledges the tunnel, so even an idle connection is represented. TLS failures update that row. After a successful TLS handshake, the row is retained and finalized as `success` at stage `tls_mitm` with an HTTP 200 response; decrypted requests are stored as additional captures.

If the initial database insert fails, traffic is not forwarded. When the proxy stops it stops accepting immediately, waits up to five seconds, and marks unfinished rows with `proxy_shutdown`.

Request and response interceptors run once at their respective header boundary and cannot read the
original body. Original request and response bodies stream directly to append-only capture files
without an application-level size limit. Normal bodies continue streaming through the proxy;
replacement strings are sent from memory and replacement files are streamed from disk while the
raw body is drained independently. Capture-storage failures are recorded without interrupting the
business transfer. The proxy accepts at most 256 client connections.

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

Bypassed metadata is persisted in `<workspace>/bypass.db` without headers or bodies. Each row stores
timestamps, source, method, URI, version, routing reason, outcome, optional HTTP status/error, and
nullable upload/download byte counts. `GET /api/bypass` pages newest first; single, batch, and
terminal-clear deletion are supported. In-progress entries cannot be deleted and are marked failed
with `proxy_shutdown` when the proxy stops.

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
logical tables or blob layout.
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
