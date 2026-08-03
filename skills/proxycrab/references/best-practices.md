# ProxyCrab debugging best practices

## Scope first

Use a dedicated active Session when possible. Record the Session ID in notes and commands so later
active-Session changes cannot silently redirect reads. Remember that direct HTTP routing runs per
request, while a CONNECT result pins one Session for the whole tunnel.

Start with the narrowest stable discriminator available:

1. an exact or distinctive URI substring;
2. request method;
3. response status;
4. a correlation header exposed by a custom column;
5. a Lua filter for compound conditions.

The API stores one filter per Session. A filtered `POST /api/logs/ids` updates that persisted filter
after a successful scan. Do not assume filtering is read-only.

## Separate observation from mutation

Inspect at least one unmodified capture before adding an interceptor. When testing an interceptor:

1. read and save the current Session chains;
2. create or update one small interceptor;
3. attach it at a deliberate position;
4. generate new traffic;
5. inspect the new capture's historical execution;
6. compare the request or response with the baseline.

A successful save only proves syntax validation passed. The capture history is the evidence that the
script executed and shows exactly which modifications were recorded.

## Diagnose by lifecycle

- `in_progress`: the capture can still gain a response or error.
- `success`: transport completed, but application-level status can still be an error.
- `failed`: use `error.stage`, `error.kind`, and `error.message`.
- `tunneled`: traffic was passed through without decrypted HTTP details.

Common stages:

- `connect`: upstream connection failure or invalid target.
- `tls_handshake`: certificate, protocol, or trust failure.
- `request_body`: request body timeout or size failure.
- `interceptor`: Lua or body replacement failure.
- `upstream`: upstream request/response failure.
- `response_body`: response body timeout or size failure.

For CONNECT traffic, a successful TLS MITM handshake retains a CONNECT capture and records decrypted
requests as additional captures. Do not mistake the CONNECT row for the application request.
Bypassed CONNECT traffic has no decrypted request detail; inspect the bypass table for source,
authority, reason, outcome, byte counts, and errors.

## Handle bodies accurately

`text` and `json` bodies include their content. `binary` and `large` bodies include only a size.
Never claim to have inspected bytes that the API did not return.

When an interceptor replaces a body from a file, the historical modification records the absolute
path, not a durable copy of that file's bytes. The interceptor source and modification history are
durable capture evidence; the external file may later change.

## Report with traceable evidence

A useful diagnostic report contains:

- Session ID and capture log IDs;
- capture time, method, URI, outcome, and response status;
- proxy error stage/kind/message when present;
- relevant request and response facts;
- interceptor execution order, modifications, and errors;
- the most likely cause and credible alternatives;
- the next discriminating test.

Distinguish direct observations from inference. Avoid dumping the entire capture when a few fields
support the conclusion.

## Protect sensitive data

Raw local script output intentionally remains unredacted. Before sharing findings, redact or omit:

- `authorization`, `proxy-authorization`, cookies, and set-cookie values;
- API keys, signed URLs, session identifiers, and bearer tokens;
- personal or regulated data in bodies;
- internal hostnames or IPs when the audience should not receive them.

Do not modify the local source evidence merely to create a redacted summary.
