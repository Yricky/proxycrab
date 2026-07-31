#!/usr/bin/env node
import {
  apiRequest,
  assertAllowedArgs,
  optionalString,
  parseArgs,
  printHelp,
  printJson,
  run,
} from "./lib/common.mjs";

run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, ["name", "clear"]);
  if (args.help) {
    printHelp(`
Usage:
  node routing-select.mjs --name NAME [--base-url URL]
  node routing-select.mjs --clear true [--base-url URL]

Select one routing script, or clear the selection to restore default-tag routing.
`);
    return;
  }
  const name = optionalString(args, "name");
  const clear = optionalString(args, "clear");
  if ((name === undefined) === (clear === undefined)) {
    throw new Error("provide exactly one of --name or --clear true");
  }
  if (clear !== undefined && clear !== "true") {
    throw new Error("--clear only accepts true");
  }
  printJson(
    await apiRequest(args, "/api/routing-script-selection", {
      method: "PUT",
      body: { name: name ?? null },
    }),
  );
});
