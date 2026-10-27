# ProxyCrab debugging best practices

For authentication, mutation boundaries, and sensitive-data handling, follow [SKILL.md](../SKILL.md).

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

Prefer bounded query batches over unbounded parallel fan-out.

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

- `in_progress`: persisted unfinished state; confirm current activity through
  `ProxyStatus.running.active_netlog` before expecting further updates.
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

For TLS or upstream protocol issues, consult the [request-interceptor controls](lua-api.md#request-interceptors).
For complete or snapshot bodies, use the [Body endpoint contract](http-api.md#get-apilogsidbodysession_id3siderequestmax_size16777216).
Asset replacements are identified by their Asset ID in historical modifications.

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

## Preserve evidence

Keep API-returned output intact; it may already be redacted by the effective read policy. Before
sharing findings, check for credentials, personal data, and internal details unsuitable for the
recipient. Redaction does not cover every field; see the [policy scope](http-api.md#redaction-library-and-bindings).

Do not modify the local source evidence merely to create a redacted summary.
