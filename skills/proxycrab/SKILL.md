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
API key, set it through `PROXYCRAB_API_KEY`; bundled scripts send it as `Authorization: Bearer ...`.
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
- Never use the per-run `pcrab_ui_…` browser token as an Agent API key. It is a trusted UI credential
  printed for a human; do not copy it into `PROXYCRAB_API_KEY` or any Agent command.
- Never use or expose a `pcrab_share_…` Session link token. It is scoped to the read-only browser
  page, is not an Agent API key, and must not appear in Agent arguments, URLs, logs, or reports.
- Treat `pcrab_har_…` HAR download tokens the same way. They expose a frozen set of unredacted
  captures through the browser-only download route and must never be used as Agent credentials or
  printed in reports.
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
| Inspect one capture | `scripts/log-get.mjs` |
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
| Inspect or change the active Session | `GET` or `PUT /api/active-session` |
| Archive, restore, or permanently delete a Session | Read `references/http-api.md` and use the archived-Session endpoints |
| Inspect transparent forwarding | `scripts/bypass-list.mjs` |
| Use a low-frequency endpoint | Read `references/http-api.md` and call it directly |
| Write or review Lua | Read `references/lua-api.md` |

Resolve script paths relative to this `SKILL.md`; do not assume the current working directory is the
skill directory.

## Standard capture-debugging workflow

1. List Sessions with their configured table column names and identify the user's intended Session:

   ```bash
   node <skill-dir>/scripts/session-list.mjs
   ```

2. If the active AGENTS.md instructions permit it and it is needed, create a dedicated Session and make it active with
   `PUT /api/active-session`:

   ```bash
   node <skill-dir>/scripts/session-create.mjs \
     --name agent-debug \
     --description "Temporary capture scope for the current investigation"
   # then PUT {"session_id":3} to /api/active-session
   ```

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
   - `request_interceptors` and `response_interceptors` preserve exact executed source, order,
     modifications, hashes, and runtime errors.
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
`replace_with_asset`. Asset IDs are permanent for the workspace because the API does not overwrite
or delete them; choose the ID deliberately and never retry a conflict under a different ID unless
the user approves the new name.

Before replacing a Session's interceptor chains, read `GET /api/session-interceptors` and keep the
previous JSON if restoration may matter. Apply both chains atomically:

```bash
node <skill-dir>/scripts/session-interceptors-set.mjs \
  --session-id 3 \
  --file /tmp/interceptors.json
```

Do not claim that an interceptor worked merely because it saved successfully. Capture a new request
and verify the historical execution and modifications in `log-get.mjs` output.

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

- Follow the active AGENTS.md instructions before creating or changing Sessions, views, filters,
  scripts, routing selection, or interceptor chains. When the endpoint is unavailable, do not make
  those UI-visible changes unless the user explicitly requests them.
- Respect management API permissions. Do not work around a denied or expired approval by switching
  to the no-key identity, changing credentials, or repeatedly retrying. Ask the user to adjust the
  selected identity in Settings > 管理接口 when authorization is required.
- Do not archive or restore Sessions, permanently delete archived Sessions, delete scripts, start
  or stop the proxy, or clear logs or records unless the user explicitly requests that action.
  CA regeneration, full application-config replacement, and workspace changes are host operations
  and are intentionally unavailable through this Agent HTTP API. An active Session
  cannot be archived; make it inactive only when the user authorizes that separate mutation.
- Do not replay requests or configure another process's proxy environment automatically.
- Keep local raw capture output intact for debugging. It can include authorization headers, cookies,
  tokens, and personal data. Redact those values from summaries, chat responses, tickets, and other
  shared artifacts unless the user explicitly needs them.
- Treat request and response bodies with side effects or credentials as sensitive.
- Prefer a dedicated active Session and tight filters over reading a large unrelated capture
  history.
- Log detail embeds at most 64 KiB of decoded text/JSON. Every non-empty persisted body includes its
  stored-byte size and local capture path. Use `body-get.mjs` for complete, normally client-decoded
  binary or oversized bytes;
  always set a deliberate stored-file maximum and output path because bodies can contain
  credentials or personal data. The body endpoint negotiates a browser-safe response encoding from
  `Accept-Encoding`; the limit applies to the selected final or interceptor-snapshot Body source.
- An empty original request or response has no capture blob file. Final interceptor Body strings
  are stored in capture metadata and Assets remain in `assets/`; ProxyCrab does not create
  `.modified` capture blobs.
- Interceptor mutations applied before a Lua runtime error remain applied. Inspect both
  `modifications` and `error`.
- Request tags are capture-local metadata. Presence of `_crab_skip`, including an empty value,
  intentionally suppresses the upstream request after the complete request chain.
- Filter and custom-column scripts can lazily read completed persisted bodies through
  `entry.req.body:as_string()` / `as_json()` and the matching `entry.resp.body` methods. Check cheap
  request/response metadata first because each matching capture may require body file I/O and
  decompression. In-progress bodies return `nil`; decoded getter output is limited to 16 MiB.
- `_crab_req_speed` and `_crab_resp_speed` pace the final outbound request and response bodies in
  bytes per second; `_crab_req_timeout` sets the upstream timeout in milliseconds (default 60000),
  and `_crab_resp_bodyframe_timeout` sets the idle milliseconds before each upstream response-body
  frame. Use only positive ASCII decimal values. Invalid final values are ignored with a runtime warning.
  These controls are per capture, never become HTTP headers, and do not apply to bypass, raw
  CONNECT, Upgrade/WebSocket, the local CA endpoint, or proxy-generated errors. `_crab_skip` still
  allows response pacing after response interceptors create the synthetic response.
- Request and response interceptors run at the header boundary. `body:as_string()` and
  `body:as_json()` wait for the effective original body to finish downloading when no replacement
  is active; this can block forever for SSE/infinite streams. Decoded getter output is limited to
  16 MiB. Captured bodies and `replace_with_asset` stream without an application-level size limit;
  use raw body endpoints with an explicit `max_size` when reading captures back.
- Workspace Assets are immutable through the API and can contain sensitive bytes. Upload only when
  the user asks for an Asset or body replacement, do not print their contents, and do not invent a
  replacement ID after `asset_already_exists` or `asset_path_conflict`. `replace_with_asset` removes
  stale Content-Encoding but does not set Content-Type.
- `_crab_tls_insecure` disables upstream HTTPS certificate-chain and hostname verification only when
  its final request-interceptor value is exactly `true`. It also applies to HTTPS Upgrade/WebSocket,
  has no effect on HTTP or transparent bypass, never becomes a header, and uses a pool isolated from
  verified traffic. Use it only for a narrowly matched controlled test host and explicitly warn the
  user about the reduced transport authentication.
- Keep user-created debugging state unless the user asks for cleanup. Automatic restoration is not
  required.
- Expect Session, filter, column, and interceptor changes to appear in the desktop UI. If the UI
  reports an external-edit conflict, do not tell the user to discard their unsaved work.

## References

- Read `references/http-api.md` for every HTTP endpoint, envelope, DTO, validation rule, pagination
  behavior, and error code.
- Read `references/lua-api.md` for the sandbox, globals, fields, methods, return contracts, examples,
  and interceptor execution semantics.
- Read `references/best-practices.md` when planning a longer investigation or writing a diagnostic
  report.
