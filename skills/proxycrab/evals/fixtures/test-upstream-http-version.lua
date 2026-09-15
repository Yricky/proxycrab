if req.uri.host == "app.example.test" then
  local target = "http://127.0.0.1:3001" .. req.uri.path
  if req.uri.query ~= "" then
    target = target .. "?" .. req.uri.query
  end
  req.uri = target
  req.headers:set("host", "127.0.0.1:3001")
  req:set_tag("_crab_upstream_http_version", "HTTP/1.1")
end
