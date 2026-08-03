#!/usr/bin/env node
import {
  apiRequest,
  assertAllowedArgs,
  optionalInteger,
  parseArgs,
  printHelp,
  printJson,
  requiredInteger,
  run,
  sessionQuery,
} from "./lib/common.mjs";

run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, ["session-id", "log-id"]);
  if (args.help) {
    printHelp(`
Usage: node log-get.mjs --log-id ID [--session-id ID] [--base-url URL]

Read one complete capture detail. Omitting --session-id uses the active Session.
Raw headers and bodies are not redacted.
`);
    return;
  }
  const sessionId = optionalInteger(args, "session-id", { min: 1 });
  const logId = requiredInteger(args, "log-id", { min: 1 });
  printJson(await apiRequest(args, `/api/logs/${logId}${sessionQuery(sessionId)}`));
});
