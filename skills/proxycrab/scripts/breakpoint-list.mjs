#!/usr/bin/env node
import {
  apiRequest,
  assertAllowedArgs,
  optionalInteger,
  optionalString,
  parseArgs,
  printHelp,
  printJson,
  run,
  UsageError,
} from "./lib/common.mjs";

run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, ["session-id", "phase", "interceptor-name"]);
  if (args.help) {
    printHelp(`
Usage: node breakpoint-list.mjs [--session-id ID] [--phase request|response]
                                [--interceptor-name NAME] [--base-url URL]

List active, process-local interceptor breakpoints. Omitting --session-id uses the active Session.
`);
    return;
  }
  const sessionId = optionalInteger(args, "session-id", { min: 1 });
  const phase = optionalString(args, "phase");
  if (phase !== undefined && phase !== "request" && phase !== "response") {
    throw new UsageError("--phase must be request or response");
  }
  const interceptorName = optionalString(args, "interceptor-name");
  const query = new URLSearchParams();
  if (sessionId !== undefined) query.set("session_id", String(sessionId));
  if (phase !== undefined) query.set("phase", phase);
  if (interceptorName !== undefined) query.set("interceptor_name", interceptorName);
  printJson(await apiRequest(args, `/api/breakpoints${query.size ? `?${query}` : ""}`));
});
