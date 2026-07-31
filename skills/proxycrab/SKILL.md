---
name: proxycrab
description: Use ProxyCrab's local management API to inspect captured or bypassed HTTP/HTTPS traffic, isolate failing requests, configure tag-based Lua routing, export evidence, and create or attach Lua filters, custom columns, and request/response interceptors. Use this skill whenever the user mentions ProxyCrab, MITM capture debugging, captured requests or responses, ProxyCrab Sessions, ProxyCrab Lua scripts, traffic filtering, or asks an agent to diagnose an API call through the running ProxyCrab desktop app.
compatibility: Requires a running ProxyCrab desktop app, Node.js 18 or newer, and access to its loopback management API.
---

# ProxyCrab

Use the running ProxyCrab desktop application as a local, inspectable HTTP/HTTPS debugging
environment. Prefer the bundled scripts for high-frequency operations because they validate
arguments, unwrap the API envelope, emit machine-readable JSON, and return non-zero exit codes on
failure.

The default management API is `http://127.0.0.1:18089`. Every script also accepts
`--base-url <url>` and honors `PROXYCRAB_API_URL`.

## Preconditions

- Confirm that the ProxyCrab desktop app is open. A connection failure usually means the app or its
  management HTTP service is not running.
- Proxy lifecycle is user-controlled. Do not start or stop the proxy unless the user explicitly
  asks for that separate action.
- Capturing traffic requires a routed Session and a proxy that the user has already started. With no
  selected routing script, traffic uses the Session tagged `default`; if that tag is unbound,
  traffic is transparently forwarded and recorded in `bypass.db`.
- Use Node.js 18 or newer. The scripts have no npm dependencies.
- Successful management mutations synchronize into the open desktop UI without changing the
  Session the user is viewing. Open editors preserve unsaved local changes.

## Choose the smallest workflow

| Goal | Start here |
| --- | --- |
| Find existing traffic | `scripts/session-list.mjs`, then `scripts/log-query.mjs` |
| Inspect one capture | `scripts/log-get.mjs` |
| Wait for a new matching capture | `scripts/log-wait.mjs` |
| Save a diagnostic artifact | `scripts/log-export.mjs` |
| Create or update a Lua filter | `scripts/filter-upsert.mjs`, then `scripts/filter-debug.mjs` |
| Create or update a custom column | `scripts/column-upsert.mjs` |
| Create or update an interceptor | `scripts/interceptor-upsert.mjs` |
| Attach interceptor chains to a Session | `scripts/session-interceptors-set.mjs` |
| Create or update a routing script | `scripts/routing-upsert.mjs` |
| Select or clear the routing script | `scripts/routing-select.mjs` |
| Inspect transparent forwarding | `scripts/bypass-list.mjs` |
| Use a low-frequency endpoint | Read `references/http-api.md` and call it directly |
| Write or review Lua | Read `references/lua-api.md` |

Resolve script paths relative to this `SKILL.md`; do not assume the current working directory is the
skill directory.

## Standard capture-debugging workflow

1. List Sessions and identify the user's intended Session:

   ```bash
   node <skill-dir>/scripts/session-list.mjs
   ```

2. If needed, create a dedicated Session and assign routing tags with
   `PUT /api/sessions/{id}`. Moving a tag automatically removes it from its previous Session:

   ```bash
   node <skill-dir>/scripts/session-create.mjs \
     --name agent-debug \
     --description "Temporary capture scope for the current investigation"
   # then PUT {"tags":["agent_debug"]} to /api/sessions/3
   ```

3. Narrow the candidate set before reading full details. Only one ProxyCrab filter option can be
   active at a time:

   ```bash
   node <skill-dir>/scripts/log-query.mjs \
     --session-id 3 \
     --uri "/api/orders" \
     --limit 50
   ```

   Supplying a filter changes the Session's persisted filter. The scripts do not restore it.

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
     with `outcome`, `stage`, and `error`.

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
filter such as `--uri` is present. See each script's `--help` output for supported options.

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

Before replacing a Session's interceptor chains, read `GET /api/session-interceptors` and keep the
previous JSON if restoration may matter. Apply both chains atomically:

```bash
node <skill-dir>/scripts/session-interceptors-set.mjs \
  --session-id 3 \
  --file /tmp/interceptors.json
```

Do not claim that an interceptor worked merely because it saved successfully. Capture a new request
and verify the historical execution and modifications in `log-get.mjs` output.

Routing scripts return a lowercase tag or `nil`. An explicit unbound tag creates a Session bearing
that tag; `nil` bypasses capture and TLS decryption. Script errors fall back to a bound `default`
Session, otherwise bypass. Select a routing script only after reviewing
`references/lua-api.md`.

## Safety and evidence rules

- Do not delete Sessions or scripts, clear system logs, regenerate the CA, replace application
  configuration, or change workspace paths unless the user explicitly requests that action.
- Do not replay requests or configure another process's proxy environment automatically.
- Keep local raw capture output intact for debugging. It can include authorization headers, cookies,
  tokens, and personal data. Redact those values from summaries, chat responses, tickets, and other
  shared artifacts unless the user explicitly needs them.
- Treat request and response bodies with side effects or credentials as sensitive.
- Prefer a dedicated routing tag and tight filters over reading a large unrelated capture history.
- Binary and oversized bodies are represented by type and size only; the management API does not
  expose their raw bytes.
- Interceptor mutations applied before a Lua runtime error remain applied. Inspect both
  `modifications` and `error`.
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
