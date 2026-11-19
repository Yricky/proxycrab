import type { Backend } from "./backend";
import type { Backend as PageBackend } from "./session-page-backend";
import { t } from "../i18n";
import { BackendError } from "./backend-error";
import type { ApiError } from "./types";
import { downloadBodyFromBase, fetchBodyFromBase, fetchWebSocketBodyFromBase } from "./body";
import { saveFileInWebview } from "./save-file";
import type { AppConfig } from "./generated/AppConfig";
import type { InterceptorSnapshot } from "./generated/InterceptorSnapshot";
import type { LogDetail } from "./generated/LogDetail";
import type { ManagerError } from "./generated/ManagerError";
import type { ProxyStatus } from "./generated/ProxyStatus";
import type { SessionMetadata } from "./generated/SessionMetadata";
import type { SessionInterceptorsPayload } from "./generated/SessionInterceptorsPayload";
import type { SessionViewPayload } from "./generated/SessionViewPayload";
import type { Script } from "./types";

import type { HttpApiChange } from "./generated/HttpApiChange";
import { shareChanges } from "./changes";
import { subscribeSse } from "./sse";

interface SharedChange extends HttpApiChange {
  session_config_download: boolean;
  redaction_revision: string | null;
}

interface ApiEnvelope<T> {
  ok: boolean;
  data?: T;
  error?: ManagerError;
}

export interface ShareBootstrap {
  session_config_download?: boolean;
  session: SessionMetadata;
  view: SessionViewPayload;
  proxy_status: ProxyStatus;
  api_port: number;
}

export const SHARE_INVALID_EVENT = "proxycrab-share-invalid";

async function request<T>(
  baseUrl: string,
  token: string,
  path: string,
  init: RequestInit = {},
): Promise<T> {
  const headers = new Headers(init.headers);
  if (init.body !== undefined) headers.set("Content-Type", "application/json");
  const url = new URL(path, baseUrl);
  url.searchParams.append("token", token);
  let response: Response;
  try {
    response = await fetch(url, { ...init, headers, referrerPolicy: "no-referrer" });
  } catch (cause) {
    throw new BackendError({ code: "network_error", message: String(cause) });
  }
  let envelope: ApiEnvelope<T> | undefined;
  try {
    envelope = (await response.json()) as ApiEnvelope<T>;
  } catch {
    // Fall through to the status-derived error.
  }
  if (!response.ok || !envelope?.ok) {
    const error = envelope?.error ?? {
      code: `http_${response.status}`,
      message: `HTTP ${response.status}`,
    };
    if (["invalid_share_token", "share_session_unavailable"].includes(error.code)) {
      window.dispatchEvent(new CustomEvent(SHARE_INVALID_EVENT, { detail: error }));
    }
    throw new BackendError(error);
  }
  return envelope.data as T;
}

function json(value: unknown): RequestInit {
  return { method: "POST", body: JSON.stringify(value) };
}

export function verifyShareAccess(baseUrl: string, token: string): Promise<ShareBootstrap> {
  return request(baseUrl, token, "/share-api/bootstrap");
}

export function createShareBackend(
  baseUrl: string,
  token: string,
  bootstrap: ShareBootstrap,
): Backend {
  const call = <T>(path: string, init?: RequestInit) => request<T>(baseUrl, token, path, init);
  const unsupported = (name: PropertyKey) => async () => {
    throw new BackendError({
      code: "readonly_share",
      message: t("分享页面不支持操作：{name}", { name: String(name) }),
    });
  };
  const config: AppConfig = {
    proxy_host: "0.0.0.0",
    proxy_port: bootstrap.proxy_status.status === "running" ? bootstrap.proxy_status.port : 8089,
    api_port: bootstrap.api_port,
    routing_script_name: null,
    active_session_id: bootstrap.session.id,
  };
  // 分享后端只实现 /share-api 暴露的只读方法：未实现的方法（replay、Session 写入、
  // 审批等）保持 undefined，UI 直接以方法是否存在决定入口是否展示。
  const downloadConfig = async () => {
    const url = new URL("/share-api/session-config", baseUrl);
    url.searchParams.set("token", token);
    const response = await fetch(url);
    if (!response.ok) {
      const payload = (await response.json().catch(() => null)) as { error?: ManagerError } | null;
      throw new BackendError(
        payload?.error ?? { code: `http_${response.status}`, message: t("下载 Session 配置失败") },
      );
    }
    const { saveBlobInWebview } = await import("./save-file");
    await saveBlobInWebview(await response.blob(), "session-config.zip");
  };
  const implemented: PageBackend = {
    downloadSessionConfig: bootstrap.session_config_download ? downloadConfig : undefined,
    host: undefined,
    downloadBody: (target, side, filename) => {
      if (target.kind === "breakpoint") return unsupported("download breakpoint body")();
      return downloadBodyFromBase(baseUrl, target, side, filename, undefined, "/share-api", token);
    },
    fetchBody: (target, side, maxSize) => {
      if (target.kind === "breakpoint") return unsupported("fetch breakpoint body")();
      return fetchBodyFromBase(baseUrl, target, side, maxSize, undefined, "/share-api", token);
    },
    subscribeChanges: shareChanges(async (handler) => {
      let redactionRevision = bootstrap.view.redaction_revision ?? null;
      const invalidateRedaction = (revision: string | null) => {
        if (revision === redactionRevision) return;
        redactionRevision = revision;
        window.dispatchEvent(new CustomEvent("proxycrab-redaction-changed"));
      };
      const url = new URL("/share-api/changes", baseUrl);
      url.searchParams.set("token", token);
      return subscribeSse<SharedChange>(
        url,
        { referrerPolicy: "no-referrer" },
        (change) => {
          const canDownload = change.session_config_download === true;
          if (Boolean(implemented.downloadSessionConfig) !== canDownload) {
            implemented.downloadSessionConfig = canDownload ? downloadConfig : undefined;
            if (
              !change.resources.some(
                (resource) => resource.kind === "session_share" || resource.kind === "all",
              )
            ) {
              change.resources.push({ kind: "session_share" });
            }
          }
          invalidateRedaction(change.redaction_revision);
          if (change.resources.length)
            handler({ resources: change.resources, session_id: change.session_id });
        },
        (error) => {
          if (error.code === "redaction_failed") invalidateRedaction("failed");
          if (!["invalid_share_token", "share_session_unavailable"].includes(error.code))
            return false;
          window.dispatchEvent(new CustomEvent(SHARE_INVALID_EVENT, { detail: error }));
          return true;
        },
      );
    }),
    subscribeApprovalChanges: async () => () => undefined,
    openExternal: async (url) => {
      const link = document.createElement("a");
      link.href = url;
      link.target = "_blank";
      link.rel = "noopener noreferrer";
      link.click();
    },
    saveFile: saveFileInWebview,
    getConfig: async () => config,
    getProxyStatus: () => call("/share-api/proxy/status"),
    listSessions: async () => ({
      sessions: [bootstrap.session],
      active_session_id: bootstrap.session.id,
    }),
    listArchivedSessions: async () => [],
    getLogIds: (value) => call("/share-api/logs/ids", json(value)),
    getLogViews: (value) => call("/share-api/logs/views", json(value)),
    validateFilterRegex: (pattern) => call("/share-api/validate-filter-regex", json({ pattern })),
    getWebSocketMessages: (_sessionId, id, side, afterId, limit = 500) =>
      call(
        `/share-api/logs/${id}/websocket/messages?side=${side}&after_id=${afterId}&limit=${limit}`,
      ),
    fetchWebSocketMessageBody: (_sessionId, id, side, messageId) =>
      fetchWebSocketBodyFromBase(
        baseUrl,
        bootstrap.session.id,
        id,
        side,
        messageId,
        undefined,
        "/share-api",
        token,
      ),
    getLog: (_sessionId, id) => call<LogDetail>(`/share-api/logs/${id}`),
    getInterceptorContent: async (_sessionId, id, executionId) => {
      const url = new URL(`/share-api/logs/${id}/interceptors/${executionId}/content`, baseUrl);
      url.searchParams.append("token", token);
      let response: Response;
      try {
        response = await fetch(url, { referrerPolicy: "no-referrer" });
      } catch (cause) {
        throw new BackendError({ code: "network_error", message: String(cause) });
      }
      if (!response.ok) {
        let error: ApiError = {
          code: `http_${response.status}`,
          message: `HTTP ${response.status}`,
        };
        try {
          const payload = (await response.json()) as { error?: ManagerError };
          if (payload.error) error = payload.error;
        } catch {
          // Keep the status-derived fallback when the server did not return JSON.
        }
        throw new BackendError(error);
      }
      return {
        hash: response.headers.get("x-proxycrab-script-sha256") ?? "",
        content: await response.text(),
      };
    },
    getInterceptorSnapshot: (_sessionId, id, executionId) =>
      call<InterceptorSnapshot>(`/share-api/logs/${id}/interceptors/${executionId}/snapshot`),
    getSessionView: () => call("/share-api/session-view"),
    getSessionInterceptors: () =>
      call<SessionInterceptorsPayload>("/share-api/session-interceptors"),
    listColumnScripts: () => call<Script[]>("/share-api/column-scripts"),
    listFilterScripts: () => call<Script[]>("/share-api/filter-scripts"),
  };
  return implemented as Backend;
}
