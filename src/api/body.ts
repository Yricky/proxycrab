import type { AppConfig, ManagerError } from "./types";

export const DEFAULT_BODY_MAX_SIZE = 16 * 1024 * 1024;
export const UI_BODY_MAX_SIZE = 1024 * 1024 * 1024;

export type BodySide = "request" | "response";

export type BodyTarget =
  | { kind: "log"; id: number; sessionId: number }
  | { kind: "breakpoint"; id: number };

export interface LoadedBody {
  bytes: Uint8Array;
  decodedSize: number;
  contentType: string;
}

export class BodyFetchError extends Error {
  readonly code: string;
  readonly actualSize?: number;
  readonly maxSize?: number;

  constructor(error: ManagerError) {
    super(error.message);
    this.name = "BodyFetchError";
    this.code = error.code;
    this.actualSize = error.actual_size;
    this.maxSize = error.max_size;
  }
}

function apiBase(config: AppConfig): string {
  return `http://127.0.0.1:${config.api_port}`;
}

export async function fetchBodyFromHttp(
  config: AppConfig,
  target: BodyTarget,
  side: BodySide,
  maxSize = DEFAULT_BODY_MAX_SIZE,
  authorization?: string,
): Promise<LoadedBody> {
  return fetchBodyFromBase(apiBase(config), target, side, maxSize, authorization);
}

export async function fetchBodyFromBase(
  base: string,
  target: BodyTarget,
  side: BodySide,
  maxSize = DEFAULT_BODY_MAX_SIZE,
  authorization?: string,
  apiPrefix = "/api",
  queryToken?: string,
): Promise<LoadedBody> {
  if (!Number.isSafeInteger(maxSize) || maxSize <= 0 || maxSize > UI_BODY_MAX_SIZE) {
    throw new BodyFetchError({ code: "bad_request", message: "无效的 Body 大小限制" });
  }
  const path =
    target.kind === "log"
      ? `${apiPrefix}/logs/${target.id}/body`
      : `${apiPrefix}/breakpoints/${target.id}/body`;
  const url = new URL(path, base);
  url.searchParams.set("side", side);
  url.searchParams.set("max_size", String(maxSize));
  if (target.kind === "log") url.searchParams.set("session_id", String(target.sessionId));
  if (queryToken !== undefined) url.searchParams.append("token", queryToken);

  const response = await fetch(url, {
    headers: authorization ? { Authorization: authorization } : undefined,
    referrerPolicy: queryToken === undefined ? undefined : "no-referrer",
  });
  if (!response.ok) {
    let error: ManagerError = {
      code: `http_${response.status}`,
      message: `Body 请求失败（HTTP ${response.status}）`,
    };
    try {
      const payload = (await response.json()) as { error?: ManagerError };
      if (payload.error) error = payload.error;
    } catch {
      // Keep the status-derived fallback when the server did not return JSON.
    }
    throw new BodyFetchError(error);
  }
  const buffer = await response.arrayBuffer();
  return {
    bytes: new Uint8Array(buffer),
    decodedSize: buffer.byteLength,
    contentType: response.headers.get("content-type") ?? "application/octet-stream",
  };
}
