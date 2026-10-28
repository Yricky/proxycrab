---
name: proxycrab
description: Use ProxyCrab's management API to inspect captured or bypassed HTTP/HTTPS traffic, isolate failing requests, configure Lua bypass routing, export evidence, and create or attach Lua filters, custom columns, and request/response interceptors. Use this skill whenever the user mentions ProxyCrab, MITM capture debugging, captured requests or responses, ProxyCrab Sessions, ProxyCrab Lua scripts, traffic filtering, or asks an agent to diagnose an API call through the running ProxyCrab desktop app.
compatibility: Requires a running ProxyCrab desktop app or CLI management service, Node.js 18 or newer, and access to its management API.
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

Immediately after reading this Skill, and before calling any other ProxyCrab API, read the active
workspace instructions:

```bash
node <skill-dir>/scripts/agents-get.mjs
```

Instruction precedence is: the user's current explicit request, then the returned AGENTS.md
instructions, then this Skill's conservative built-in rules. If `/api/agents.md` is unavailable,
continue with the conservative built-in rules. Do not let AGENTS.md override the user's request.

## Preconditions

- Confirm that the ProxyCrab desktop app is open. A connection failure usually means the app or its
  management HTTP service is not running. For the headless target, confirm that the CLI process and
  its management service are running instead.
- Proxy lifecycle is user-controlled. Do not start or stop the proxy unless the user explicitly
  asks for that separate action.
- Capturing traffic requires an active Session and a proxy that the user has already started. With
  no selected routing script, traffic enters the active Session; if none is active, traffic is
  transparently forwarded and recorded in `bypass.db`.
- Use Node.js 18 or newer. The scripts have no npm dependencies.
- On the desktop target, a management call may wait for approval for up to 30 seconds. Bundled
  scripts allow 40 seconds for that decision. If a call returns `approval_timeout`, ask the user to
  retry and approve it; do not loop automatically. The CLI target has no approval decisions and
  treats `approval` as deny. `permission_denied` means the selected identity cannot perform the
  action, and `invalid_api_key` means the environment key is missing, malformed, unknown, or deleted.
- Never use the browser UIKey as an Agent API key. It defaults to a per-run `pcrab_ui_…` token,
  but `--ui-key KEY` can supply a custom value without that prefix. Both are trusted UI credentials
  printed for a human; do not copy them into `PROXYCRAB_API_KEY` or any Agent command. A custom
  UIKey cannot bypass startup redaction.
- Never use or expose a `pcrab_share_…` Session link token. It is scoped to the read-only browser
  page, is not an Agent API key, and must not appear in Agent arguments, URLs, logs, or reports.
- Treat `pcrab_har_…` HAR download tokens the same way. They grant browser-only access to a frozen
  set of captures subject to the effective redaction policy. Never use them as Agent credentials or
  print them in reports.
- Treat the `GET`, `POST`, and `DELETE` Session-share management routes as normal
  permission-controlled Agent actions; the HAR-share management routes follow the same rule.
  Returned tokens are browser-only and must not be reused in `Authorization` or printed in reports.
- Bundled Agent scripts allow a remote base URL only when `PROXYCRAB_API_KEY` is set. The remote
  service must be deliberately bound to a reachable address, and that API-key identity must allow
  the requested route. Prefer authenticated TLS or an SSH local port forward; plain HTTP exposes
  the Bearer key and captured data, so use it only on a trusted network.
- Successful management mutations synchronize into the open desktop UI. Archiving the viewed
  Session selects the first remaining Session; other mutations keep the viewed Session. Open
  editors preserve unsaved local changes.

## Choose the smallest workflow

| Goal | Start here |
| --- | --- |
| Read active Agent instructions | `scripts/agents-get.mjs` |
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
   `log-query.mjs` for Rust regex syntax such as anchored exact matches or inline `(?i)` flags.

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
capture and TLS decryption. Returning `true` with no active Session also bypasses. Invalid returns
and script errors emit a system warning and bypass. Select a routing script only after reviewing
`references/lua-api.md`. Effective active-Session and selected-routing changes disconnect existing
proxy connections, tunnels, upgrades, and upstream pools; identical updates do not.

## Safety and evidence rules

- Follow the active AGENTS.md instructions for mutations. When the endpoint is unavailable, do not
  make UI-visible changes unless the user explicitly requests them.
- Respect API permissions. Do not bypass a denial or expired approval by switching credentials or
  retrying. The user can adjust the selected identity in Settings > 管理接口.
- Archive/restore/delete Sessions, delete scripts, clear records, and replay requests only when
  requested. Do not configure another process's proxy environment automatically. Keep debugging
  state unless the user asks for cleanup. An active Session cannot be archived; deactivating it
  requires authorization too.
- CA regeneration, full application-config replacement, and workspace changes are unavailable
  through the Agent API. Desktop workspace switching is under Settings > 通用 > 切换工作区 and
  restarts the app, discarding unsaved edits. CLI users select `--workspace` at startup.
- Preserve API-returned evidence, which may already be redacted. Check for credentials and personal
  data before sharing summaries or artifacts; do not assume read-time redaction covers every field.
- Log detail embeds at most 64 KiB of decoded text/JSON. Use `body-get.mjs` for complete bodies with
  a deliberate output path and stored-size maximum (default 16 MiB). See the HTTP reference for
  encoding negotiation and interceptor-entry snapshot selection.
- Interceptor mutations made before a Lua runtime error remain applied: inspect both modifications
  and errors. Body getters can wait forever on SSE/infinite streams; decoded getter output is
  limited to 16 MiB. See the Lua reference for Body access, tags, and traffic controls.
- Use `_crab_tls_insecure` only for a narrowly matched controlled test host and warn that it disables
  upstream certificate-chain and hostname verification. Forced upstream `HTTP/2` has no HTTP/1.1
  fallback. These tags affect the upstream hop, not the captured client protocol.
- If the UI reports an external-edit conflict, preserve the user's unsaved work.

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
The owner can manage bindings in settings and Session sharing; HAR policies run during download.

Use `scripts/redaction-upsert.mjs --name NAME --file SCRIPT.lua` to create/update an authorized
redaction script. Use `scripts/share-redaction-set.mjs --session ID --kind page|har --name NAME`
(or `--clear true` only when explicitly requested) to change an enabled share's binding. Binding
changes do not print the share token. Listing and creating scripts are allowed by default, replacing
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
messages retain metadata. HAR excludes WS connection rows but may include handshake rows.
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
