#!/usr/bin/env node
import {
  apiRequest,
  assertAllowedArgs,
  optionalInteger,
  optionalString,
  parseArgs,
  printHelp,
  printJson,
  requiredInteger,
  requiredString,
  run,
} from "./lib/common.mjs";

run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, ["name", "session-id", "log-id", "input"]);
  if (args.help) {
    printHelp(`
Usage: node filter-debug.mjs --name NAME --log-id ID [--session-id ID] [--input TEXT]

Evaluate one saved Lua filter against one capture and report its boolean result or runtime error.
`);
    return;
  }
  const name = requiredString(args, "name");
  const sessionId = optionalInteger(args, "session-id", { min: 1 });
  const body = {
    ...(sessionId === undefined ? {} : { session_id: sessionId }),
    log_id: requiredInteger(args, "log-id", { min: 1 }),
    input: optionalString(args, "input") ?? "",
  };
  const matched = await apiRequest(
    args,
    `/api/filter-scripts/${encodeURIComponent(name)}/debug`,
    { method: "POST", body },
  );
  printJson({ name, matched });
});
