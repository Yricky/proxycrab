---
name: proxycrab
description: Use ProxyCrab's management API to inspect captured or bypassed HTTP/HTTPS traffic, isolate failing requests, configure Lua bypass routing, export evidence, and create or attach Lua filters, custom columns, and request/response interceptors. Use this skill whenever the user mentions ProxyCrab, MITM capture debugging, captured requests or responses, ProxyCrab Sessions or /session?token= sharing links, ProxyCrab Lua scripts, traffic filtering, or asks an agent to diagnose an API call through the running ProxyCrab desktop app.
compatibility: Requires Node.js 18 or newer and access to a running ProxyCrab service through its management API or a Session sharing link.
---

# ProxyCrab

Use the running ProxyCrab desktop application or CLI process as an inspectable HTTP/HTTPS
debugging environment. Prefer the bundled scripts for high-frequency operations because they validate
arguments, unwrap the API envelope, emit machine-readable JSON, and return non-zero exit codes on
failure.

The service listens on every IPv4 interface by default; local Agent scripts still use
`http://127.0.0.1:18089`. Every script also accepts
`--base-url <url>` and honors `PROXYCRAB_API_URL`. If the selected management identity requires an
API key, use the least-privileged key suitable for the task through `PROXYCRAB_API_KEY`; scripts send
it as `Authorization: Bearer ...`.
Never put an API key in command arguments, URLs, output, or logs.

## Load active Agent instructions first

For management API workflows, immediately after reading this Skill and before calling any other
management API, read the active workspace instructions:

```bash
node <skill-dir>/scripts/agents-get.mjs
```

Instruction precedence is: the user's current explicit request, then the returned AGENTS.md
instructions, then this Skill's conservative built-in rules. If `/api/agents.md` is unavailable,
continue with the conservative built-in rules. Do not let AGENTS.md override the user's request.

When the user supplies a Session sharing link, use the sharing workflow below directly. The share
surface has no `/api/agents.md` endpoint and needs no management API key or local ProxyCrab instance.

## Preconditions

- For management workflows, confirm that the ProxyCrab desktop app is open. A connection failure usually means the app or its
  management HTTP service is not running. For the headless target, confirm that the CLI process and
  its management service are running instead.
- Proxy lifecycle is user-controlled. Do not start or stop the proxy unless the user explicitly
  asks for that separate action.
- Capturing traffic requires a proxy that the user has already started and either an active
  Session or a routing script returning a valid Session ID. With no selected routing script,
  traffic enters the active Session; if none is active, traffic is transparently forwarded and
  recorded in `bypass.db`.
- Use Node.js 18 or newer. The scripts have no npm dependencies.
- On the desktop target, a management call may wait for approval for up to 30 seconds. Bundled
  scripts allow 40 seconds for that decision. If a call returns `approval_timeout`, ask the user to
  retry and approve it; do not loop automatically. The CLI target has no approval decisions and
  treats `approval` as deny. `permission_denied` means the selected identity cannot perform the
  action, and `invalid_api_key` means the environment key is missing, malformed, unknown, or deleted.
- Never use the browser UIKey as an Agent API key. The default `pcrab_ui_…` token and custom
  UIKeys without that prefix are browser credentials; do not copy them into `PROXYCRAB_API_KEY`
  or any Agent command. UIKey cannot bypass startup redaction.
- Use a supplied `pcrab_share_…` Session link token to read its Session through `/share-api/*`
  with a `token` query parameter. It is not a management API key; see the sharing workflow below.
- Treat the `GET`, `POST`, and `DELETE` Session-share management routes as normal
  permission-controlled Agent actions.
  Returned tokens authorize the Session share surface, not management `Authorization`.
- Bundled management scripts allow a remote base URL only when `PROXYCRAB_API_KEY` is set. The remote
  service must be deliberately bound to a reachable address, and that API-key identity must allow
  the requested route. Prefer authenticated TLS or an SSH local port forward; plain HTTP exposes
  the Bearer key and captured data, so use it only on a trusted network.
- Management mutations also update the open application. Preserve the user's unsaved edits
  when an external-edit conflict is reported.

## Choose the smallest workflow

| Goal | Start here |
| --- | --- |
| Read active Agent instructions | `scripts/agents-get.mjs` |
| Write/import a reusable Session page | Import `{ backend }` from `./init.js`; see [Session pages](references/session-pages.md) and `scripts/session-page-import.mjs` |
| Import explicitly accepted bundle intents / resolve conflicts | `scripts/bundle-import.mjs`; see `references/http-api.md#session-configuration-zips-and-intent-imports` |
| List enabled Session shares without tokens | `scripts/session-share-list.mjs` |
| Enable or configure sharing, including restart persistence | `POST /api/session-shares` with optional `session_share_config`; see `references/http-api.md` |
| Read a supplied Session sharing link | [Session sharing workflow](#read-a-session-sharing-link), then `references/http-api.md#session-share-read-api` |
| Find existing traffic | `scripts/session-list.mjs`, then `scripts/log-query.mjs` |
| Intercept WebSocket messages | Request interceptor `req:on_ws_message`; see `references/lua-api.md`. Verify provenance and disposition with `websocket-messages.mjs` |
| Inspect WebSocket messages | `scripts/websocket-messages.mjs`, then `body-get.mjs --message-id ID`; see WebSocket sections in HTTP/Lua references |
| Inspect one capture | `scripts/log-get.mjs` |
| Replay a request through a Session (source `ProxyCrabRequest`) | `scripts/replay-send.mjs` |
| Read an interceptor execution's source or entry snapshot | `scripts/interceptor-get.mjs` |
| Save a complete request/response body | `scripts/body-get.mjs` |
| Upload an immutable workspace Asset | `scripts/asset-upload.mjs` |
| Wait for a new matching capture | `scripts/log-wait.mjs` |
| Save a diagnostic artifact | `scripts/log-export.mjs` |
| Query with temporary Lua without saving | `scripts/log-filter.mjs --session-id ID --file SCRIPT.lua` |
| Create or update a Lua filter | `scripts/filter-upsert.mjs`, then `scripts/filter-debug.mjs` |
| Create or update a custom column | `scripts/column-upsert.mjs` |
| Create or update an interceptor | `scripts/interceptor-upsert.mjs` |
| Attach interceptor chains to a Session | `scripts/session-interceptors-set.mjs` |
| Inspect or control active interceptor breakpoints | `scripts/breakpoint-list.mjs`, then `scripts/breakpoint-control.mjs` |
| Create or update a routing script | `scripts/routing-upsert.mjs` |
| Select or clear the routing script | `scripts/routing-select.mjs` |
| Download the current public CA certificate | `scripts/certificate-download.mjs --out PATH` (no proxy setup required) |
| Inspect, enable, or disable a temporary Clash subscription, or read/update its YAML | `scripts/clash-subscription.mjs`; see `references/http-api.md#clash-subscription` |
| Inspect the active Session | `scripts/session-list.mjs` (`active_session_id`) |
| Change the active Session | `PUT /api/active-session` |
| Archive, restore, or permanently delete a Session | Read `references/http-api.md` and use the archived-Session endpoints |
| Inspect transparent forwarding | `scripts/bypass-list.mjs` |
| Use a low-frequency endpoint | Read `references/http-api.md` and call it directly |
| Write or review Lua | Read `references/lua-api.md` |

Resolve script paths relative to this `SKILL.md`; do not assume the current working directory is the
skill directory.

Updating an existing custom-column script refreshes cached values in open views using that name,
including historical captures. Unrelated columns retain their cache. Shared viewers receive named
invalidations over SSE; reconnection triggers a full refresh without exposing script source. Verify API results by rendering the same
capture without `updated_at`; capture timestamps do not change when a script is edited.

Share defaults are enforced by the backend. New shares with an omitted redaction field inherit
local dynamically. Keys bound to a script must omit the field and shares follow their Key binding;
unconfigured Keys must also omit it. Only a Key explicitly bound to `""` may supply a share policy.
Never remove bindings or change credentials to bypass `share_redaction_forbidden`. Explicit `""`
means no share-level redaction; explicit null restores local inheritance. Omitted update fields
preserve existing policy. Use POST /api/session-shares for every update; the separate redaction
route is removed. Local/Key lookups fail closed. Startup redaction remains authoritative.

## Write a Session page

Create a ZIP with root `manifest.json` (version/id/name/backend_version: 1) and `index.html`.
Use `import { backend } from "./init.js"` inside a native ES module; initialization is automatic.
ProxyCrab serves init.js: do not package it. Use relative resource paths, the exported Backend
methods, and method presence for optional features. Interested components subscribe directly with
`subscribeChanges`, load initial state explicitly, and unsubscribe on disposal. Subscribers share one
Backend connection; `all` refreshes only their supported resources. Open the page through a Session sharing link;
the Backend reads only that link's Session. For format, types, examples and import/share operations, read
[Session pages](references/session-pages.md).

## Read a Session sharing link

For a URL such as `http://host:18089/session?token=pcrab_share_…`, agents can call the
read-only share API directly. Use the link's origin (scheme, host, and port) as the base URL and
its decoded `token` as exactly one query parameter on every `/share-api/*` request. Use URL parsing
and `URL.searchParams.set` so the token is encoded correctly. No `Authorization` header,
`PROXYCRAB_API_KEY`, local app, or management approval is needed. The existing bundled management
scripts use `/api/*` and do not support share authentication; use an HTTP client directly.

1. Call `GET /share-api/bootstrap` to obtain `session`, `view`, `proxy_status`, and `api_port`.
   Use `session.id` for evidence; an optional `id` in the original page URL identifies a capture to
   inspect, rather than a Session ID.
2. Query `POST /share-api/logs/ids` with an explicit filter (or `{}` for all captures).
   Filtering is stateless. Read `matched_ids`, then batch table rows through
   `POST /share-api/logs/views` with `{"logs":[{"id":42}]}` (at most 200 per call), or inspect
   candidates with `GET /share-api/logs/{id}`. For a full ID page, continue with the smallest
   matched ID as exclusive `max_id`; stop on a short page. Re-evaluate `in_progress_ids` on completion.
3. Read current chain metadata with `GET /share-api/session-interceptors` when needed. It returns
   ordered request/response items (`name`, optional `display_name`, `enabled`, `valid`) for the shared
   Session only, with no library source or writes. This is current configuration, not proof of what
   executed for a historical capture.
   Read raw bodies, WebSocket messages, or historical interceptor source/snapshots as needed using
   the [complete share route table](references/http-api.md#session-share-read-api).
   These endpoints always use the shared Session; a supplied `session_id` cannot select another one.
4. Report capture evidence and the Session/log IDs. Share reads apply the effective redaction
   policy. `invalid_share_token` means the token is invalid or revoked; `share_session_unavailable`
   means the Session is unavailable. Report the error and request a working link when needed.
   A `redaction_failed` response must not be bypassed by changing credentials or bindings.

This Node.js example uses `PROXYCRAB_SHARE_URL` as an example input variable containing the supplied
link (not a built-in management-script option). It finds captures whose URI contains `/checkout`:

```bash
node --input-type=module <<'JS'
const link = new URL(process.env.PROXYCRAB_SHARE_URL);
const tokens = link.searchParams.getAll("token");
if (tokens.length !== 1 || !tokens[0]) throw new Error("Expected one share token");
async function read(path, body) {
  const url = new URL(path, link.origin);
  url.searchParams.set("token", tokens[0]);
  const response = await fetch(url, {
    ...(body === undefined ? {} : {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    }),
    signal: AbortSignal.timeout(40000),
  });
  const result = await response.json();
  if (!response.ok || !result.ok) throw new Error(result.error?.code ?? `HTTP ${response.status}`);
  return result.data;
}
const bootstrap = await read("/share-api/bootstrap");
const matches = await read("/share-api/logs/ids", {
  filter: { option: { kind: "column", column: { kind: "uri" }, regex: false }, input: "/checkout" },
  limit: 50,
});
console.log(JSON.stringify({ session_id: bootstrap.session.id, ...matches }));
JS
```

Sharing does not expose replay, writes, Session switching, global traffic, or management instructions.
Disabling sharing invalidates the link. Temporary shares also expire on process exit; persistent
shares restore the same token and redaction binding after restart. For management operations use a
separately authorized management identity and the ordinary management workflow.

## Standard capture-debugging workflow

1. List Sessions with their configured table column names and identify the user's intended Session.
   The script returns `{ sessions: [...], active_session_id }`; match the ID to an item in `sessions`.
   A `null` ID means no Session is active:

   ```bash
   node <skill-dir>/scripts/session-list.mjs
   ```

2. If the active AGENTS.md instructions permit it and it is needed, create a dedicated Session and
   make it active:

   ```bash
   node <skill-dir>/scripts/session-create.mjs \
     --name agent-debug \
     --description "Temporary capture scope for the current investigation" \
     --active true
   ```

   Creation never changes the active Session; `--active true` follows it with `PUT /api/active-session`,
   which closes every established proxy connection, CONNECT tunnel, Upgrade/WebSocket, and upstream
   pooled connection before returning. An existing Session can also be made active with
   `PUT {"session_id":3}` to `/api/active-session`.

3. Narrow the candidate set before reading full details. Only one ProxyCrab filter option can be
   active at a time:

   ```bash
   node <skill-dir>/scripts/log-query.mjs \
     --session-id 3 \
     --uri "/api/orders" \
     --limit 50
   ```

   The bundled query script and direct `POST /api/logs/ids` calls are always read-only. Save a
   Session filter only when the user explicitly requests it, through
   `PUT /api/sessions/{id}/filter`. Column matching is case-sensitive; add `--regex` to
   `log-query.mjs` for Rust `fancy-regex` syntax, including anchors, inline `(?i)` flags,
   lookaround such as `^(?!.*xxx).*$` to exclude values containing `xxx`, and backreferences.
   Matching runtime errors, including backtracking-limit errors, count as non-matches.

4. Fetch only the candidate captures needed for diagnosis:

   ```bash
   node <skill-dir>/scripts/log-get.mjs --session-id 3 --log-id 1042
   ```

5. Diagnose from concrete evidence:

   - `outcome`, `stage`, and `error` identify transport and proxy failures.
   - Request URI, headers, and body describe what was sent.
   - Response status, headers, and body describe what came back.
   - `request_interceptors` and `response_interceptors` preserve execution order, hashes,
     non-snapshot modifications, and runtime errors. `log-get.mjs` does not embed the executed
     source or the entry snapshot; read them with `interceptor-get.mjs` using the capture ID and the
     execution's `execution_id` when `has_snapshot` is true.
   - An absent response can be normal for `in_progress`, a tunnel, or a failed exchange; interpret it
     with `outcome`, `stage`, and `error`. If current activity matters, read `ProxyStatus`: a netlog
     ID is active only when it appears under its Session ID in `running.active_netlog`. Do not infer
     activity from persisted `outcome`, `stage`, `created_at`, or `started_at`.
   - `running.latest_netlog_id` is a per-Session new-record marker, not an activity count. It retains
     the greatest registered capture ID observed by the application process after requests finish,
     so merged notifications still reveal short requests. It survives proxy stop/start, starts
     empty on application restart, and is Session-scoped on share reads. The UI uses it to discover
     new records independently of active-Session selection and stops log polling when idle.

6. Report the Session ID and log IDs used, the observed evidence, the most likely cause, uncertainty,
   and a concrete next check. Redact secrets from the user-facing explanation.

## Waiting for new traffic

`log-wait.mjs` snapshots the current matching IDs, then waits for a newer match. It polls every
500 ms for up to 30 seconds by default and waits for the selected capture to leave `in_progress`.

```bash
node <skill-dir>/scripts/log-wait.mjs \
  --session-id 3 \
  --uri "/api/checkout" \
  --method POST \
  --timeout-ms 30000
```

The management API supports one filter option, so `--method` is applied locally when another remote
filter such as `--uri` is present. The script re-evaluates explicit candidate IDs so a capture that
was in progress but ultimately fails the remote filter is not reported. `in_progress_ids` contains
only captures active in the current proxy run, not stale unfinished rows from an earlier run. See
each script's `--help` output for supported options.

## Lua change workflow

Read `references/lua-api.md` before creating Lua. Write source to a local `.lua` file so quoting and
newlines remain exact.

For multiple values of one header, use `req.headers:set(name, {"first", "second"})` or the same
method on `resp.headers`. This replaces existing values; `{}` deletes the header. Every array
element must be a valid string. See the Lua reference for validation and modification history.

Run a temporary standard filter without saving a script or changing the Session:

```bash
node <skill-dir>/scripts/log-filter.mjs --session-id 3 --file /tmp/server-error.lua --limit 100
```

This calls `POST /api/logs/ids` with `filter.option: {kind: "script_content", content: source}`
and `filter.input` passed exactly via `...`, including the empty string. All ordinary ID query
parameters and `{matched_ids, in_progress_ids}` behavior apply; omitting Session selects the active
one. Unlike named-script list filtering, inline compilation/runtime errors and non-boolean returns
fail with `bad_request`. The option is query-only and also works on `/share-api/logs/ids` within
the share scope and redaction policy; Session filter writes do not accept it.

For temporary computed cells, call `POST /api/logs/views` with
`view.columns: [{kind: "script_content", name: "host", content: "return entry.req.uri.host", width: 160}]`.
The query and response use `LogViewColumn`; saved Session columns stay unchanged. All three fields
`name`/`content`/positive `width` are required. Same-name columns run independently. Errors leave
only that cell empty and appear in `exceptions` with its log ID and column index. Management and
shared `/logs/views` remain read-only and use the caller's effective redaction policy.

Create or update a filter and test it against a known capture:

```bash
node <skill-dir>/scripts/filter-upsert.mjs --name server-error --file /tmp/server-error.lua
node <skill-dir>/scripts/filter-debug.mjs \
  --name server-error \
  --session-id 3 \
  --log-id 1042 \
  --input example.com
```

Create or update an interceptor:

```bash
node <skill-dir>/scripts/interceptor-upsert.mjs \
  --kind request \
  --name add-debug-header \
  --file /tmp/add-debug-header.lua
```

For a reusable body, upload an immutable workspace Asset before writing the interceptor:

```bash
node <skill-dir>/scripts/asset-upload.mjs \
  --id fixtures/mobile-error.json \
  --file /tmp/mobile-error.json \
  --content-type application/json
```

Then resolve it with `get_asset("fixtures/mobile-error.json")` and pass the returned object to
`replace_with_asset`. Assets cannot be overwritten or deleted through the API. Upload only for an
authorized Asset/body-replacement task, and do not invent a new ID after a conflict without the
user's approval. `replace_with_asset` removes stale Content-Encoding but does not set Content-Type.

Before replacing a Session's interceptor chains, read `GET /api/session-interceptors` and keep the
previous JSON if restoration may matter. Apply both chains atomically:

```bash
node <skill-dir>/scripts/session-interceptors-set.mjs \
  --session-id 3 \
  --file /tmp/interceptors.json
```

Do not claim that an interceptor worked merely because it saved successfully. Capture a new request
and verify the historical execution and modifications with `log-get.mjs`, then read the executed
source with `interceptor-get.mjs --what content` and the entry snapshot with
`interceptor-get.mjs --what snapshot` when `has_snapshot` is true.

When an interceptor is paused, list active breakpoints before acting:

```bash
node <skill-dir>/scripts/breakpoint-list.mjs --session-id 3 --phase request
node <skill-dir>/scripts/breakpoint-control.mjs --id 7 --action detail
```

Extending, releasing, or executing a temporary script changes a live request. Perform those actions
only when the user asks. Temporary execution keeps the breakpoint paused and can retain mutations
made before a Lua error; inspect the returned `execution.error` and modifications.

Routing scripts return `true` to capture into the active Session and `false` or `nil` to bypass
capture and TLS decryption. Returning `true` with no active Session also bypasses. A positive Lua
integer Session ID selects that non-archived Session directly, even when no Session is active,
using its interceptors without changing the active Session. Routing-only `get_sessions()` returns
a snapshot of non-archived Sessions with `id`, `name`, optional `description`, and boolean `active`.
HTTPS selects its Session once at CONNECT for the entire tunnel. Invalid or archived targets,
invalid returns, and script errors emit a system warning and bypass. Select a routing script only after reviewing
`references/lua-api.md`. Effective active-Session and selected-routing changes disconnect existing
proxy connections, tunnels, upgrades, and upstream pools; identical updates do not.

## Safety and evidence rules

- Follow the active AGENTS.md instructions for mutations. When the endpoint is unavailable, do not
  mutate application state unless the user explicitly requests it.
- Respect API permissions. Do not bypass a denial or expired approval by switching credentials or
  retrying. Identity and permission management are unavailable through the Agent API.
- Archive/restore/delete Sessions, delete scripts, clear records, and replay requests only when
  requested. Do not configure another process's proxy environment automatically. Keep debugging
  state unless the user asks for cleanup. An active Session cannot be archived; deactivating it
  requires authorization too. Archiving an inactive Session now disconnects its associated
  connections and replay tasks and releases its breakpoints before moving its files; in-progress
  requests no longer block archive. Other independent connections and the active selection are
  preserved. UI Sweep also supports inactive Sessions and keeps their replacement inactive.
- CA regeneration, full application-config replacement, and workspace changes are unavailable
  through the Agent API. Workspace switching restarts the desktop app and discards unsaved edits;
  CLI workspaces are selected with `--workspace` at startup.
- Preserve API-returned evidence, which may already be redacted. Check for credentials and personal
  data before sharing summaries or artifacts; do not assume read-time redaction covers every field.
- Log detail embeds at most 64 KiB of decoded text/JSON. Use `body-get.mjs` for complete bodies with
  a deliberate output path and stored-size maximum (default 16 MiB). See the HTTP reference for
  encoding negotiation and interceptor-entry snapshot selection.
- Interceptor mutations made before a Lua runtime error remain applied: inspect both modifications
  and errors. Body getters can wait forever on SSE/infinite streams. Text/JSON getters and
  materialized Lua results are limited by the 16 MiB budget; `as_bytes()` returns an editable view
  over immutable memory/files and can inspect larger files. Edits affect only the view until
  explicitly submitted. HTTP `replace_with_bytes` persists an Asset reference; read-time binary
  redaction stays temporary. WS redaction `as_bytes` now returns a view, so use `:to_string()` when
  a binary string is required. See the Lua reference for the complete API.
- Use `_crab_tls_insecure` only for a narrowly matched controlled test host and warn that it disables
  upstream certificate-chain and hostname verification. Forced upstream `HTTP/2` has no HTTP/1.1
  fallback. These tags affect the upstream hop, not the captured client protocol.

## References

- Read `references/http-api.md` for every HTTP endpoint, envelope, DTO, validation rule, pagination
  behavior, and error code.
- Read `references/lua-api.md` for the sandbox, globals, fields, methods, return contracts, examples,
  and interceptor execution semantics.
- Read `references/best-practices.md` when planning a longer investigation or writing a diagnostic
  report.

## Read-time redaction

Management reads may be transformed by the selected identity's bound Lua redaction script. An
unbound identity normally returns original data; a failed/missing bound script refuses the read. Do not retry
with another credential or remove a binding to bypass that failure. Ordinary captures are unchanged.

Use `scripts/redaction-upsert.mjs --name NAME --file SCRIPT.lua` to create/update an authorized
redaction script. Use `scripts/share-redaction-set.mjs --session ID --kind page --name NAME`
(or `--clear true` for explicit no redaction, `--inherit true` for local inheritance, only when requested) to change an enabled share's binding. The helper reads the current persistence mode and updates both settings through
`POST /api/session-shares`. Binding changes do not print
the share token. Listing and creating scripts are allowed by default, replacing
or deleting one needs owner approval, and reading one script's source is denied by default; the
upsert script creates first and treats a conflict as an update instead of reading the script. Never
change permissions automatically. See the HTTP and Lua references for
`context.api`, the three-state data model, lazy Body access, and the intentionally excluded history.

The headless server may enforce a startup policy with `--redaction-script FILE`. This overrides all
identity/share bindings, including UIKey, and remains pinned until restart. Its five script-library
operations return `redaction_management_disabled` (HTTP 403, `脱敏接口管理功能已禁用`), subject to
normal authentication/permissions. Report this as disabled management; do not retry an update,
change credentials, or clear bindings to bypass it. Runtime failures fail closed; the policy covers
only the supported reads listed in the HTTP reference.

## WebSocket captures

Successful HTTP/1.1 WebSocket 13 upgrades have a separate linked ws/wss capture. Read its
`websocket` field and `request.version` (`WebSocket/13`); use runtime active-netlog membership
for current connection activity. Message IDs are per direction (`request` = C2S, `response` = S2C).
List metadata with `websocket-messages.mjs`, using `next_after_id` for pagination/polling, then read
one payload with `body-get.mjs --message-id`. Never attempt raw container download or treat it as
an HTTP body. Capture content is decoded/reassembled and limited to 16 MiB per message; omitted
messages retain metadata.
Bound redaction scripts must opt into WebSocket messages; never bypass a failed/legacy policy.


Request interceptors may register one `req:on_ws_message(function(ws) ... end)` callback each.
Callbacks run serially per connection, in registration order, for complete text/binary messages.
`ws:drop()` stops the current message but keeps its captured record. `ws:send(...)` can target either
direction; generated messages only reach later callbacks and enter a breadth-first FIFO queue.
History is read-only and includes finalized original, generated, dropped and failed messages from
both directions. Follow `root`, `parent`, `producer` and `dropped_by` to explain a message's lineage;
`producer.execution_id` identifies the source script snapshot, while `invocation_id` identifies its
specific callback invocation. Read historical code with `interceptor-get.mjs --log-id HANDSHAKE_ID
--execution-id EXECUTION_ID`, not the current library script. The connection's `interceptors` list
contains its shared source-snapshot references. See the Lua reference for rollback and queue limits.

### Nested script paths

All script helpers accept relative paths through `--name`, for example
`--name business/errors` (without the automatic `.lua` extension). Use the same full name in
Session bindings and routing selection. Helpers encode the whole name in URL parameters;
do not pre-encode the argument. See [script path rules](references/http-api.md#script-relative-paths).

## HAR API boundary

There is no HAR generation, download, or sharing HTTP endpoint. Agents should read supplied
Session links through the existing share routes rather than invent export
routes. Read failures never authorize changing credentials or bypassing redaction.

## Session configuration bundles

Configuration ZIP downloads require the link owner to enable `session_config_share`. Asset dependencies are manually selected exact IDs or slash-terminated prefixes; see the asset best practices in `references/lua-api.md`. Import only intent IDs and conflict choices explicitly accepted by the user; never activate sessions as part of import.
