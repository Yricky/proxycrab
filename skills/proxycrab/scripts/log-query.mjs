#!/usr/bin/env node
import {
  apiRequest,
  assertAllowedArgs,
  buildRemoteFilter,
  optionalInteger,
  parseArgs,
  printHelp,
  printJson,
  run,
} from "./lib/common.mjs";

run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, [
    "session-id",
    "limit",
    "min-id",
    "max-id",
    "uri",
    "method",
    "status",
    "source",
    "stage",
    "column-script",
    "filter-script",
    "input",
    "case-sensitive",
  ]);
  if (args.help) {
    printHelp(`
Usage: node log-query.mjs [options]

Query capture IDs. Omitting --session-id uses the active Session.

Bounds:
  --session-id ID
  --limit N              Default/max 10000
  --min-id ID            Exclusive; scans toward newer IDs
  --max-id ID            Exclusive; scans toward older IDs

Choose at most one filter selector:
  --uri TEXT
  --method TEXT
  --status TEXT
  --source TEXT
  --stage TEXT
  --column-script NAME [--input TEXT]
  --filter-script NAME [--input TEXT]
  --case-sensitive

With no selector or --input, the Session's persisted filter is reused. Supplying a selector applies
it only to this query and does not change the Session's persisted filter.
`);
    return;
  }

  const filter = buildRemoteFilter(args);
  const body = {
    session_id: optionalInteger(args, "session-id", { min: 1 }),
    filter,
    persist_filter: filter === undefined ? undefined : false,
    min_id: optionalInteger(args, "min-id", { min: 0 }),
    max_id: optionalInteger(args, "max-id", { min: 1 }),
    limit: optionalInteger(args, "limit", { min: 0, max: 10_000 }),
  };
  for (const key of Object.keys(body)) {
    if (body[key] === undefined) delete body[key];
  }
  printJson(await apiRequest(args, "/api/logs/ids", { method: "POST", body }));
});
