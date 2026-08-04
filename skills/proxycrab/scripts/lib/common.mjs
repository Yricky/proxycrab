import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";

export const DEFAULT_BASE_URL = "http://127.0.0.1:18089";

export class UsageError extends Error {}

export class ApiError extends Error {
  constructor(status, code, message, details = {}) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.code = code;
    Object.assign(this, details);
  }
}

export function parseArgs(argv = process.argv.slice(2)) {
  const args = {};
  for (let index = 0; index < argv.length; index += 1) {
    const token = argv[index];
    if (!token.startsWith("--")) {
      throw new UsageError(`unexpected positional argument: ${token}`);
    }
    const key = token.slice(2);
    if (!key) throw new UsageError("empty option name");
    if (key === "help" || key === "case-sensitive" || key === "include-in-progress") {
      args[key] = true;
      continue;
    }
    const value = argv[index + 1];
    if (value === undefined || value.startsWith("--")) {
      throw new UsageError(`missing value for --${key}`);
    }
    if (Object.hasOwn(args, key)) {
      throw new UsageError(`duplicate option: --${key}`);
    }
    args[key] = value;
    index += 1;
  }
  return args;
}

export function assertAllowedArgs(args, allowed) {
  const allowedSet = new Set(["help", "base-url", ...allowed]);
  for (const key of Object.keys(args)) {
    if (!allowedSet.has(key)) throw new UsageError(`unknown option: --${key}`);
  }
}

export function requiredString(args, key) {
  const value = optionalString(args, key);
  if (value === undefined) throw new UsageError(`--${key} is required`);
  return value;
}

export function optionalString(args, key) {
  const value = args[key];
  if (value === undefined) return undefined;
  if (typeof value !== "string" || value.length === 0) {
    throw new UsageError(`--${key} must not be empty`);
  }
  return value;
}

export function optionalInteger(args, key, { min = 0, max = Number.MAX_SAFE_INTEGER } = {}) {
  const value = args[key];
  if (value === undefined) return undefined;
  if (typeof value !== "string" || !/^\d+$/.test(value)) {
    throw new UsageError(`--${key} must be an integer`);
  }
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed < min || parsed > max) {
    throw new UsageError(`--${key} must be between ${min} and ${max}`);
  }
  return parsed;
}

export function requiredInteger(args, key, range) {
  const value = optionalInteger(args, key, range);
  if (value === undefined) throw new UsageError(`--${key} is required`);
  return value;
}

export function normalizeBaseUrl(args) {
  const raw = args["base-url"] ?? process.env.PROXYCRAB_API_URL ?? DEFAULT_BASE_URL;
  let url;
  try {
    url = new URL(raw);
  } catch {
    throw new UsageError(`invalid ProxyCrab base URL: ${raw}`);
  }
  if (!["http:", "https:"].includes(url.protocol)) {
    throw new UsageError("ProxyCrab base URL must use http or https");
  }
  const hostname = url.hostname.replace(/^\[|\]$/g, "").replace(/\.$/, "").toLowerCase();
  if (hostname !== "localhost" && hostname !== "127.0.0.1" && hostname !== "::1") {
    throw new UsageError("ProxyCrab base URL must use a loopback host");
  }
  url.pathname = url.pathname.replace(/\/+$/, "");
  url.search = "";
  url.hash = "";
  return url.toString().replace(/\/$/, "");
}

export async function apiRequest(args, pathname, { method = "GET", body } = {}) {
  const url = new URL(`${normalizeBaseUrl(args)}${pathname}`);
  let response;
  try {
    response = await fetch(url, {
      method,
      headers: body === undefined ? undefined : { "content-type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
      signal: AbortSignal.timeout(10_000),
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
    const code = payload?.error?.code ?? `http_${response.status}`;
    const message = payload?.error?.message ?? `ProxyCrab request failed with HTTP ${response.status}`;
    throw new ApiError(response.status, code, message);
  }
  return payload.data;
}

export async function apiTextRequest(args, pathname) {
  const url = new URL(`${normalizeBaseUrl(args)}${pathname}`);
  let response;
  try {
    response = await fetch(url, { signal: AbortSignal.timeout(10_000) });
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
    const code = payload?.error?.code ?? `http_${response.status}`;
    const message = payload?.error?.message ?? `ProxyCrab request failed with HTTP ${response.status}`;
    throw new ApiError(response.status, code, message);
  }
  return text;
}

export async function apiDownloadRequest(args, pathname, { method = "GET", body } = {}) {
  const url = new URL(`${normalizeBaseUrl(args)}${pathname}`);
  let response;
  try {
    response = await fetch(url, {
      method,
      headers: body === undefined ? undefined : { "content-type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
  } catch (error) {
    throw new Error(`cannot reach ProxyCrab at ${url.origin}: ${error.message}`);
  }
  if (!response.ok) {
    const text = await response.text();
    let payload;
    try {
      payload = JSON.parse(text);
    } catch {
      payload = undefined;
    }
    const code = payload?.error?.code ?? `http_${response.status}`;
    const message = payload?.error?.message ?? `ProxyCrab request failed with HTTP ${response.status}`;
    throw new ApiError(response.status, code, message);
  }
  const disposition = response.headers.get("content-disposition") ?? "";
  const filename = /filename="([^"]+)"/i.exec(disposition)?.[1];
  return {
    bytes: new Uint8Array(await response.arrayBuffer()),
    filename: filename === undefined ? undefined : path.basename(filename),
  };
}

export function sessionQuery(sessionId) {
  return sessionId === undefined ? "" : `?session_id=${encodeURIComponent(sessionId)}`;
}

export async function readText(pathname) {
  return readFile(pathname, "utf8");
}

export async function readJson(pathname) {
  const content = await readText(pathname);
  try {
    return JSON.parse(content);
  } catch (error) {
    throw new UsageError(`invalid JSON file ${pathname}: ${error.message}`);
  }
}

export async function writeJson(pathname, value) {
  const resolved = path.resolve(pathname);
  await writeFile(resolved, `${JSON.stringify(value, null, 2)}\n`, "utf8");
  return resolved;
}

export async function writeBytes(pathname, value) {
  const resolved = path.resolve(pathname);
  await writeFile(resolved, value);
  return resolved;
}

export function printJson(value) {
  process.stdout.write(`${JSON.stringify(value, null, 2)}\n`);
}

export function printHelp(text) {
  process.stdout.write(`${text.trim()}\n`);
}

export function run(main) {
  Promise.resolve()
    .then(main)
    .catch((error) => {
      const payload = {
        ok: false,
        error: {
          type: error.name || "Error",
          ...(error.code ? { code: error.code } : {}),
          ...(error.status ? { status: error.status } : {}),
          ...(error.actual_size !== undefined ? { actual_size: error.actual_size } : {}),
          ...(error.max_size !== undefined ? { max_size: error.max_size } : {}),
          message: error.message,
        },
      };
      process.stderr.write(`${JSON.stringify(payload, null, 2)}\n`);
      process.exitCode = error instanceof UsageError ? 2 : 1;
    });
}

export function buildRemoteFilter(args, { allowMultiple = false } = {}) {
  const selectors = [
    ["filter-script", "script", null],
    ["column-script", "column", null],
    ["uri", "column", { kind: "uri" }],
    ["method", "column", { kind: "method" }],
    ["status", "column", { kind: "code" }],
    ["source", "column", { kind: "source" }],
    ["stage", "column", { kind: "stage" }],
  ].filter(([key]) => args[key] !== undefined);

  if (!allowMultiple && selectors.length > 1) {
    throw new UsageError(
      `only one filter selector is allowed: ${selectors.map(([key]) => `--${key}`).join(", ")}`,
    );
  }
  if (selectors.length === 0) {
    return args.input === undefined ? undefined : { option: null, input: args.input };
  }

  const [key, kind, column] = selectors[0];
  const value = args[key];
  if (kind === "script") {
    return {
      option: { kind: "script", script_name: value },
      input: args.input ?? "",
    };
  }
  const selectedColumn =
    key === "column-script" ? { kind: "script", script_name: value } : column;
  return {
    option: {
      kind: "column",
      column: selectedColumn,
      case_sensitive: args["case-sensitive"] === true,
    },
    input: key === "column-script" ? (args.input ?? "") : value,
  };
}

export function matchesBuiltInCriteria(detail, args) {
  const caseSensitive = args["case-sensitive"] === true;
  const contains = (actual, expected) => {
    if (expected === undefined) return true;
    const left = String(actual ?? "");
    return caseSensitive
      ? left.includes(expected)
      : left.toLowerCase().includes(expected.toLowerCase());
  };
  return (
    contains(detail.request?.uri, args.uri) &&
    contains(detail.request?.method, args.method) &&
    contains(detail.response?.status, args.status) &&
    contains(detail.source_addr, args.source) &&
    contains(detail.stage, args.stage)
  );
}
