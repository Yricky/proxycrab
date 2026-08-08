#!/usr/bin/env node
import { createReadStream } from "node:fs";
import { stat } from "node:fs/promises";
import path from "node:path";
import { Readable } from "node:stream";
import {
  ApiError,
  REQUEST_TIMEOUT_MS,
  UsageError,
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
  assertAllowedArgs(args, ["id", "file", "content-type"]);
  if (args.help) {
    printHelp(`
Usage: node asset-upload.mjs --id ASSET_ID --file FILE [--content-type TYPE]

Stream FILE into a new immutable workspace Asset. The content type defaults to
application/octet-stream. Existing IDs are never overwritten.
`);
    return;
  }

  const id = requiredString(args, "id");
  const file = path.resolve(requiredString(args, "file"));
  const contentType = args["content-type"] ?? "application/octet-stream";
  if (/[^a-z0-9_./]/.test(id)) {
    throw new UsageError("--id may contain only lowercase letters, digits, _, ., and /");
  }
  if (/[^\x20-\x7e]/.test(contentType) || contentType.trim() !== contentType) {
    throw new UsageError("--content-type must be a non-empty printable HTTP header value");
  }
  if (contentType.length === 0) {
    throw new UsageError("--content-type must not be empty");
  }
  const fileStat = await stat(file).catch((error) => {
    throw new UsageError(`cannot read --file ${file}: ${error.message}`);
  });
  if (!fileStat.isFile()) throw new UsageError(`--file is not a regular file: ${file}`);

  const encodedId = id.split("/").map(encodeURIComponent).join("/");
  const url = new URL(`${normalizeBaseUrl(args)}/api/assets/${encodedId}`);
  let response;
  try {
    response = await fetch(url, {
      method: "POST",
      headers: { ...requestHeaders(), "content-type": contentType },
      body: Readable.toWeb(createReadStream(file)),
      duplex: "half",
      signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
    });
  } catch (error) {
    throw new Error(`cannot reach ProxyCrab at ${url.origin}: ${error.message}`);
  }

  let payload;
  try {
    payload = await response.json();
  } catch {
    throw new Error(`ProxyCrab returned non-JSON HTTP ${response.status}`);
  }
  if (!response.ok || payload?.ok !== true) {
    throw new ApiError(
      response.status,
      payload?.error?.code ?? `http_${response.status}`,
      payload?.error?.message ?? `ProxyCrab request failed with HTTP ${response.status}`,
    );
  }
  printJson(payload.data);
});
