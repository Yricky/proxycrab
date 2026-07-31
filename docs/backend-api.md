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

## HTTP changes and desktop UI synchronization

Successful HTTP operations that mutate application-visible state publish an internal change event
to the Tauri frontend. This event is not a public HTTP endpoint and does not change HTTP response
envelopes. The frontend coalesces adjacent events, refreshes only affected resources, and keeps
normal Agent/API activity silent.

| HTTP operation | UI resources affected | Desktop behavior |
| --- | --- | --- |
| `PUT /api/workspace` | Workspace settings | Refreshes an open clean settings window |
| `PUT /api/config` | Settings and routing selection | Refreshes global config state |
| `POST /api/proxy/start`, `POST /api/proxy/stop` | Proxy status | Refreshes the toolbar state |
| Session create/update/delete | Session list and tags | Keeps the viewed Session unless it was deleted |
| Routing-script create/update/delete/selection | Routing library and selection | Refreshes the routing manager |
| Bypass delete/batch delete/clear | Bypass table | Refreshes the bypass window |
| `POST /api/logs/ids` with a changed `filter` | Target Session filter and visible log set | Reloads the table only when that Session is being viewed |
| `PUT /api/session-view` | Target Session columns | Reloads the table only when that Session is being viewed |
| Column-script create/update/delete | Column/filter choices and rendered custom columns | Refreshes script lists and the current table view |
| Filter-script create/update/delete | Filter choices and filtered results | Refreshes script lists and the current table view |
| Interceptor create/update/delete | Global interceptor library and Session chains | Refreshes the library and current pipeline |
| `PUT /api/session-interceptors` | Target Session pipeline | Refreshes the pipeline only when that Session is being viewed |
| `POST /api/ca` | CA manager | Reloads an open CA window |
| `DELETE /api/system-logs` | System-log viewer | Clears and reloads an open log window |

If HTTP deletes the viewed Session, the frontend selects the first remaining Session.

Open script and settings windows automatically reload when clean. If they contain unsaved input,
the frontend preserves it, displays an external-change warning, and lets the user explicitly reload
instead of overwriting local edits.

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
| `/api/logs/ids` | `POST` bounded/filterable log ID query |
| `/api/logs/views` | `POST` batch incremental table-view rendering |
| `/api/logs/{id}` | `GET` complete log detail |
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

## Network log queries

All three log operations accept an optional Session. The POST operations accept `session_id` in
their JSON body; the detail operation accepts `session_id` in its query string. Omitting it selects
the Session tagged `default`. If that tag is unbound, the operation returns `409 conflict`.

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
  "limit": 10000
}
```

Every field is optional. Omitting `filter` reuses the Session's persisted filter. Supplying it applies the draft to this query and persists it only after the ID scan succeeds. `option: null` or an empty `input` matches all logs; an empty input still preserves the selected option.

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

### Read one complete log

`GET /api/logs/{id}?session_id=1` returns request/response metadata and bodies, error/outcome state, plus `created_at`, `updated_at`, and ordered `request_interceptors` / `response_interceptors` execution arrays. Every execution contains the historical script name, phase, zero-based position, SHA-256 hash, exact source content, its own modifications, and an optional runtime error. Scripts that executed without changes are still present. Disabled and missing scripts are not recorded.

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
empty filter. The first manually created Session in an empty workspace receives the `default` tag.
Tags match `^[a-z0-9_]{1,20}$`, are globally unique, and move atomically between Sessions.

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

## Routing scripts and Session tags

`AppConfig.routing_script_name` stores an optional selected routing script. CRUD lives under
`/api/routing-scripts`; `PUT /api/routing-script-selection` accepts `{"name":"route"}` or
`{"name":null}`. There is no rename or debug endpoint.

For direct HTTP, routing runs per request. For HTTPS, it runs once on CONNECT and pins the selected
Session for the whole tunnel. Lua returns a tag or `nil`; `nil` bypasses. An explicit unbound tag
atomically creates one Session named after the tag with description
`由分流脚本「<script>」自动创建`. Invalid returns or runtime errors emit a system warning and fall
back to a bound `default` Session, otherwise bypass. With no selected script, `default` is used when
bound, otherwise bypass.

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

## Capture lifecycle

A routed HTTP request is inserted into its selected Session database before it is forwarded. The
Session is pinned for the entire request/response lifecycle even if tags change.

Capture outcomes are:

- `in_progress`
- `success`
- `failed`
- `tunneled`

Errors contain a stable `kind`, an execution `stage`, and the underlying message. A provisional `CONNECT` row is inserted before the proxy acknowledges the tunnel, so even an idle connection is represented. TLS failures update that row. After a successful TLS handshake, the row is retained and finalized as `success` at stage `tls_mitm` with an HTTP 200 response; decrypted requests are stored as additional captures.

If the initial database insert fails, traffic is not forwarded. When the proxy stops it stops accepting immediately, waits up to five seconds, and marks unfinished rows with `proxy_shutdown`.

Request and response bodies are bounded to 64 MiB with a 60-second read timeout. The proxy allows at most four exchanges to materialize bodies concurrently and at most 256 client connections, preventing many clients from multiplying per-request resource bounds without limit. Lua file body replacements use the same size bound.

Bypassed metadata is persisted in `<workspace>/bypass.db` without headers or bodies. Each row stores
timestamps, source, method, URI, version, routing reason, outcome, optional HTTP status/error, and
nullable upload/download byte counts. `GET /api/bypass` pages newest first; single, batch, and
terminal-clear deletion are supported. In-progress entries cannot be deleted and are marked failed
with `proxy_shutdown` when the proxy stops.

## Workspace and CA

The workspace is locked exclusively for the process lifetime. Routing selection and proxy/API
addresses are stored in workspace configuration. Each Session stores metadata (including tags),
its table view and filter in `sessions/<id>/view.json`, and interceptor chains in
`sessions/<id>/interceptors.json`. Session deletion is rejected while the proxy is starting,
running, or stopping.

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
