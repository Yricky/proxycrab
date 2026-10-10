#!/usr/bin/env node
import {
  UsageError,
  apiRequest,
  assertAllowedArgs,
  optionalString,
  parseArgs,
  printHelp,
  printJson,
  run,
} from "./lib/common.mjs";

function optionalBoolean(args, key) {
  const value = optionalString(args, key);
  if (value === undefined) return undefined;
  if (value !== "true" && value !== "false") {
    throw new UsageError(`--${key} must be "true" or "false"`);
  }
  return value === "true";
}

function optionalJson(args, key) {
  const value = optionalString(args, key);
  if (value === undefined) return undefined;
  try {
    return JSON.parse(value);
  } catch {
    throw new UsageError(`--${key} must be valid JSON`);
  }
}

run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, ["name", "description", "view", "interceptors", "active"]);
  if (args.help) {
    printHelp(`
Usage: node session-create.mjs [--name NAME] [--description TEXT] [--view JSON] [--interceptors JSON] [--active true|false] [--base-url URL]

Create a ProxyCrab Session. ProxyCrab generates a name when --name is omitted.
--view presets the Session view, e.g. '{"columns":[{"kind":"stage","width":91}],"filter":{"option":null,"input":"example"}}'.
--interceptors presets the interceptor chains by name, e.g. '{"request":[{"name":"rewrite","enabled":true}],"response":[]}'.
--active true makes the new Session the active one with a second PUT /api/active-session call, which closes every established proxy connection, CONNECT tunnel, Upgrade/WebSocket, and upstream pooled connection before returning.
`);
    return;
  }
  const session = await apiRequest(args, "/api/sessions", {
    method: "POST",
    body: {
      name: optionalString(args, "name") ?? null,
      description: optionalString(args, "description") ?? null,
      view: optionalJson(args, "view") ?? null,
      interceptors: optionalJson(args, "interceptors") ?? null,
    },
  });
  // 创建不含活跃语义：需要活跃会话时显式调用 PUT /api/active-session。
  if (optionalBoolean(args, "active") === true) {
    await apiRequest(args, "/api/active-session", {
      method: "PUT",
      body: { session_id: session.id },
    });
  }
  printJson(session);
});
