import type { Backend } from "./backend";
import { BackendError } from "./backend-error";
import { fetchBodyFromBase } from "./body";
import type {
  AppConfig,
  LogDetail,
  ManagerError,
  ProxyStatus,
  Script,
  SessionMetadata,
  SessionViewPayload,
} from "./types";

interface ApiEnvelope<T> {
  ok: boolean;
  data?: T;
  error?: ManagerError;
}

export interface ShareBootstrap {
  session: SessionMetadata;
  view: SessionViewPayload;
  proxy_status: ProxyStatus;
  api_port: number;
  expires_at: number;
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
    if (["invalid_share_token", "share_expired", "share_session_unavailable"].includes(error.code)) {
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
      message: `分享页面不支持操作：${String(name)}`,
    });
  };
  const config: AppConfig = {
    proxy_host: "0.0.0.0",
    proxy_port: bootstrap.proxy_status.status === "running" ? bootstrap.proxy_status.port : 8089,
    api_port: bootstrap.api_port,
    routing_script_name: null,
    active_session_id: bootstrap.session.id,
  };
  const implemented: Partial<Backend> = {
    capabilities: {
      target: "share",
      approvals: false,
      permissionModes: [],
      readonly: true,
    },
    host: undefined,
    fetchBody: (target, side, maxSize) => {
      if (target.kind !== "log") return unsupported("fetch breakpoint body")();
      return fetchBodyFromBase(
        baseUrl,
        target,
        side,
        maxSize,
        undefined,
        "/share-api",
        token,
      );
    },
    subscribeChanges: async (handler) => {
      let stopped = false;
      let columns = JSON.stringify(bootstrap.view.columns);
      const poll = async () => {
        if (stopped) return;
        try {
          const view = await call<SessionViewPayload>("/share-api/session-view");
          const next = JSON.stringify(view.columns);
          if (next !== columns) {
            columns = next;
            handler({ resources: ["session_view"], session_id: bootstrap.session.id });
          }
        } catch {
          // The request helper announces terminal share errors; network errors retry.
        }
      };
      const timer = window.setInterval(() => void poll(), 1500);
      return () => {
        stopped = true;
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
    getConfig: async () => config,
    getProxyStatus: () => call("/share-api/proxy/status"),
    listSessions: async () => [bootstrap.session],
    listArchivedSessions: async () => [],
    getActiveSession: async () => ({ session_id: bootstrap.session.id }),
    getLogIds: (value) => call("/share-api/logs/ids", json({ ...value, persist_filter: false })),
    getLogViews: (value) => call("/share-api/logs/views", json(value)),
    validateFilterRegex: (pattern) =>
      call("/share-api/validate-filter-regex", json({ pattern })),
    getLog: (_sessionId, id) => call<LogDetail>(`/share-api/logs/${id}`),
    getSessionView: () => call("/share-api/session-view"),
    listColumnScripts: () => call<Script[]>("/share-api/column-scripts"),
    listFilterScripts: () => call<Script[]>("/share-api/filter-scripts"),
  };
  return new Proxy(implemented as Backend, {
    get(target, property, receiver) {
      if (Reflect.has(target, property)) return Reflect.get(target, property, receiver);
      return unsupported(property);
    },
  });
}
