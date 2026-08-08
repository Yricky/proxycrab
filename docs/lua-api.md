# ProxyCrab Lua API

ProxyCrab embeds sandboxed Lua 5.4. `io`, `os`, `package`, `debug`, `dofile`, `loadfile`, and `require` are unavailable. Each execution is limited to 100,000 VM instructions and 16 MiB of Lua-managed memory.

Scripts are syntax-checked before being saved. Script files use the `.lua` suffix.

## Base64 and JSON

Every filter, custom-column, request-interceptor, and response-interceptor sandbox provides
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

Interceptor body objects also use the same JSON representation through `body:as_json()` as
documented below.

## Filter scripts

A global filter script receives the filter-bar input string as the Lua chunk's first vararg (`...`). It must explicitly return a boolean. `entry` remains available as a read-only global:

```lua
local input = ...
return entry.req.uri.host:find(input, 1, true) ~= nil
   and entry.resp ~= nil
   and entry.resp.status >= 500
```

The input is passed exactly, including leading and trailing whitespace. During normal list filtering, runtime errors, instruction exhaustion, and non-boolean results make that record a non-match without logging. The filter-script manager's debug action reports those errors directly.

## Custom columns

A column returns `nil`, a string, number, or boolean. `nil` renders as an empty string. Tables, functions, and userdata are rejected.

```lua
return entry.req.headers:get("x-trace-id")
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

Responses provide:

- `status`
- `version`
- `headers`

URI fields are `scheme`, `host`, `port`, `path`, and `query`.

Header names are case-insensitive:

```lua
local first = entry.req.headers:get("x-name")       -- string or nil
local values = entry.req.headers:get_all("x-name") -- string array
local all = entry.req.headers:all()                 -- name -> string array
local tag = entry.req:get_tag("trace")             -- string or nil
```

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

Five additional proxy-local tags control ordinary captured HTTP/HTTPS traffic:

| Tag | Unit | Behavior |
| --- | --- | --- |
| `_crab_req_speed` | bytes per second | Maximum speed for the final request body sent from ProxyCrab to the server |
| `_crab_resp_speed` | bytes per second | Maximum speed for the final response body sent from ProxyCrab to the client |
| `_crab_req_timeout` | milliseconds | Upstream operation timeout; defaults to `60000` |
| `_crab_resp_bodyframe_timeout` | milliseconds | Idle timeout from response headers to the first upstream body frame and between later frames; unset by default |
| `_crab_tls_insecure` | exact string `true` | Disable upstream HTTPS certificate-chain and hostname verification |

Values must match ASCII `[0-9]+`, fit in `u64`, and be greater than zero. Leading zeroes are
accepted. Invalid final values are ignored with a Rust `warn`; they do not fail the interceptor or
capture. Request speed and timeout use the values after the complete request interceptor chain.
Response speed and response-frame timeout use the values after the complete response interceptor
chain, so either phase may set or overwrite them. Breakpoint temporary scripts participate in the
same final-value behavior.
`_crab_tls_insecure` is resolved after the complete request interceptor chain and enables the
insecure TLS policy only when its final value is exactly `true`; any other present value is ignored
with a warning. It has no effect on HTTP. Verified and insecure HTTPS requests use separate
connection pools, including Upgrade/WebSocket requests, so an untagged request cannot reuse a TLS
connection established under the insecure policy. Use it only for controlled test systems.

Speed limits are independent per capture and direction. They pace the final outbound body bytes
without an initial or catch-up burst; headers, HTTP framing, and TLS overhead are not counted.
Network backpressure may make the transfer slower. `_crab_req_timeout` covers upstream connection,
TLS, paced request-body upload, and waiting for response headers. `_crab_resp_bodyframe_timeout`
starts after response interceptors and resets after every upstream response-body frame. On a normal
response, expiry terminates the downstream body stream and fails the capture. When a response body
replacement is already being returned, expiry only stops the background raw-body drain and records
a capture warning; the replacement and successful outcome are retained.

The speed and timeout tags do not apply to bypass traffic, raw CONNECT tunnels, Upgrade/WebSocket,
the local `proxy.crab/ca.crt` response, or proxy-generated error responses. With `_crab_skip`,
request speed and timeout are unused, while `_crab_resp_speed` still applies to the final synthetic
response after response interceptors. All special tags remain persisted capture metadata and are
never emitted as HTTP headers.

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
