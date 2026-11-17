#!/usr/bin/env node
import { readFile } from "node:fs/promises";
import {
  ApiError,
  UsageError,
  REQUEST_TIMEOUT_MS,
  assertAllowedArgs,
  normalizeBaseUrl,
  parseArgs,
  printHelp,
  printJson,
  requestHeaders,
  requiredString,
  run,
} from "./lib/common.mjs";

run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, ["file", "replace"]);
  if (args.help) {
    printHelp(
      "Usage: node session-page-import.mjs --file PAGE.zip [--replace true]\nImport one page package. --replace true requires the user's acceptance of replacing that package ID.",
    );
    return;
  }
  if (args.replace !== undefined && !["true", "false"].includes(args.replace))
    throw new UsageError("--replace must be true or false");
  const bytes = await readFile(requiredString(args, "file"));
  if (bytes.length > 128 * 1024 * 1024) throw new UsageError("page package exceeds 128 MiB");
  const url = new URL(normalizeBaseUrl(args) + "/api/session-pages");
  if (args.replace === "true") url.searchParams.set("replace", "true");
  const response = await fetch(url, {
    method: "POST",
    headers: { ...requestHeaders(), "content-type": "application/zip" },
    body: bytes,
    signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
  });
  const payload = await response.json();
  if (!response.ok || payload?.ok !== true)
    throw new ApiError(
      response.status,
      payload?.error?.code ?? `http_${response.status}`,
      payload?.error?.message ?? "Page import failed",
    );
  printJson(payload.data);
});
