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
Usage: node redaction-upsert.mjs --name NAME --file SCRIPT.lua [--base-url URL]

NAME accepts a relative path, e.g. business/example (without the automatic .lua extension).

Create a read-time redaction script or replace its source. Does not bind it or alter captured traffic.
Unavailable when the server uses --redaction-script; disabled management errors are not retried.
`);
    return;
  }
  const name = requiredString(args, "name");
  const content = await readText(requiredString(args, "file"));
  // The name-scoped read is denied by default, so probe with a create and treat a
  // conflict as an existing script rather than reading it first.
  let action;
  try {
    await apiRequest(args, "/api/redaction-scripts", {
      method: "POST",
      body: { name, content },
    });
    action = "created";
  } catch (error) {
    if (!(error instanceof ApiError) || error.code !== "conflict") throw error;
    await apiRequest(args, `/api/redaction-scripts/${encodeURIComponent(name)}`, {
      method: "PUT",
      body: { content },
    });
    action = "updated";
  }
  printJson({ action, name });
});
