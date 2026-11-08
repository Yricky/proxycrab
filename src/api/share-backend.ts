import type { Backend } from "./backend";
import { t } from "../i18n";
import { BackendError } from "./backend-error";
import { downloadBodyFromBase, fetchBodyFromBase, fetchWebSocketBodyFromBase } from "./body";
import { saveFileInWebview } from "./save-file";
import type {
  AppConfig,
  InterceptorSnapshotPayload,
  LogDetail,
  ManagerError,
  ProxyStatus,
  Script,
  SessionMetadata,
  SessionInterceptorsPayload,
  SessionViewPayload,
} from "./types";

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
  const implemented: Partial<Backend> = {
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
    subscribeChanges: async (handler) => {
      let stopped = false;
      let columns = JSON.stringify(bootstrap.view.columns);
      let redactionRevision = bootstrap.view.redaction_revision ?? null;
      let columnScripts = "";
      let filterScripts = "";
      let interceptors = "";
      let polling = false;
      let proxyRevision: string | undefined;
      const proxyAbort = new AbortController();
      const poll = async () => {
        if (stopped || polling) return;
        polling = true;
        try {
          const [view, nextColumns, nextFilters, nextInterceptors, capabilities] =
            await Promise.all([
              call<SessionViewPayload>("/share-api/session-view"),
              call<Script[]>("/share-api/column-scripts"),
              call<Script[]>("/share-api/filter-scripts"),
              call<SessionInterceptorsPayload>("/share-api/session-interceptors"),
              call<{ session_config_download: boolean }>("/share-api/capabilities"),
            ]);
          if (stopped) return;
          if (
            Boolean(implemented.downloadSessionConfig) !==
            (capabilities.session_config_download === true)
          ) {
            implemented.downloadSessionConfig =
              capabilities.session_config_download === true ? downloadConfig : undefined;
            handler({ resources: ["session_share"], session_id: bootstrap.session.id });
          }
          const revision = view.redaction_revision ?? null;
          if (revision !== redactionRevision) {
            redactionRevision = revision;
            window.dispatchEvent(new CustomEvent("proxycrab-redaction-changed"));
          }
          const next = JSON.stringify(view.columns);
          if (next !== columns) {
            columns = next;
            handler({ resources: ["session_view"], session_id: bootstrap.session.id });
          }
          const nextInterceptorConfig = JSON.stringify(nextInterceptors);
          if (nextInterceptorConfig !== interceptors) {
            interceptors = nextInterceptorConfig;
            handler({ resources: ["session_interceptors"], session_id: bootstrap.session.id });
          }
          const nextColumnScripts = JSON.stringify(nextColumns);
          const nextFilterScripts = JSON.stringify(nextFilters);
          if (nextColumnScripts !== columnScripts) {
            columnScripts = nextColumnScripts;
            handler({ resources: ["column_scripts"], session_id: bootstrap.session.id });
          }
          if (nextFilterScripts !== filterScripts) {
            filterScripts = nextFilterScripts;
            handler({ resources: ["filter_scripts"], session_id: bootstrap.session.id });
          }
        } catch (error) {
          if (
            error instanceof BackendError &&
            error.code === "redaction_failed" &&
            redactionRevision !== "failed"
          ) {
            redactionRevision = "failed";
            window.dispatchEvent(new CustomEvent("proxycrab-redaction-changed"));
          }
          // The request helper announces terminal share errors; network errors retry.
        } finally {
          polling = false;
        }
      };
      const watchProxy = async () => {
        while (!stopped) {
          try {
            const query =
              proxyRevision === undefined ? "" : `?after=${encodeURIComponent(proxyRevision)}`;
            const change = await call<{ revision: string; changed: boolean }>(
              `/share-api/proxy/changes${query}`,
              { signal: proxyAbort.signal },
            );
            proxyRevision = change.revision;
            if (change.changed) {
              handler({ resources: ["proxy"], session_id: bootstrap.session.id });
            }
          } catch {
            if (!stopped) await new Promise((resolve) => window.setTimeout(resolve, 1000));
          }
        }
      };
      void watchProxy();
      const timer = window.setInterval(() => void poll(), 1500);
      return () => {
        stopped = true;
        proxyAbort.abort();
        window.clearInterval(timer);
      };
    },
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
        let error: ManagerError = {
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
      call<InterceptorSnapshotPayload>(
        `/share-api/logs/${id}/interceptors/${executionId}/snapshot`,
      ),
    getSessionView: () => call("/share-api/session-view"),
    getSessionInterceptors: () =>
      call<SessionInterceptorsPayload>("/share-api/session-interceptors"),
    listColumnScripts: () => call<Script[]>("/share-api/column-scripts"),
    listFilterScripts: () => call<Script[]>("/share-api/filter-scripts"),
  };
  return implemented as Backend;
}
