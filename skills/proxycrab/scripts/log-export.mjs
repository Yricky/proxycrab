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
  run,
  sessionQuery,
  writeJson,
} from "./lib/common.mjs";

run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, ["session-id", "log-id", "output"]);
  if (args.help) {
    printHelp(`
Usage: node log-export.mjs --log-id ID [--session-id ID] [--output FILE] [--base-url URL]

Export one raw capture detail as formatted JSON. The default file is proxycrab-log-<ID>.json
in the current directory. Text and JSON bodies include content; binary/large bodies include
metadata only.
`);
    return;
  }
  const sessionId = optionalInteger(args, "session-id", { min: 1 });
  const logId = requiredInteger(args, "log-id", { min: 1 });
  const output = optionalString(args, "output") ?? `proxycrab-log-${logId}.json`;
  const detail = await apiRequest(args, `/api/logs/${logId}${sessionQuery(sessionId)}`);
  const outputPath = await writeJson(output, detail);
  printJson({ path: outputPath, session_id: detail.session_id, log_id: detail.id });
});
