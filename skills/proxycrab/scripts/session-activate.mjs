#!/usr/bin/env node
import {
  apiRequest,
  assertAllowedArgs,
  parseArgs,
  printHelp,
  printJson,
  requiredInteger,
  run,
} from "./lib/common.mjs";

run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, ["session-id"]);
  if (args.help) {
    printHelp(`
Usage: node session-activate.mjs --session-id ID [--base-url URL]

Activate a ProxyCrab Session for newly captured traffic.
`);
    return;
  }
  const sessionId = requiredInteger(args, "session-id", { min: 1 });
  printJson(await apiRequest(args, `/api/sessions/${sessionId}/activate`, { method: "POST" }));
});
