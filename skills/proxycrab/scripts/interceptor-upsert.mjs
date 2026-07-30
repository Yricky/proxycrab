#!/usr/bin/env node
import {
  ApiError,
  UsageError,
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
  assertAllowedArgs(args, ["kind", "name", "file"]);
  if (args.help) {
    printHelp(`
Usage: node interceptor-upsert.mjs --kind request|response --name NAME --file SCRIPT.lua

Create an interceptor or replace the source of an existing interceptor with the same kind/name.
`);
    return;
  }
  const kind = requiredString(args, "kind");
  if (!["request", "response"].includes(kind)) {
    throw new UsageError("--kind must be request or response");
  }
  const name = requiredString(args, "name");
  const content = await readText(requiredString(args, "file"));
  const endpoint = `/api/interceptors/${kind}/${encodeURIComponent(name)}`;
  let action;
  try {
    await apiRequest(args, endpoint);
    await apiRequest(args, endpoint, { method: "PUT", body: { content } });
    action = "updated";
  } catch (error) {
    if (!(error instanceof ApiError) || error.code !== "not_found") throw error;
    await apiRequest(args, "/api/interceptors", {
      method: "POST",
      body: { kind, name, content },
    });
    action = "created";
  }
  printJson({ action, kind, name });
});
