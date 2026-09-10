#!/usr/bin/env node
import {
  ApiError,
  assertAllowedArgs,
  normalizeBaseUrl,
  optionalInteger,
  parseArgs,
  printHelp,
  printJson,
  requestHeaders,
  REQUEST_TIMEOUT_MS,
  requiredInteger,
  run,
  sessionQuery,
  UsageError,
} from "./lib/common.mjs";

run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, ["session-id", "log-id", "execution-id", "what"]);
  if (args.help) {
    printHelp(`
Usage: node interceptor-get.mjs --log-id ID --execution-id ID [--session-id ID] [--what content|snapshot]

Read one interceptor execution's exact executed source (--what content, the default) or its entry
snapshot (--what snapshot). Omitting --session-id uses the active Session.
`);
    return;
  }
  const sessionId = optionalInteger(args, "session-id", { min: 1 });
  const logId = requiredInteger(args, "log-id", { min: 1 });
  const executionId = requiredInteger(args, "execution-id", { min: 1 });
  const what = args.what ?? "content";
  if (what !== "content" && what !== "snapshot") {
    throw new UsageError("--what must be content or snapshot");
  }
  const url = new URL(
    `${normalizeBaseUrl(args)}/api/logs/${logId}/interceptors/${executionId}/${what}${sessionQuery(sessionId)}`,
  );
  let response;
  try {
    response = await fetch(url, {
      headers: requestHeaders(),
      signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
    });
  } catch (error) {
    throw new Error(`cannot reach ProxyCrab at ${url.origin}: ${error.message}`);
  }
  const text = await response.text();
  if (!response.ok) {
    let payload;
    try {
      payload = JSON.parse(text);
    } catch {
      payload = undefined;
    }
    const error = payload?.error;
    throw new ApiError(
      response.status,
      error?.code ?? `http_${response.status}`,
      error?.message ?? `ProxyCrab request failed with HTTP ${response.status}`,
    );
  }
  if (what === "content") {
    printJson({
      script_hash: response.headers.get("x-proxycrab-script-sha256") ?? "",
      content: text,
    });
    return;
  }
  printJson(JSON.parse(text).data);
});
