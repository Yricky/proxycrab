#!/usr/bin/env node
import { createWriteStream } from "node:fs";
import path from "node:path";
import { pipeline } from "node:stream/promises";
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
  requiredString,
  run,
  UsageError,
} from "./lib/common.mjs";

run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, ["session-id", "log-id", "breakpoint-id", "side", "decompress", "max-size", "output"]);
  if (args.help) {
    printHelp(`
Usage:
  node body-get.mjs --log-id ID [--session-id ID] --side request|response --output FILE [--max-size BYTES]
  node body-get.mjs --breakpoint-id ID --side request|response --output FILE [--decompress true]

Read the complete decoded body into FILE. The default maximum is 16777216 stored bytes. Exactly one
of --log-id and --breakpoint-id is required. Server-side decompression defaults to false; Node
normally decodes the preserved Content-Encoding. max-size is only valid in raw server mode.
`);
    return;
  }

  const logId = optionalInteger(args, "log-id", { min: 1 });
  const breakpointId = optionalInteger(args, "breakpoint-id", { min: 1 });
  if ((logId === undefined) === (breakpointId === undefined)) {
    throw new UsageError("exactly one of --log-id and --breakpoint-id is required");
  }
  const sessionId = optionalInteger(args, "session-id", { min: 1 });
  if (breakpointId !== undefined && sessionId !== undefined) {
    throw new UsageError("--session-id is only valid with --log-id");
  }
  const side = requiredString(args, "side");
  if (side !== "request" && side !== "response") {
    throw new UsageError("--side must be request or response");
  }
  const maxSize = optionalInteger(args, "max-size", { min: 1 });
  const decompressValue = args.decompress ?? "false";
  if (decompressValue !== "true" && decompressValue !== "false") {
    throw new UsageError("--decompress must be true or false");
  }
  const decompress = decompressValue === "true";
  if (decompress && maxSize !== undefined) {
    throw new UsageError("--max-size is not supported with --decompress true");
  }
  const output = path.resolve(requiredString(args, "output"));
  const pathname =
    logId !== undefined ? `/api/logs/${logId}/body` : `/api/breakpoints/${breakpointId}/body`;
  const url = new URL(`${normalizeBaseUrl(args)}${pathname}`);
  url.searchParams.set("side", side);
  url.searchParams.set("decompress", String(decompress));
  if (sessionId !== undefined) url.searchParams.set("session_id", String(sessionId));
  if (maxSize !== undefined) url.searchParams.set("max_size", String(maxSize));

  let response;
  try {
    response = await fetch(url, {
      headers: requestHeaders(),
      signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
    });
  } catch (error) {
    throw new Error(`cannot reach ProxyCrab at ${url.origin}: ${error.message}`);
  }
  if (!response.ok) {
    let payload;
    try {
      payload = await response.json();
    } catch {
      payload = undefined;
    }
    const error = payload?.error;
    throw new ApiError(
      response.status,
      error?.code ?? `http_${response.status}`,
      error?.message ?? `ProxyCrab request failed with HTTP ${response.status}`,
      {
        ...(error?.actual_size !== undefined ? { actual_size: error.actual_size } : {}),
        ...(error?.max_size !== undefined ? { max_size: error.max_size } : {}),
      },
    );
  }
  if (!response.body) throw new Error("ProxyCrab returned an empty response stream");
  await pipeline(response.body, createWriteStream(output));
  printJson({
    output,
    stored_size: Number(response.headers.get("x-proxycrab-body-size") ?? 0),
    content_type: response.headers.get("content-type") ?? "application/octet-stream",
  });
});
