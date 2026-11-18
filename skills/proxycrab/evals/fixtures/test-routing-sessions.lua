-- Route local test traffic to an existing non-archived Session, even when it is inactive.
if source.ip == "127.0.0.1" then
  for _, session in ipairs(get_sessions()) do
    if session.name == "proxycrab-skill-eval-route" then
      return session.id
    end
  end
end
-- Preserve the existing active-Session fallback (bypass when none is active).
return true
