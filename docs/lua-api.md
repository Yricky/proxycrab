# ProxyCrab Lua API

ProxyCrab embeds sandboxed Lua 5.4. `io`, `os`, `package`, `debug`, `dofile`, `loadfile`, and `require` are unavailable. Each execution is limited to 100,000 VM instructions and 16 MiB of Lua-managed memory.

Scripts are syntax-checked before being saved. Script files use the `.lua` suffix.

## Filters

A filter must explicitly return a boolean:

```lua
return entry.req.uri.host == "example.com"
   and entry.resp ~= nil
   and entry.resp.status >= 500
```

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
