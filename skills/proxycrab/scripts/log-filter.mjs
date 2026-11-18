#!/usr/bin/env node
import { readFile } from "node:fs/promises";
import {
  apiRequest,
  assertAllowedArgs,
  optionalInteger,
  optionalString,
  parseArgs,
  printHelp,
  printJson,
  requiredString,
  run,
} from "./lib/common.mjs";

run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, ["session-id", "file", "input", "min-id", "max-id", "limit"]);
  if (args.help) {
    printHelp(`
Usage: node log-filter.mjs --file SCRIPT.lua [options]

Query capture IDs with temporary Lua through POST /api/logs/ids. No changes are saved.
  --session-id ID  Defaults to the active Session
  --input TEXT     Exact string passed as ... (defaults to empty; Lua still executes)
  --min-id ID      Exclusive lower bound
  --max-id ID      Exclusive upper bound
  --limit N        Default/max 10000; 0 validates without scanning

Returns {matched_ids, in_progress_ids}. ScriptContent errors fail the whole query.
`);
    return;
  }
  const content = await readFile(requiredString(args, "file"), "utf8");
  const body = {
    session_id: optionalInteger(args, "session-id", { min: 1 }),
    filter: {
      option: { kind: "script_content", content },
      input: optionalString(args, "input") ?? "",
    },
    min_id: optionalInteger(args, "min-id", { min: 0 }),
    max_id: optionalInteger(args, "max-id", { min: 0 }),
    limit: optionalInteger(args, "limit", { min: 0, max: 10_000 }),
  };
  printJson(await apiRequest(args, "/api/logs/ids", { method: "POST", body }));
});
