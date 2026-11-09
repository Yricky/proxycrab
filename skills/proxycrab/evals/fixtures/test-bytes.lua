-- #display_name: 二进制标记
-- Request interceptor; no resource dependencies. Generated output is persisted as an Asset.
if req.uri.path ~= "/proxycrab-skill-eval/bytes" then return end
local data = req.body:as_bytes()
if data == nil or #data < 4 then return end
local original = data:clone()
data:pack(1, ">I4", 12345)
assert(data:unpack(">I4") == 12345)
req.body:replace_with_bytes(data)
data:set(1, 255) -- Does not change the submitted Asset or the original snapshot.
assert(req.body:as_bytes():unpack(">I4") == 12345)
assert(#original == #data)
