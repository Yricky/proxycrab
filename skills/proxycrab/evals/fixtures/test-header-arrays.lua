local headers = resp and resp.headers or req.headers
headers:set("x-proxycrab-skill-eval", {"first", "second"})
headers:set("x-proxycrab-skill-eval-remove", "old")
headers:set("x-proxycrab-skill-eval-remove", {})
