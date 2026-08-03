#!/usr/bin/env node
import {
  apiRequest,
  assertAllowedArgs,
  optionalInteger,
  optionalString,
  parseArgs,
  printHelp,
  printJson,
  readText,
  requiredInteger,
  requiredString,
  run,
  UsageError,
} from "./lib/common.mjs";

run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, ["id", "action", "timeout-ms", "file"]);
  if (args.help) {
    printHelp(`
Usage: node breakpoint-control.mjs --id ID --action detail|extend|release|execute
       [--timeout-ms MS] [--file /absolute/script.lua] [--base-url URL]

Read or control one active breakpoint. Execute applies a temporary script and keeps it paused.
`);
    return;
  }
  const id = requiredInteger(args, "id", { min: 1 });
  const action = requiredString(args, "action");
  const base = `/api/breakpoints/${id}`;
  if (action === "detail") {
    printJson(await apiRequest(args, base));
    return;
  }
  if (action === "extend") {
    const timeoutMs = optionalInteger(args, "timeout-ms", { min: 0 });
    if (timeoutMs === undefined) throw new UsageError("--timeout-ms is required for extend");
    printJson(await apiRequest(args, `${base}/extend`, { method: "POST", body: { timeout_ms: timeoutMs } }));
    return;
  }
  if (action === "release") {
    printJson(await apiRequest(args, `${base}/release`, { method: "POST" }));
    return;
  }
  if (action === "execute") {
    const file = optionalString(args, "file");
    if (file === undefined) throw new UsageError("--file is required for execute");
    printJson(await apiRequest(args, `${base}/execute`, { method: "POST", body: { content: await readText(file) } }));
    return;
  }
  throw new UsageError("--action must be detail, extend, release, or execute");
});
