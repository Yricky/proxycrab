#!/usr/bin/env node
import {
  apiRequest,
  assertAllowedArgs,
  parseArgs,
  printHelp,
  printJson,
  run,
} from "./lib/common.mjs";

run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, []);
  if (args.help) {
    printHelp(`
Usage: node session-list.mjs [--base-url URL]

List all ProxyCrab Sessions.
`);
    return;
  }
  printJson(await apiRequest(args, "/api/sessions"));
});
