local path = ...
return entry.req.uri.path:find(path, 1, true) ~= nil
   and entry.resp ~= nil
   and entry.resp.status >= 500
