# ProxyCrab Lua API

ProxyCrab embeds sandboxed Lua 5.4. `io`, `os`, `package`, `debug`, `dofile`, `loadfile`, and `require` are unavailable. Each execution is limited to 100,000 VM instructions and 16 MiB of Lua-managed memory.

Scripts are syntax-checked before being saved. Script files use the `.lua` suffix.

## Script display names

All six script kinds (custom columns, filters, routing, request interceptors, response
interceptors, and redaction) support an optional display name in the leading Lua comment header:

```lua
-- #display_name: 鉴权token
```

Only the initial blank lines and single-line `--` comments are scanned. Parsing stops at code
or a Lua long comment (`--[[...]]` / `--[=[...]=]`). Whitespace around the colon and value is
trimmed; the first `#display_name` declaration wins, including an empty declaration. Missing or
empty values fall back to the script's original name. Unknown annotations are ignored.

This changes UI labels only. File names, API paths, `name`, `script_name`, selections, and Session
references remain unchanged. Duplicate display names are allowed; tooltips and delete confirmations
retain the original name. Script editor sidebars show original names; the editor toolbar shows
the display name (falling back to the original). Custom-column header menus provide 编辑脚本 at
the top to open and select that script, including in an already-open editor. Labels refresh after saving. Historical interceptor executions use the
metadata from their captured script source, not the current script file. The share UI receives the
parsed display name while script content remains hidden. Metadata is derived from the Lua source,
not stored in a separate settings file.

## Base64 and JSON

Every filter, custom-column, redaction, request-interceptor, and response-interceptor sandbox provides
read-only `base64` and `json` modules. Codec inputs and outputs are limited to 16 MiB. Errors raise
a Lua runtime error and never return a partial result.

### Base64

Lua strings are byte strings, so Base64 functions accept and return arbitrary binary data,
including `\0`:

```lua
local standard = base64.encode("hello")       -- "aGVsbG8="
local decoded = base64.decode(standard)       -- "hello"

local padded = base64.url_encode("hello", true)
local unpadded = base64.url_encode("hello", false)
local original = base64.url_decode(unpadded)
```

- `base64.encode(data)` uses the standard alphabet and always writes canonical `=` padding.
- `base64.decode(text)` accepts only the standard alphabet with canonical padding.
- `base64.url_encode(data, with_padding)` uses the URL-safe alphabet; the boolean padding argument
  is required.
- `base64.url_decode(text)` accepts canonical URL-safe text with or without padding.
- Decoders reject whitespace, mixed alphabets, invalid characters, and invalid padding.

### JSON

```lua
local text = json.encode({
  ok = true,
  missing = json.null,
  items = json.array({ "a", "b" }),
})
local value = json.decode(text)
```

`json.encode(value)` and `json.decode(text)` support JSON null, booleans, numbers, UTF-8 strings,
arrays, and objects. `json.encode(nil)` and `json.encode(json.null)` both return `"null"`. Use
`json.null` inside a table to preserve an object field or array position; assigning Lua `nil`
removes the key normally.

Unmarked Lua tables use these encoding rules:

- consecutive positive integer keys `1..n` encode as an array;
- string-only keys encode as an object;
- an empty table encodes as `{}`;
- sparse arrays, mixed integer/string keys, unsupported key types, cycles, functions, threads, and
  ordinary userdata are rejected;
- shared non-cyclic tables are serialized independently;
- object keys are emitted in lexicographic order for stable output.

Use `json.array(table)` or `json.object(table)` to force the container type, including `[]` versus
`{}` for empty tables. They mark and return the same table, reject a table that already has a
metatable, and install a protected type marker. Decoded arrays and objects carry the same protected
markers, so empty containers round-trip without changing type. The tables remain mutable through
normal indexing, `pairs`, `ipairs`, and `table.insert`, but their metatables cannot be replaced.

JSON input must be valid UTF-8 and contain exactly one value followed only by whitespace. Duplicate
object keys use the last value. Integers in Lua's signed 64-bit range remain Lua integers; larger
finite numbers become Lua floating-point numbers and may lose precision. One script execution
writes at most one aggregated `WARN` system log when its successful decode calls replaced duplicate
keys or performed lossy numeric conversion. The warning includes script identity, Capture Log ID
when available, and counts, but never includes JSON content or original values. A failed decode
does not emit these conversion warnings.

JSON nesting is limited to 128 containers. Encoding rejects `NaN` and infinities; decoding rejects
numbers outside the finite `f64` range. JSON output is compact rather than pretty-printed.

Interceptor bodies and persisted capture bodies also use the same JSON representation through
`body:as_json()` as documented below.

## Filter scripts

A global filter script receives the filter-bar input string as the Lua chunk's first vararg (`...`). It must explicitly return a boolean. `entry` remains available as a read-only global:

```lua
local input = ...
if entry.req.uri.host ~= "api.example.com" then
  return false
end
local body = entry.req.body:as_string()
return body ~= nil and body:find(input, 1, true) ~= nil
```

The input is passed exactly, including leading and trailing whitespace. During normal list filtering, runtime errors, instruction exhaustion, and non-boolean results make that record a non-match without logging. The filter-script manager's debug action reports those errors directly.
Check inexpensive metadata before reading a body because persisted body access performs lazy file
I/O and may decompress up to 16 MiB for every evaluated capture.

## Custom columns

A column returns `nil`, a string, number, or boolean. `nil` renders as an empty string. Tables, functions, and userdata are rejected.

```lua
local body = entry.resp and entry.resp.body:as_json()
return body and body.result or nil
```

## Read-only capture objects

`entry` provides:

- `entry.id`
- `entry.req`
- `entry.resp`, which is `nil` before a response exists

Requests provide:

- `method`
- `version`
- `uri`
- `headers`
- read-only `body`

Responses provide:

- `status`
- `version`
- `headers`
- read-only `body`

URI fields are `scheme`, `host`, `port`, `path`, and `query`.

Header names are case-insensitive:

```lua
local first = entry.req.headers:get("x-name")       -- string or nil
local values = entry.req.headers:get_all("x-name") -- string array
local all = entry.req.headers:all()                 -- name -> string array
local tag = entry.req:get_tag("trace")             -- string or nil
```

Persisted request and response bodies provide only `as_string()` and `as_json()`:

```lua
local request_text = entry.req.body:as_string()
local response_json = entry.resp and entry.resp.body:as_json()
```

The first getter lazily loads the persisted final Body source recorded on the capture (original
network bytes, a complete string replacement, or an Asset), and later getters for that capture side
reuse the decoded bytes. Bodies on
`in_progress` captures are unavailable and both getters return `nil`, even if a partial capture file
already exists. The getters require a textual `Content-Type`; `as_string()` also requires valid
UTF-8, while `as_json()` returns `nil` for malformed JSON. Empty text returns `""` / `nil`.
gzip, br, deflate, and zstd are decoded first; unknown or malformed encodings return `nil` and
decoded content over 16 MiB raises a Lua runtime error. Body replacement methods remain available
only to request and response interceptors.

## Request interceptors

```lua
req.method = "BREW"
req.uri = "https://alternate.example.com/new-path?q=1"
req.headers:remove("x-env")
req.headers:append("x-env", "staging")
req.headers:set("x-use-staging", "1")
req:set_tag("environment", "staging")
local body = req.body:as_json()
req.body:replace_with_string("new request body")
breakpoint(30000)
```

`req.method`, `req.uri`, `req.headers`, and `req.body` are mutable; `req.version` remains read-only.
Method accepts standard or extension HTTP tokens. URI accepts any value representable by the HTTP
stack; normal upstream forwarding requires an absolute URI with a host. Changing URI does not
automatically rewrite the `Host` header, so scripts can either preserve a mismatched Host or update
it explicitly.
`req:set_tag(key, value)` stores a proxy-local string tag and `req:get_tag(key)` returns its value or
`nil`; an empty value still means the tag exists. Tags are persisted with the capture but are never
sent to the server. After all request interceptors finish, the presence of `_crab_skip` skips the
upstream request, creates an empty HTTP/1.1 200 response, and continues through response interceptors.

Six additional proxy-local tags control captured HTTP/HTTPS traffic:

| Tag | Unit | Behavior |
| --- | --- | --- |
| `_crab_req_speed` | bytes per second | Maximum speed for the final request body sent from ProxyCrab to the server |
| `_crab_resp_speed` | bytes per second | Maximum speed for the final response body sent from ProxyCrab to the client |
| `_crab_req_timeout` | milliseconds | Upstream operation timeout; defaults to `60000` |
| `_crab_resp_bodyframe_timeout` | milliseconds | Idle timeout from response headers to the first upstream body frame and between later frames; unset by default |
| `_crab_tls_insecure` | exact string `true` | Disable upstream HTTPS certificate-chain and hostname verification |
| `_crab_upstream_http_version` | `HTTP/1.1` or `HTTP/2` | Force the HTTP version used from ProxyCrab to the upstream server |

The four speed/timeout values must match ASCII `[0-9]+`, fit in `u64`, and be greater than zero.
Leading zeroes are accepted. Invalid final values are ignored with a Rust `warn`; they do not fail
the interceptor or capture. Request speed and timeout use the values after the complete request interceptor chain.
Response speed and response-frame timeout use the values after the complete response interceptor
chain, so either phase may set or overwrite them. Breakpoint temporary scripts participate in the
same final-value behavior.
`_crab_tls_insecure` is resolved after the complete request interceptor chain and enables the
insecure TLS policy only when its final value is exactly `true`; any other present value is ignored
with a warning. It has no effect on HTTP. Verified and insecure HTTPS requests use separate
connection pools, including Upgrade/WebSocket requests, so an untagged request cannot reuse a TLS
connection established under the insecure policy. Use it only for controlled test systems.

`_crab_upstream_http_version` is also resolved after the complete request interceptor chain. Only
the exact values `HTTP/1.1` and `HTTP/2` are accepted; any other present value is ignored with a
warning and keeps the default behavior of selecting the upstream protocol from the captured request.
The override affects only the ProxyCrab-to-upstream hop, including Upgrade/WebSocket requests. It
does not change the captured request version or the HTTP version used between the client and
ProxyCrab. Forced `HTTP/2` is strict: HTTPS advertises only `h2` through ALPN and fails if the server
does not negotiate it; cleartext HTTP uses direct h2c prior knowledge without an HTTP/1.1 fallback.

Speed limits are independent per capture and direction. They pace the final outbound body bytes
without an initial or catch-up burst; headers, HTTP framing, and TLS overhead are not counted.
Network backpressure may make the transfer slower. `_crab_req_timeout` covers upstream connection,
TLS, paced request-body upload, and waiting for response headers. `_crab_resp_bodyframe_timeout`
starts after response interceptors and resets after every upstream response-body frame. On a normal
response, expiry terminates the downstream body stream and fails the capture. When a response body
replacement is already being returned, expiry only stops the background raw-body drain and records
a capture warning; the replacement and successful outcome are retained.

The speed and timeout tags do not apply to bypass traffic, raw CONNECT tunnels, Upgrade/WebSocket,
or proxy-generated error responses. With `_crab_skip`,
request speed and timeout are unused, while `_crab_resp_speed` still applies to the final synthetic
response after response interceptors. The upstream-version override applies to Upgrade/WebSocket but
not bypass traffic, raw CONNECT tunnels, or proxy-generated errors. All
special tags remain persisted capture metadata and are never emitted as HTTP headers.

## Response interceptors

```lua
if req.method == "GET" and req.uri.path == "/api/example" then
  resp.status = 777
end
resp.headers:append("x-proxy-crab-debug", "1")
req:set_tag("response-debugged", "")
local asset = get_asset("fixtures/example.json")
if asset ~= nil then
  resp.headers:set("content-type", asset.content_type)
  resp.body:replace_with_asset(asset)
end
```

Response interceptors receive read-only `req.method`, `req.version`, `req.uri`, and `req.headers`,
plus mutable request tags through `set_tag` and `get_tag`; `req.body` is unavailable in response
interceptors.
`resp.status` is mutable and accepts any integer from 100 through 999, including non-standard
status codes and status/body combinations. `resp.version` remains read-only.

Request interceptors run once after downstream request headers arrive. Response interceptors run
once after upstream response headers arrive. The first `as_string()` or `as_json()` call that needs
the original body waits for it to finish downloading and being captured, then ProxyCrab replays it
from the capture file. Without a getter call, bodies retain their normal streaming behavior. SSE or
an infinite stream can therefore block a getter indefinitely unless the configured response-frame
timeout terminates it.

## Interceptor body and Asset API

Request and response interceptor bodies provide:

```lua
local text = req.body:as_string() -- string or nil
local value = resp.body:as_json() -- decoded value or nil
body:replace_with_string("new body")
local asset = get_asset("fixtures/body.json")
if asset ~= nil then body:replace_with_asset(asset) end
```

The getters read the effective body at the instant of the call: the latest replacement in the
current script, then a replacement made by an earlier interceptor, otherwise the original body.
They use the current effective `Content-Type`; non-textual content returns `nil`. `as_string()` also
returns `nil` for invalid UTF-8. `as_json()` attempts JSON parsing for every textual content type and
returns `nil` for malformed JSON. An empty textual body is `""` from `as_string()` and `nil` from
`as_json()`. gzip, br, deflate, and zstd content encodings are decoded first; unknown or malformed
encoding returns `nil`. Decoded content over 16 MiB raises a runtime error.

`get_asset(id)` is available to request and response interceptors. It returns `nil` when the ID is
invalid or absent, and otherwise returns a read-only Asset with `id`, `size`, `content_type`,
`sha256`, and `created_at`. `replace_with_asset` accepts only this Asset object and streams its
workspace file without loading it into Lua memory. Assets are workspace-wide and immutable through
the API. Both replacement methods remove an existing `Content-Encoding` header and record that
removal; neither changes `Content-Type`, so the script should set it when appropriate.

Asset IDs use only lowercase ASCII letters, digits, `_`, `.`, and `/`; they cannot start or end in
`/`, contain `//`, or contain `.`, `..`, or `.metadata` as a path segment. IDs are at most 255 bytes
and each segment is at most 100 bytes. Upload and download Assets through the management API
documented in `backend-api.md`.

`breakpoint(timeoutMs)` is available in saved request and response interceptors. Zero returns
immediately. A positive value pauses the current request until the timeout or manual release; the
desktop UI can repeatedly extend the cumulative wait up to 1,800 seconds and execute temporary Lua
against the same live request/response state. Temporary scripts have the same phase capabilities,
except they cannot call `breakpoint`. Each temporary run is stored as its own interceptor execution.

Interceptor changes made before a runtime error remain applied. The error is stored on the capture and written to the system log; traffic continues when possible.

## Read-time redaction scripts

Redaction is a read policy bound to a local management identity, ordinary API key, or Session share. An unbound identity receives the original data. A bound missing/broken script causes
`redaction_failed`; there is no fallback to original content. Forwarding, live interceptor execution,
and persisted captures never run redaction. Interceptor modification history and executed source are
outside this policy; historical snapshots and their bodies are redacted independently.

The globals are `req`, `resp` (nil when absent), and `context`:

```lua
-- #display_name: Hide credentials
if context.req.header == "loaded" and req and req.headers then
  req.headers:remove("authorization")
  req.headers:remove("cookie")
end
if resp and context.resp.header == "loaded" then
  resp.headers:remove("set-cookie")
end
if resp and context.resp.body == "loaded" then
  local value = resp.body:as_json()
  if value then
    value.token = "[redacted]"
    resp.body:replace_with_string(json.encode(value))
  end
end
```

`context.api` is the route template (for example `/api/logs/{id}` or `/share-api/logs/views`), without IDs or query tokens. `context.req.header`, `context.req.body`,
`context.resp.header`, and `context.resp.body` each contain `unloaded`, `loaded`, or `redacted`.
Header state includes request URI and message metadata. Body availability implies header
availability. Unloaded fields are nil. URI built-in filtering and URI-only summaries provide only
`req.uri`; `req.headers`, `req.body`, `req.method`, and `resp` are nil. Other built-in filters and
empty filters do not execute redaction. Custom-column filters and filter scripts operate on the
redacted input, with bodies loaded only when accessed.

Within a redactor, getters always return the original value for this record/snapshot, including
reads after writes and reads of already-redacted parts. Only parts `loaded` at invocation start
accept writes. Header `get/get_all/all/set/append/remove`, request `uri` assignment, and Body
`as_string/as_json/replace_with_string` are supported. Method, status and HTTP version are read-only.
Successful completion (including early return/no edits) marks every loaded part redacted and commits
all writes. Failure commits none and refuses output. Header decisions that depend on a later body
cannot be deferred by early return. Body replacement normalizes output encoding/length metadata;
original encoding metadata is retained privately for decoding original bytes. A successfully read
original text Body is cached for the flow and reused by later stages and output, so subsequent file
appends cannot change the data already examined by the script. Text body decoding
errors and size-limit errors fail the read. Binary `as_string/as_json` returns nil; replacing the
body with a string is supported without reading its original bytes.

Each API request compiles its selected script once and isolates globals between records/stages.
States and output are scoped to a record/snapshot in that request. Frontend HAR derives query
strings, cookies, redirects and Body fields from these authorized reads. Each request resolves its
own policy version; failed reads become export diagnostics without fallback to original data.
No cross-request or cross-snapshot result cache is used.

## WebSocket interception callbacks

A connection request interceptor can register **one** callback with `req:on_ws_message(function(ws) ... end)`.
Registration is unavailable in temporary breakpoint scripts; attempts raise a Lua error.
The runtime retains the Lua closure and its captured variables for the upgraded connection. Only
successful HTTP/1.1 WebSocket upgrades activate callbacks. Editing a library script affects new
connections, not the closures or source snapshots of existing ones.

```lua
local received = 0
req:on_ws_message(function(ws)
    received = received + 1
    local message = ws.message
    if message.direction == "request" and message.type == "text" and message.body == "ping" then
        ws:drop()
        ws:send("response", "pong", "text")
    end
end)
```

| API | Behavior |
| --- | --- |
| `ws.message` | Read-only current complete message. `body` is a Lua byte string; `direction`/`side` is `request` (to server) or `response` (to client); `type` is `text` or `binary`. Metadata includes IDs and provenance described in the HTTP reference. |
| `ws:drop()` | Stops this message before later callbacks/delivery; still records it with `status=dropped`. Does not return from the Lua function or cancel its sends. |
| `ws:send(direction, body, type?)` | Stages a new message for either direction. `type` defaults to `text`; text must be valid UTF-8, binary accepts arbitrary bytes. Payload limit is 16 MiB. |
| `ws:history(after_sequence?, limit?)` | Read-only page of finalized messages from both directions, including generated, dropped and failed messages. Defaults: after sequence 0, limit 64; limit is clamped to 1–256. Access `#page` and `page[i]`; advance using the last item's `sequence`. History excludes the current message and queued messages. |

Current messages, history entries, and nested provenance metadata are read-only. To replace a
message, call `drop()` and `send()` to the same direction. During callbacks `req` and its previously
captured headers/body aliases are read-only; request mutations, new registration and request
breakpoints are unavailable. Captured local variables remain mutable. `ws:drop`, `ws:send` and
`ws:history` are scoped to the active invocation; retaining an old `ws` does not permit later writes.

Callbacks run serially per connection in request-interceptor registration order. Each original
network message completes its entire derived chain before the next original message starts.
Original messages proceed through the chain before their sends. Successful sends append to one
FIFO queue in call order. A generated message starts at the callback **after its sender**, even if
its direction differs; it never invokes the sender or earlier callbacks. For example, A processing
M emits X1/X2, then B processing M emits Y: the order is M, X1, X2, Y. If X1 emits Z, Z goes behind Y.
Ping, Pong and Close are handled by the protocol layer and do not invoke Lua callbacks.

A callback's sends/drop commit together only when it succeeds. Errors or instruction-budget
exhaustion cancel those effects, record an invocation error and pass the unchanged message to the
next callback. The callback is invoked again for subsequent messages. Lua variable changes are not
rolled back. The pending FIFO limit is **16384 messages**, counting the invocation's staged sends,
excluding history and the message currently processing. Overflow fails the invocation even if
its Lua error is caught, cancelling all of that invocation's staged sends/drop; previously committed
queue entries continue processing.

Calling `on_ws_message` twice in one request script raises a Lua error. If caught with `pcall`, the
first registration remains; otherwise request-script failure cancels its registration. Any other
uncaught request-script error likewise cancels registration.

Every final record preserves its root network-message reference and direct parent reference.
Generated records also identify source script `execution_id`, callback `invocation_id`, and
`send_index`. Dropped records identify the dropping invocation. Failed/unsent records retain a
reason and body. Read the connection's source snapshots using its handshake log and registered
execution IDs, rather than the current script library. Compression (`permessage-deflate`) is
supported; callbacks see decoded, reassembled payloads.

## WebSocket read-time redaction

A bound script must explicitly opt in using an exact standalone comment:

```lua
-- @proxycrab websocket-messages
if context.kind == "websocket_message" then
    local current = req or resp
    -- req exists only for C2S; resp exists only for S2C.
    if context.type == "text" then
        local value = current.body:as_json()
        value.access_token = nil
        current.body:replace_with_json(value)
    else
        current.body:hide()
    end
end
```

The message invocation provides read-only `context`: kind, api, session_id, log_id, id, type,
started_at, completed_at, size, content_available, omitted_reason. `req`/`resp` offers read-only
connection `uri` (the usual URI view), `version` (`WebSocket/13`), and current-message `body`.
No top-level `message` object, HTTP headers/method/status, or raw container is exposed.
Message body methods: `as_string()` (UTF-8), `as_bytes()` (Lua binary string), `as_json()`,
`replace_with_string(value)`, `replace_with_bytes(value)`, `replace_with_json(value)`, `hide()`.
Reads observe original message bytes; replacements stage output and do not modify capture/forwarding.
`hide()` differs from replacing with an empty string. Size limits and script failures are fail-closed.

The opt-in applies only to message invocations; scripts must still handle ordinary HTTP reads.
Legacy bound scripts cannot read WS content until opted in. Metadata indexes expose no payload and
remain available: original timing, direction, type and byte size are intentionally not anonymized.
Identity/share bindings and process-forced policies use the existing precedence. No binding means
original content according to ordinary read permission. URI/handshake redaction remains the existing
HTTP flow. Under policies, close reasons are suppressed rather than exposed outside message redaction.
