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
  const host = config.api_host.includes(":") ? `[${config.api_host}]` : config.api_host;
  return `http://${host}:${config.api_port}`;
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
): Promise<LoadedBody> {
  if (!Number.isSafeInteger(maxSize) || maxSize <= 0 || maxSize > UI_BODY_MAX_SIZE) {
    throw new BodyFetchError({ code: "bad_request", message: "无效的 Body 大小限制" });
  }
  const path =
    target.kind === "log"
      ? `/api/logs/${target.id}/body`
      : `/api/breakpoints/${target.id}/body`;
  const url = new URL(path, base);
  url.searchParams.set("side", side);
  url.searchParams.set("decompress", "false");
  url.searchParams.set("max_size", String(maxSize));
  if (target.kind === "log") url.searchParams.set("session_id", String(target.sessionId));

  const response = await fetch(url, {
    headers: authorization ? { Authorization: authorization } : undefined,
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
