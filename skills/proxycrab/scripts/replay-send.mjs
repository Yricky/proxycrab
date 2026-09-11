#!/usr/bin/env node
import {
  apiRequest,
  assertAllowedArgs,
  parseArgs,
  printHelp,
  printJson,
  requiredInteger,
  requiredString,
  run,
} from "./lib/common.mjs";

run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, [
    "session-id",
    "method",
    "url",
    "headers-json",
    "body-text",
    "body-asset-id",
    "body-ref",
  ]);
  if (args.help) {
    printHelp(`
Usage: node replay-send.mjs --session-id ID --method M --url U [options] [--base-url URL]

Send a request through a Session's interceptor pipeline (source: ProxyCrabRequest).
The proxy must be running. Returns as soon as the capture is created.

Options:
  --headers-json JSON   Ordered header pairs, e.g. '[["content-type","text/plain"]]'
  --body-text TEXT      UTF-8 text body
  --body-asset-id ID    Workspace asset body
  --body-ref S:L:SIDE   Reuse capture body, e.g. '3:12:request' or '3:12:response'
`);
    return;
  }
  const sessionId = requiredInteger(args, "session-id", { min: 1 });
  const method = requiredString(args, "method");
  const url = requiredString(args, "url");
  const headers = args["headers-json"] ? JSON.parse(args["headers-json"]) : [];
  const bodyModes = [args["body-text"], args["body-asset-id"], args["body-ref"]].filter(
    (value) => value !== undefined,
  );
  if (bodyModes.length > 1) throw new Error("use only one body option");
  let body;
  if (args["body-text"] !== undefined) {
    body = { type: "text", text: args["body-text"], charset: "utf8" };
  } else if (args["body-asset-id"] !== undefined) {
    body = { type: "asset", asset_id: args["body-asset-id"] };
  } else if (args["body-ref"] !== undefined) {
    const match = /^(\d+):(\d+):(request|response)$/.exec(args["body-ref"]);
    if (!match) throw new Error("--body-ref must be '<session>:<log>:request|response'");
    body = {
      type: "body_ref",
      session_id: Number(match[1]),
      log_id: Number(match[2]),
      side: match[3],
    };
  }
  printJson(
    await apiRequest(args, `/api/replay?session=${sessionId}`, {
      method: "POST",
      body: { method, url, headers, ...(body ? { body } : {}) },
    }),
  );
});
