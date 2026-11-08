import type { ErrorCode } from "./generated/ErrorCode";
import type { ManagerError } from "./generated/ManagerError";
import type { ApiError, TransportErrorCode } from "./types";
import type { BodySide } from "./generated/BodySide";
import { t } from "../i18n";
import { saveBlobInWebview } from "./save-file";

export const DEFAULT_BODY_MAX_SIZE = 16 * 1024 * 1024;
export const UI_BODY_MAX_SIZE = 1024 * 1024 * 1024;

export type BodyTarget =
  | { kind: "log"; id: number; sessionId: number }
  | { kind: "interceptor"; id: number; sessionId: number; executionId: number }
  | { kind: "breakpoint"; id: number };

export interface LoadedBody {
  bytes: Uint8Array;
  decodedSize: number;
  contentType: string;
}

export class BodyFetchError extends Error {
  readonly code: ErrorCode | TransportErrorCode;
  readonly actualSize?: number;
  readonly maxSize?: number;

  constructor(error: ApiError) {
    super(error.message);
    this.name = "BodyFetchError";
    this.code = error.code;
    this.actualSize = error.actual_size;
    this.maxSize = error.max_size;
  }
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
    throw new BodyFetchError({
      code: "bad_request",
      message: t("无效的 Body 大小限制"),
    });
  }
  const response = await requestBody(
    base,
    target,
    side,
    maxSize,
    authorization,
    apiPrefix,
    queryToken,
  );
  const buffer = await response.arrayBuffer();
  return {
    bytes: new Uint8Array(buffer),
    decodedSize: buffer.byteLength,
    contentType: response.headers.get("content-type") ?? "application/octet-stream",
  };
}

/** Downloads use browser-managed Blob storage, without the preview's JS buffer or size cap. */
export async function downloadBodyFromBase(
  base: string,
  target: BodyTarget,
  side: BodySide,
  filename: string,
  authorization?: string,
  apiPrefix = "/api",
  queryToken?: string,
): Promise<boolean> {
  const response = await requestBody(
    base,
    target,
    side,
    Number.MAX_SAFE_INTEGER,
    authorization,
    apiPrefix,
    queryToken,
  );
  return saveBlobInWebview(await response.blob(), filename);
}

async function requestBody(
  base: string,
  target: BodyTarget,
  side: BodySide,
  maxSize: number,
  authorization: string | undefined,
  apiPrefix: string,
  queryToken: string | undefined,
): Promise<Response> {
  const path =
    target.kind === "breakpoint"
      ? `${apiPrefix}/breakpoints/${target.id}/body`
      : `${apiPrefix}/logs/${target.id}/body`;
  const url = new URL(path, base);
  url.searchParams.set("side", side);
  url.searchParams.set("max_size", String(maxSize));
  if (target.kind !== "breakpoint") {
    url.searchParams.set("session_id", String(target.sessionId));
  }
  if (target.kind === "interceptor") {
    url.searchParams.set("execution_id", String(target.executionId));
  }
  if (queryToken !== undefined) url.searchParams.append("token", queryToken);

  const response = await fetch(url, {
    headers: authorization ? { Authorization: authorization } : undefined,
    referrerPolicy: queryToken === undefined ? undefined : "no-referrer",
  });
  if (!response.ok) {
    let error: ApiError = {
      code: `http_${response.status}`,
      message: t("Body 请求失败（HTTP {status}）", { status: response.status }),
    };
    try {
      const payload = (await response.json()) as { error?: ManagerError };
      if (payload.error) error = payload.error;
    } catch {
      // Keep the status-derived fallback when the server did not return JSON.
    }
    throw new BodyFetchError(error);
  }
  return response;
}

/** Reads one policy-filtered WebSocket message, never the internal container. */
export async function fetchWebSocketBodyFromBase(
  base: string,
  sessionId: number,
  id: number,
  side: BodySide,
  messageId: number,
  authorization?: string,
  apiPrefix = "/api",
  queryToken?: string,
): Promise<LoadedBody> {
  const url = new URL(`${apiPrefix}/logs/${id}/websocket/messages/${messageId}/body`, base);
  url.searchParams.set("session_id", String(sessionId));
  url.searchParams.set("side", side);
  url.searchParams.set("max_size", String(DEFAULT_BODY_MAX_SIZE));
  if (queryToken !== undefined) url.searchParams.set("token", queryToken);
  const response = await fetch(url, {
    headers: authorization ? { Authorization: authorization } : undefined,
    referrerPolicy: "no-referrer",
    cache: "no-store",
  });
  if (!response.ok) {
    const result = (await response.json().catch(() => null)) as { error?: ManagerError } | null;
    throw new BodyFetchError(
      result?.error ?? {
        code: `http_${response.status}`,
        message: t("消息读取失败 ({status})", { status: response.status }),
      },
    );
  }
  const bytes = new Uint8Array(await response.arrayBuffer());
  return {
    bytes,
    decodedSize: bytes.length,
    contentType: response.headers.get("content-type") ?? "application/octet-stream",
  };
}
