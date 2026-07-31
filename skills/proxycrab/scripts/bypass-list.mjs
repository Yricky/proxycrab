#!/usr/bin/env node
import {
  apiRequest,
  assertAllowedArgs,
  optionalInteger,
  parseArgs,
  printHelp,
  printJson,
  run,
} from "./lib/common.mjs";

run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, ["before-id", "limit"]);
  if (args.help) {
    printHelp(`
Usage: node bypass-list.mjs [--before-id ID] [--limit 200] [--base-url URL]

List persisted transparent-forwarding entries, newest first.
`);
    return;
  }
  const beforeId = optionalInteger(args, "before-id", { min: 1 });
  const limit = optionalInteger(args, "limit", { min: 1, max: 1000 });
  const query = new URLSearchParams();
  if (beforeId !== undefined) query.set("before_id", String(beforeId));
  if (limit !== undefined) query.set("limit", String(limit));
  const suffix = query.size ? `?${query}` : "";
  printJson(await apiRequest(args, `/api/bypass${suffix}`));
});
