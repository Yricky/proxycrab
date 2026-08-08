if req.uri.scheme == "https" and req.uri.host == "self-signed.test" then
  req:set_tag("_crab_tls_insecure", "true")
end
