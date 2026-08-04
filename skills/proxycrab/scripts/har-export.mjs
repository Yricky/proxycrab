#!/usr/bin/env node
import {
  apiDownloadRequest,
  assertAllowedArgs,
  optionalInteger,
  optionalString,
  parseArgs,
  printHelp,
  printJson,
  run,
  UsageError,
  writeBytes,
} from "./lib/common.mjs";

function optionalLogIds(args) {
  const value = optionalString(args, "log-ids");
  if (value === undefined) return undefined;
  return value.split(",").map((part) => {
    if (!/^\d+$/.test(part)) {
      throw new UsageError("--log-ids must be a comma-separated list of positive integers");
    }
    const id = Number(part);
    if (!Number.isSafeInteger(id) || id < 1) {
      throw new UsageError("--log-ids must contain positive safe integers");
    }
    return id;
  });
}

run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, ["session-id", "log-ids", "output"]);
  if (args.help) {
    printHelp(`
Usage: node har-export.mjs [--session-id ID] [--log-ids 1,2,3] [--output FILE] [--base-url URL]

Export successful HTTP captures as HAR 1.2. Omit --log-ids to export every eligible capture in
the Session. Omit --session-id to use the active Session. The default output filename comes from
ProxyCrab. HAR files include captured credentials, cookies, headers, and bodies without redaction.
`);
    return;
  }
  const sessionId = optionalInteger(args, "session-id", { min: 1 });
  const logIds = optionalLogIds(args);
  const requestedOutput = optionalString(args, "output");
  const request = {
    format: "har",
    ...(sessionId === undefined ? {} : { session_id: sessionId }),
    ...(logIds === undefined ? {} : { log_ids: logIds }),
  };
  const download = await apiDownloadRequest(args, "/api/logs/export", {
    method: "POST",
    body: request,
  });
  const output = requestedOutput ?? download.filename ?? "proxycrab-export.har";
  const outputPath = await writeBytes(output, download.bytes);
  printJson({
    path: outputPath,
    format: "har",
    ...(sessionId === undefined ? {} : { session_id: sessionId }),
    ...(logIds === undefined ? {} : { log_ids: logIds }),
  });
});
