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

These codecs do not expose captured request or response body content. Interceptors still only
replace bodies through the methods documented below.

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
```

## Request interceptors

```lua
req.headers:remove("x-env")
req.headers:append("x-env", "staging")
req.headers:set("x-use-staging", "1")
req.body:replace_with_string("new request body")
```

`req.method`, `req.version`, and `req.uri` are read-only. `req.headers` and `req.body` are mutable.

## Response interceptors

```lua
resp.headers:append("x-proxy-crab-debug", "1")
resp.body:replace_with_file("/absolute/path/to/body.bin")
```

`resp.status` and `resp.version` are read-only. File replacement requires an absolute path and is limited to 64 MiB.

Interceptor changes made before a runtime error remain applied. The error is stored on the capture and written to the system log; traffic continues when possible.
