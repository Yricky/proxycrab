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
Usage: node filter-upsert.mjs --name NAME --file SCRIPT.lua [--base-url URL]

Create a filter script or replace the source of an existing filter with the same name.
`);
    return;
  }
  const name = requiredString(args, "name");
  const content = await readText(requiredString(args, "file"));
  const encodedName = encodeURIComponent(name);
  let action;
  try {
    await apiRequest(args, `/api/filter-scripts/${encodedName}`);
    await apiRequest(args, `/api/filter-scripts/${encodedName}`, {
      method: "PUT",
      body: { content },
    });
    action = "updated";
  } catch (error) {
    if (!(error instanceof ApiError) || error.code !== "not_found") throw error;
    await apiRequest(args, "/api/filter-scripts", {
      method: "POST",
      body: { name, content },
    });
    action = "created";
  }
  printJson({ action, name });
});
