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
  assertAllowedArgs(args, ["name", "description"]);
  if (args.help) {
    printHelp(`
Usage: node session-create.mjs [--name NAME] [--description TEXT] [--base-url URL]

Create a ProxyCrab Session. ProxyCrab generates a name when --name is omitted.
`);
    return;
  }
  const body = {
    name: optionalString(args, "name") ?? null,
    description: optionalString(args, "description") ?? null,
  };
  printJson(await apiRequest(args, "/api/sessions", { method: "POST", body }));
});
