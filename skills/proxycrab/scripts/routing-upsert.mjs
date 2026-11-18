#!/usr/bin/env node
import {
  ApiError,
  apiRequest,
  assertAllowedArgs,
  parseArgs,
  printHelp,
  printJson,
  readText,
  requiredString,
  run,
} from "./lib/common.mjs";

run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, ["name", "file"]);
  if (args.help) {
    printHelp(`
Usage: node routing-upsert.mjs --name NAME --file SCRIPT.lua [--base-url URL]

NAME accepts a relative path, e.g. business/example (without the automatic .lua extension).

Create a routing script or replace the source of an existing script with the same name.
This does not select the script; use routing-select.mjs separately.
Lua get_sessions() lists non-archived Sessions (id, name, description, active).
Return a positive integer Session ID to capture there, true for the active Session,
or false/nil to bypass. HTTPS selects its Session once at CONNECT.
`);
    return;
  }
  const name = requiredString(args, "name");
  const content = await readText(requiredString(args, "file"));
  const encodedName = encodeURIComponent(name);
  let action;
  try {
    await apiRequest(args, `/api/routing-scripts/${encodedName}`);
    await apiRequest(args, `/api/routing-scripts/${encodedName}`, {
      method: "PUT",
      body: { content },
    });
    action = "updated";
  } catch (error) {
    if (!(error instanceof ApiError) || error.code !== "not_found") throw error;
    await apiRequest(args, "/api/routing-scripts", {
      method: "POST",
      body: { name, content },
    });
    action = "created";
  }
  printJson({ action, name });
});
