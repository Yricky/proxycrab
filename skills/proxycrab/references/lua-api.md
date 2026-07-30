# ProxyCrab Lua 5.4 API

ProxyCrab syntax-checks every saved script and runs it in a fresh sandbox.

## Sandbox and limits

- Lua version: 5.4
- Instruction limit: 100,000 VM instructions
- Lua-managed memory limit: 16 MiB
- Unavailable globals/libraries: `io`, `os`, `package`, `debug`, `dofile`, `loadfile`, `require`
- Script files use the `.lua` suffix.
- Request/response body replacement is limited to 64 MiB.

Do not depend on filesystem or process APIs other than the explicit
`replace_with_file("/absolute/path")` body method.

## Filter scripts

The filter-bar input is the Lua chunk's first vararg. `entry` is a read-only global. A filter must
explicitly return a boolean.

```lua
local input = ...
return entry.req.uri.host:find(input, 1, true) ~= nil
   and entry.resp ~= nil
   and entry.resp.status >= 500
```

The input is exact, including leading and trailing whitespace. During normal log filtering,
runtime errors, instruction exhaustion, or non-boolean results count as non-matches without a system
log entry. `/api/filter-scripts/{name}/debug` reports those errors directly.

Guard `entry.resp` before reading it because in-progress and failed captures might not have one.

## Custom-column scripts

`entry` is the only ProxyCrab-specific global. Return `nil`, string, number, or boolean:

```lua
return entry.req.headers:get("x-request-id")
```

`nil` renders as an empty string. Tables, functions, threads, and userdata are rejected.

## Read-only capture model

### `entry`

| Field | Type | Notes |
| --- | --- | --- |
| `entry.id` | integer | Capture log ID |
| `entry.req` | request | Always present |
| `entry.resp` | response or `nil` | Missing until/unless a response exists |

### Read-only request

| Field | Type |
| --- | --- |
| `entry.req.method` | string |
| `entry.req.version` | string |
| `entry.req.uri` | URI object |
| `entry.req.headers` | read-only headers |

### Read-only response

| Field | Type |
| --- | --- |
| `entry.resp.status` | integer |
| `entry.resp.version` | string |
| `entry.resp.headers` | read-only headers |

### URI object

| Field | Type | Missing-value behavior |
| --- | --- | --- |
| `scheme` | string | Empty string |
| `host` | string | Empty string |
| `port` | integer or `nil` | `nil` |
| `path` | string | URI path or original unparsable value |
| `query` | string | Empty string |

Example:

```lua
local uri = entry.req.uri
return uri.scheme .. "://" .. uri.host .. uri.path
```

## Header API

Header names are matched case-insensitively.

Read-only and mutable header objects both provide:

```lua
local first = headers:get("x-name")       -- string or nil
local values = headers:get_all("x-name") -- string array, empty when absent
local all = headers:all()                 -- name -> string array snapshot
```

Mutable request/response headers additionally provide:

```lua
headers:append("x-name", "another-value")
headers:set("x-name", "only-value")
headers:remove("x-name")
```

- `append` preserves existing values and adds one value.
- `set` removes every existing case-insensitive match and stores one value.
- `remove` succeeds even when the header is absent.
- Names and values must be valid HTTP header syntax.
- Every mutation is recorded in the capture's interceptor history.

## Request interceptors

A request interceptor receives mutable global `req`:

| Field | Access | Type |
| --- | --- | --- |
| `req.method` | read-only | string |
| `req.version` | read-only | string |
| `req.uri` | read-only | URI object |
| `req.headers` | mutable methods | headers |
| `req.body` | mutable methods | body |

Example:

```lua
req.headers:remove("x-old-debug")
req.headers:set("x-debug-mode", "1")
req.body:replace_with_string('{"debug":true}')
```

Method, version, and URI cannot be changed.

## Response interceptors

A response interceptor receives mutable global `resp`:

| Field | Access | Type |
| --- | --- | --- |
| `resp.status` | read-only | integer |
| `resp.version` | read-only | string |
| `resp.headers` | mutable methods | headers |
| `resp.body` | mutable methods | body |

Example:

```lua
resp.headers:append("x-proxycrab-debug", "1")
resp.body:replace_with_file("/absolute/path/to/response.json")
```

Status and version cannot be changed.

## Mutable body API

Request and response bodies expose:

```lua
body:replace_with_string("new bytes encoded as UTF-8")
body:replace_with_file("/absolute/path/to/body.bin")
```

- Only the last body replacement in one script determines the outgoing body.
- Every replacement call is still recorded in the modification history.
- File paths must be absolute.
- The file is opened when ProxyCrab applies the effects; missing/unreadable files fail the
  interceptor stage.
- String and file bodies over 64 MiB fail.

ProxyCrab does not expose the original body content to Lua.

## Execution and historical evidence

At the beginning of a request, ProxyCrab pins the active Session and snapshots the exact source of
every enabled, present request and response interceptor. Response interceptors use the same
snapshot even if the files or Session chain change while the request is in flight.

Each executed script records:

- phase and zero-based chain position;
- historical name;
- lowercase SHA-256 of exact source;
- exact source content;
- the initial header snapshot and ordered mutations;
- an optional runtime error.

Disabled and missing scripts do not execute and are not recorded.

Mutations performed before a runtime error remain applied. Traffic continues when possible, the
error is stored on the capture, and an entry is written to the system log. Therefore inspect both
`modifications` and `error`; an error does not imply “no effect.”

## Patterns

### Compound filter with nil safety

```lua
local host = ...
if entry.resp == nil then
  return false
end
return entry.req.uri.host == host and entry.resp.status >= 500
```

### Correlation-ID custom column

```lua
return entry.req.headers:get("x-request-id")
    or entry.req.headers:get("x-trace-id")
```

### Add a request debug header

```lua
req.headers:set("x-debug-mode", "1")
```

### Remove response caching

```lua
resp.headers:remove("cache-control")
resp.headers:remove("expires")
resp.headers:set("cache-control", "no-store")
```

### Replace a JSON response

```lua
resp.headers:set("content-type", "application/json; charset=utf-8")
resp.body:replace_with_string('{"ok":true,"source":"proxycrab"}')
```

When changing body format, update relevant metadata such as `content-type`. The HTTP stack may
reconcile transfer framing, but scripts should not rely on stale semantic headers.
