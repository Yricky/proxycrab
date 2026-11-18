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
  const logId = requiredInteger(args, "log-id", { min: 1, max: Number.MAX_SAFE_INTEGER - 1 });
  const script = await apiRequest(args, `/api/filter-scripts/${encodeURIComponent(name)}`);
  const result = await apiRequest(args, "/api/logs/ids", {
    method: "POST",
    body: {
      session_id: sessionId,
      min_id: logId - 1,
      max_id: logId + 1,
      filter: {
        option: { kind: "script_content", content: script.content },
        input: optionalString(args, "input") ?? "",
      },
    },
  });
  printJson({ name, matched: result.matched_ids.includes(logId) });
});
