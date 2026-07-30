#!/usr/bin/env node
import {
  UsageError,
  apiRequest,
  assertAllowedArgs,
  parseArgs,
  printHelp,
  printJson,
  readJson,
  requiredInteger,
  requiredString,
  run,
  sessionQuery,
} from "./lib/common.mjs";

function validateChain(value, key) {
  if (!Array.isArray(value)) throw new UsageError(`${key} must be an array`);
  if (value.length > 12) throw new UsageError(`${key} may contain at most 12 interceptors`);
  const names = new Set();
  for (const [index, item] of value.entries()) {
    if (
      typeof item !== "object" ||
      item === null ||
      typeof item.name !== "string" ||
      item.name.length === 0 ||
      typeof item.enabled !== "boolean"
    ) {
      throw new UsageError(`${key}[${index}] must contain string name and boolean enabled`);
    }
    if (names.has(item.name)) throw new UsageError(`${key} contains duplicate name: ${item.name}`);
    names.add(item.name);
  }
}

run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, ["session-id", "file"]);
  if (args.help) {
    printHelp(`
Usage: node session-interceptors-set.mjs --session-id ID --file CHAINS.json [--base-url URL]

Atomically replace both interceptor chains. File shape:
{
  "request": [{ "name": "request-script", "enabled": true }],
  "response": [{ "name": "response-script", "enabled": true }]
}
`);
    return;
  }
  const sessionId = requiredInteger(args, "session-id", { min: 1 });
  const body = await readJson(requiredString(args, "file"));
  if (typeof body !== "object" || body === null) {
    throw new UsageError("interceptor file must contain a JSON object");
  }
  validateChain(body.request, "request");
  validateChain(body.response, "response");
  printJson(
    await apiRequest(args, `/api/session-interceptors${sessionQuery(sessionId)}`, {
      method: "PUT",
      body: { request: body.request, response: body.response },
    }),
  );
});
