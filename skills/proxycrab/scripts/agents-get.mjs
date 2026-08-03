#!/usr/bin/env node
import {
  apiTextRequest,
  assertAllowedArgs,
  parseArgs,
  printHelp,
  run,
} from "./lib/common.mjs";

run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, []);
  if (args.help) {
    printHelp(`
Usage: node agents-get.mjs [--base-url URL]

Print the active workspace AGENTS.md instructions as plain text. Run this immediately after
reading the ProxyCrab Skill and before calling any other ProxyCrab API.
`);
    return;
  }

  process.stdout.write(await apiTextRequest(args, "/api/agents.md"));
});
