// 重放草稿与预填、headers 表格规范化、header 警告的纯逻辑。

import type {
  BodySourceType,
  InterceptorSnapshotPayload,
  LogDetail,
} from "../api/types";

export type ReplayBodySpec =
  | { type: "text"; text: string; charset: "utf8" }
  | {
      type: "body_ref";
      session_id: number;
      log_id: number;
      side: "request" | "response";
    }
  | { type: "asset"; asset_id: string };

export interface ReplayDraft {
  method: string;
  url: string;
  headers: Array<[string, string]>;
  body: ReplayBodySpec | null;
}

export const REPLAY_METHODS = [
  "GET",
  "POST",
  "PUT",
  "DELETE",
  "PATCH",
  "HEAD",
  "OPTIONS",
] as const;

/** 日志详情页预填：最终值；body 非空时用 body_ref 指向该日志的请求 body。 */
export function prefillFromLog(detail: LogDetail): ReplayDraft {
  return {
    method: detail.request.method,
    url: detail.request.uri,
    headers: detail.request.headers.map((h) => [h.name, h.value]),
    body:
      detail.request.body.type === "empty"
        ? null
        : {
            type: "body_ref",
            session_id: detail.session_id,
            log_id: detail.id,
            side: "request",
          },
  };
}

/** 快照页预填：仅请求侧快照可用；string→text，asset→asset，original→body_ref。 */
export function prefillFromSnapshot(
  sessionId: number,
  logId: number,
  snapshot: InterceptorSnapshotPayload,
): ReplayDraft | null {
  const request = snapshot.request;
  if (!request) return null;
  return {
    method: request.method,
    url: request.uri,
    headers: Object.entries(request.headers).flatMap(([name, values]) =>
      values.map((value) => [name, value] as [string, string]),
    ),
    body: bodySpecFromSource(sessionId, logId, "request", request.body),
  };
}

function bodySpecFromSource(
  sessionId: number,
  logId: number,
  side: "request" | "response",
  source: BodySourceType,
): ReplayBodySpec | null {
  switch (source.type) {
    case "original":
      return { type: "body_ref", session_id: sessionId, log_id: logId, side };
    case "string":
      return { type: "text", text: source.content, charset: "utf8" };
    case "asset":
      return { type: "asset", asset_id: source.asset_id };
  }
}

export interface KvRow {
  name: string;
  value: string;
}

/** 规范化：删除中间全空行，末尾保留恰好一个全空行。 */
export function normalizeKvRows(rows: KvRow[]): KvRow[] {
  const cleaned = rows.filter((row) => row.name !== "" || row.value !== "");
  cleaned.push({ name: "", value: "" });
  return cleaned;
}

/** 提交用：去掉全空行，转二元组。 */
export function kvRowsToTuples(rows: KvRow[]): Array<[string, string]> {
  return rows
    .filter((row) => row.name !== "" || row.value !== "")
    .map((row) => [row.name, row.value]);
}

export function headerValue(
  headers: Array<[string, string]>,
  name: string,
): string | null {
  const found = headers.find(
    ([key]) => key.toLowerCase() === name.toLowerCase(),
  );
  return found ? found[1] : null;
}

/** body 字节数已知时校验 Content-Length。 */
export function contentLengthIssue(
  headers: Array<[string, string]>,
  bodySize: number | null,
): "missing" | "mismatch" | null {
  if (bodySize === null || bodySize === 0) return null;
  const value = headerValue(headers, "content-length");
  if (value === null) return "missing";
  return Number(value) === bodySize ? null : "mismatch";
}

/** 按文本内容猜测 Content-Type。 */
export function guessContentType(text: string): string {
  const trimmed = text.trim();
  if (trimmed === "") return "application/octet-stream";
  try {
    JSON.parse(trimmed);
    return "application/json";
  } catch {
    return "text/plain";
  }
}
