import type { Backend } from "../api/backend";
import { t } from "../i18n";
import { UI_BODY_MAX_SIZE } from "../api/body";
import type { FileWriter } from "../api/save-file";
import type { BodyPayload } from "../api/generated/BodyPayload";
import type { HeaderItem } from "../api/generated/HeaderItem";
import type { LogDetail } from "../api/generated/LogDetail";
import { version } from "../../package.json";

export interface ExportProgress {
  processed: number;
  total: number;
  exported: number;
  skipped: number;
  warnings: number;
}
export type ExportLog = (level: "info" | "warning" | "error", message: string) => void;
export function errorMessage(error: unknown): string {
  return error instanceof Error
    ? error.message
    : String((error as { message?: unknown } | null)?.message ?? error);
}
function header(headers: HeaderItem[], name: string): string {
  return headers.find((h) => h.name.toLowerCase() === name)?.value ?? "";
}
function cookies(headers: HeaderItem[], response: boolean) {
  return headers
    .filter((h) => h.name.toLowerCase() === (response ? "set-cookie" : "cookie"))
    .flatMap((h) => (response ? [h.value] : h.value.split(";")))
    .flatMap((value) => {
      const [pair = "", ...attributes] = value.split(";");
      const separator = pair.indexOf("=");
      if (separator < 1) return [];
      const cookie: Record<string, string | boolean> = {
        name: pair.slice(0, separator).trim(),
        value: pair.slice(separator + 1).trim(),
      };
      for (const attr of attributes) {
        const [name = "", ...parts] = attr.trim().split("=");
        const key = name.toLowerCase();
        if (key === "path" || key === "domain") cookie[key] = parts.join("=");
        if (key === "secure") cookie.secure = true;
        if (key === "httponly") cookie.httpOnly = true;
      }
      return [cookie];
    });
}
function base64(bytes: Uint8Array): string {
  const parts: string[] = [];
  // Chunk on multiples of three so independently encoded pieces can be joined.
  for (let offset = 0; offset < bytes.length; offset += 24_576) {
    parts.push(btoa(String.fromCharCode(...bytes.subarray(offset, offset + 24_576))));
  }
  return parts.join("");
}

async function readBody(
  backend: Backend,
  detail: LogDetail,
  side: "request" | "response",
  payload: BodyPayload,
  signal: AbortSignal,
  log: ExportLog,
) {
  const headers = side === "request" ? detail.request.headers : (detail.response?.headers ?? []);
  const mimeType = header(headers, "content-type") || "application/octet-stream";
  if (payload.type === "empty") return { size: 0, storedSize: 0, mimeType };
  try {
    const body = await backend.fetchBody(
      { kind: "log", id: detail.id, sessionId: detail.session_id },
      side,
      UI_BODY_MAX_SIZE,
    );
    signal.throwIfAborted();
    let text: string;
    let encoding: "base64" | undefined;
    try {
      if (!/^text\/|json|xml|javascript|x-www-form-urlencoded/i.test(mimeType))
        throw new Error("binary");
      text = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(body.bytes);
    } catch {
      text = base64(body.bytes);
      encoding = "base64";
    }
    return { size: body.bytes.length, storedSize: payload.size, mimeType, text, encoding };
  } catch (error) {
    signal.throwIfAborted();
    const message = t("Body 读取失败（{side}）：{error}", { side, error: errorMessage(error) });
    log("warning", `#${detail.id} ${message}`);
    const text = `[ProxyCrab HAR export] ${message}`;
    return {
      size: new TextEncoder().encode(text).length,
      storedSize: payload.size,
      mimeType: "text/plain; charset=utf-8",
      text,
      error: message,
    };
  }
}

export async function buildHarEntry(
  backend: Backend,
  detail: LogDetail,
  signal: AbortSignal,
  log: ExportLog,
) {
  const requestBody = await readBody(backend, detail, "request", detail.request.body, signal, log);
  const responseBody = await readBody(
    backend,
    detail,
    "response",
    detail.response?.body ?? { type: "empty" },
    signal,
    log,
  );
  let queryString: { name: string; value: string }[] = [];
  try {
    queryString = [...new URL(detail.request.uri).searchParams].map(([name, value]) => ({
      name,
      value,
    }));
  } catch {
    /* Preserve malformed URLs verbatim. */
  }
  return {
    startedDateTime: new Date(detail.created_at).toISOString(),
    time: Math.max(0, detail.updated_at - detail.created_at),
    request: {
      method: detail.request.method,
      url: detail.request.uri,
      httpVersion: detail.request.version,
      headers: detail.request.headers,
      queryString,
      cookies: cookies(detail.request.headers, false),
      headersSize: -1,
      bodySize: requestBody.storedSize,
      ...(requestBody.text === undefined
        ? {}
        : {
            postData: {
              mimeType: requestBody.mimeType,
              text: requestBody.text,
              ...(requestBody.encoding ? { _encoding: requestBody.encoding } : {}),
            },
          }),
    },
    response: {
      status: detail.response?.status ?? 0,
      statusText: detail.response?.status_text ?? "",
      httpVersion: detail.response?.version ?? detail.request.version,
      headers: detail.response?.headers ?? [],
      cookies: cookies(detail.response?.headers ?? [], true),
      content: {
        size: responseBody.size,
        mimeType: responseBody.mimeType,
        text: responseBody.text,
        encoding: responseBody.encoding,
      },
      redirectURL: header(detail.response?.headers ?? [], "location"),
      headersSize: -1,
      bodySize: responseBody.storedSize,
    },
    cache: {},
    timings: { blocked: -1, dns: -1, connect: -1, ssl: -1, send: -1, wait: -1, receive: -1 },
    _proxyCrab: {
      logId: detail.id,
      sessionId: detail.session_id,
      tags: detail.request.tags,
      stage: detail.stage,
      sourceAddr: detail.source_addr,
      createdAt: detail.created_at,
      updatedAt: detail.updated_at,
      outcome: detail.outcome,
      error: detail.error,
      ...(requestBody.error || responseBody.error
        ? {
            bodyErrors: {
              request: requestBody.error,
              response: responseBody.error,
            },
          }
        : {}),
    },
  };
}

/** Reads and writes one capture at a time. The caller opens/commits/aborts the Writer. */
export async function appendHar(
  backend: Backend,
  writer: FileWriter,
  sessionId: number,
  ids: number[],
  signal: AbortSignal,
  onProgress: (progress: ExportProgress) => void,
  log: ExportLog,
): Promise<ExportProgress> {
  const encoder = new TextEncoder();
  const progress: ExportProgress = {
    processed: 0,
    total: ids.length,
    exported: 0,
    skipped: 0,
    warnings: 0,
  };
  const report: ExportLog = (level, message) => {
    if (level === "warning") progress.warnings++;
    log(level, message);
  };
  const append = async (text: string) => {
    signal.throwIfAborted();
    await writer.append(encoder.encode(text));
  };
  onProgress({ ...progress });
  await append(
    `{"log":{"version":"1.2","creator":${JSON.stringify({ name: "ProxyCrab", version })},"entries":[`,
  );
  for (const id of ids) {
    signal.throwIfAborted();
    let detail: LogDetail;
    try {
      detail = await backend.getLog(sessionId, id);
    } catch (error) {
      signal.throwIfAborted();
      // Record unavailable captures rather than silently dropping a selected row.
      report(
        "warning",
        t("#{id} 详情读取失败：{error}，已保留失败占位记录", { id, error: errorMessage(error) }),
      );
      detail = {
        id,
        session_id: sessionId,
        created_at: 0,
        updated_at: 0,
        source_type: "",
        source_addr: null,
        stage: "export_error",
        outcome: "failed",
        error: { stage: "upstream", kind: "export_error", message: errorMessage(error) },
        request: {
          method: "",
          uri: "",
          version: "",
          headers: [],
          tags: {},
          body: { type: "empty" },
        },
        response: null,
        request_interceptors: [],
        response_interceptors: [],
      };
    }
    signal.throwIfAborted();
    if (
      detail.request.version.startsWith("WebSocket/") ||
      (detail.request.method.toUpperCase() === "CONNECT" && detail.stage === "tls_mitm")
    ) {
      progress.skipped++;
      report("info", t("#{id} 跳过 WebSocket 连接或合成 TLS CONNECT 记录", { id }));
    } else {
      const entry = await buildHarEntry(backend, detail, signal, report);
      await append((progress.exported ? "," : "") + JSON.stringify(entry));
      progress.exported++;
      report(
        "info",
        t("#{id} 已写入 {method} {status}", {
          id,
          method: detail.request.method,
          status: detail.response?.status ?? 0,
        }),
      );
    }
    progress.processed++;
    onProgress({ ...progress });
    // Let progress and cancellation UI paint even when reads resolve from a local cache.
    await new Promise<void>((resolve) => setTimeout(resolve, 0));
  }
  await append("]}}");
  return progress;
}
