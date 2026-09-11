import type { Backend } from "./backend";
import { BackendError } from "./backend-error";
import { fetchBodyFromBase } from "./body";
import type {
  ActiveSession,
  ApiActionView,
  AssetMetadata,
  BreakpointQuery,
  BypassQuery,
  CreatedApiKey,
  CreateAgentsPresetRequest,
  CreateSessionRequest,
  DebugFilterScriptRequest,
  ExecuteTemporaryScriptRequest,
  ExtendBreakpointRequest,
  EnableHarShareRequest,
  HarShareState,
  HttpApiChange,
  HttpServiceStatus,
  IdentityPermissions,
  InterceptorCreateRequest,
  InterceptorKind,
  InterceptorSnapshotPayload,
  InterceptorUpdateRequest,
  LogDetail,
  LogIdsRequest,
  LogViewsRequest,
  ManagerError,
  PendingApproval,
  PermissionEntry,
  PermissionIdentitySummary,
  ReplaceSessionInterceptorsRequest,
  ReplaceSessionViewRequest,
  ReplayRequestPayload,
  ReplayResult,
  ResolveApprovalRequest,
  RoutingSelection,
  SessionShareState,
  ScriptRequest,
  SystemLogsQuery,
  UpdateAgentsPresetRequest,
  UpdateScriptRequest,
  UpdateSessionRequest,
  WorkspacePaths,
} from "./types";

interface ApiEnvelope<T> {
  ok: boolean;
  data?: T;
  error?: ManagerError;
}

export interface CliBootstrap {
  target: "cli";
  workspace: WorkspacePaths;
  http_service: HttpServiceStatus;
}

function authorization(token: string): string {
  return `Bearer ${token}`;
}

function query(path: string, values: Record<string, unknown>): string {
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(values)) {
    if (value !== undefined && value !== null && value !== "") {
      params.set(key, String(value));
    }
  }
  const encoded = params.toString();
  return encoded ? `${path}?${encoded}` : path;
}

async function request<T>(
  baseUrl: string,
  token: string,
  path: string,
  init: RequestInit = {},
): Promise<T> {
  const headers = new Headers(init.headers);
  headers.set("Authorization", authorization(token));
  if (init.body !== undefined) headers.set("Content-Type", "application/json");
  let response: Response;
  try {
    response = await fetch(new URL(path, baseUrl), { ...init, headers });
  } catch (cause) {
    throw new BackendError({ code: "network_error", message: String(cause) });
  }
  let envelope: ApiEnvelope<T> | undefined;
  try {
    envelope = (await response.json()) as ApiEnvelope<T>;
  } catch {
    // The status-derived error below is more useful than a JSON parse exception.
  }
  if (!response.ok || !envelope?.ok) {
    throw new BackendError(
      envelope?.error ?? {
        code: `http_${response.status}`,
        message: response.status === 401 ? "Access Token 无效" : `HTTP ${response.status}`,
      },
    );
  }
  return envelope.data as T;
}

function json(method: string, value?: unknown): RequestInit {
  return {
    method,
    ...(value === undefined ? {} : { body: JSON.stringify(value) }),
  };
}

export function verifyCliAccess(baseUrl: string, token: string): Promise<CliBootstrap> {
  return request(baseUrl, token, "/ui-api/bootstrap");
}

export function createHttpBackend(
  baseUrl: string,
  token: string,
  bootstrap: CliBootstrap,
): Backend {
  const call = <T>(path: string, init?: RequestInit) =>
    request<T>(baseUrl, token, path, init);
  const scriptPath = (scope: string, name: string) =>
    `/api/${scope}/${encodeURIComponent(name)}`;

  return {
    capabilities: {
      target: "cli",
      approvals: false,
      permissionModes: ["allow", "deny"],
      readonly: false,
    },
    fetchBody: (target, side, maxSize) =>
      fetchBodyFromBase(baseUrl, target, side, maxSize, authorization(token)),
    subscribeChanges: async (handler) => {
      let stopped = false;
      let cursor = 0;
      const poll = async () => {
        while (!stopped) {
          try {
            const result = await call<{ cursor: number; changes: HttpApiChange[] }>(
              query("/ui-api/changes", { after: cursor }),
            );
            cursor = result.cursor;
            for (const change of result.changes) handler(change);
          } catch {
            await new Promise((resolve) => window.setTimeout(resolve, 1000));
          }
        }
      };
      void poll();
      return () => { stopped = true; };
    },
    subscribeApprovalChanges: async () => () => undefined,
    openExternal: async (url) => {
      const link = document.createElement("a");
      link.href = url;
      link.target = "_blank";
      link.rel = "noopener noreferrer";
      link.click();
    },

    getWorkspace: async () => bootstrap.workspace,
    getConfig: () => call("/api/config"),
    getAgentsPresets: () => call("/ui-api/agents-presets"),
    createAgentsPreset: (requestValue: CreateAgentsPresetRequest) =>
      call("/ui-api/agents-presets", json("POST", requestValue)),
    updateAgentsPreset: (id: string, requestValue: UpdateAgentsPresetRequest) =>
      call(`/ui-api/agents-presets/${encodeURIComponent(id)}`, json("PUT", requestValue)),
    activateAgentsPreset: (id: string) =>
      call(`/ui-api/agents-presets/${encodeURIComponent(id)}/activate`, json("POST")),
    deleteAgentsPreset: (id: string) =>
      call(`/ui-api/agents-presets/${encodeURIComponent(id)}`, json("DELETE")),
    reimportDefaultAgentsPresets: () =>
      call("/ui-api/agents-presets/reimport-defaults", json("POST")),

    getProxyStatus: () => call("/api/proxy/status"),
    startProxy: () => call("/api/proxy/start", json("POST")),
    stopProxy: () => call("/api/proxy/stop", json("POST")),
    listLocalIps: () => call("/ui-api/local-ips"),

    listSessions: () => call("/api/sessions"),
    listArchivedSessions: () => call("/api/archived-sessions"),
    createSession: (requestValue: CreateSessionRequest) =>
      call("/api/sessions", json("POST", requestValue)),
    updateSession: (id: number, requestValue: UpdateSessionRequest) =>
      call(`/api/sessions/${id}`, json("PUT", requestValue)),
    archiveSession: (id: number) => call(`/api/sessions/${id}/archive`, json("POST")),
    restoreSession: (id: number) =>
      call(`/api/archived-sessions/${id}/restore`, json("POST")),
    deleteArchivedSession: (id: number) =>
      call(`/api/archived-sessions/${id}`, json("DELETE")),
    getActiveSession: () => call("/api/active-session"),
    replaceActiveSession: (active: ActiveSession) =>
      call("/api/active-session", json("PUT", active)),
    getSessionShare: (sessionId: number) =>
      call<SessionShareState>(`/api/session-shares/${sessionId}`),
    enableSessionShare: (sessionId: number) =>
      call<SessionShareState>(
        "/api/session-shares",
        json("POST", { session_id: sessionId }),
      ),
    disableSessionShare: (sessionId: number) =>
      call<SessionShareState>(
        `/api/session-shares/${sessionId}`,
        json("DELETE"),
      ),
    getHarShare: (sessionId: number) =>
      call<HarShareState>(`/api/session-har-shares/${sessionId}`),
    enableHarShare: (requestValue: EnableHarShareRequest) =>
      call<HarShareState>(
        "/api/session-har-shares",
        json("POST", requestValue),
      ),
    disableHarShare: (sessionId: number) =>
      call<HarShareState>(
        `/api/session-har-shares/${sessionId}`,
        json("DELETE"),
      ),

    getLogIds: (requestValue: LogIdsRequest) =>
      call("/api/logs/ids", json("POST", requestValue)),
    getLogViews: (requestValue: LogViewsRequest) =>
      call("/api/logs/views", json("POST", requestValue)),
    validateFilterRegex: (pattern: string) =>
      call("/ui-api/validate-filter-regex", json("POST", { pattern })),
    getLog: (sessionId: number | null, id: number) =>
      call<LogDetail>(query(`/api/logs/${id}`, { session_id: sessionId })),
    replay: (sessionId: number, request: ReplayRequestPayload) =>
      call<ReplayResult>(`/api/replay?session=${sessionId}`, json("POST", request)),
    listAssets: () => call<AssetMetadata[]>("/api/assets"),
    getInterceptorContent: async (sessionId, id, executionId) => {
      const url = new URL(
        query(`/api/logs/${id}/interceptors/${executionId}/content`, {
          session_id: sessionId,
        }),
        baseUrl,
      );
      let response: Response;
      try {
        response = await fetch(url, {
          headers: { Authorization: authorization(token) },
        });
      } catch (cause) {
        throw new BackendError({ code: "network_error", message: String(cause) });
      }
      if (!response.ok) {
        let error: ManagerError = {
          code: `http_${response.status}`,
          message: `HTTP ${response.status}`,
        };
        try {
          const payload = (await response.json()) as ApiEnvelope<unknown>;
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
    getInterceptorSnapshot: (sessionId, id, executionId) =>
      call<InterceptorSnapshotPayload>(
        query(`/api/logs/${id}/interceptors/${executionId}/snapshot`, {
          session_id: sessionId,
        }),
      ),
    listBreakpoints: (requestValue: BreakpointQuery) =>
      call(query("/api/breakpoints", requestValue as unknown as Record<string, unknown>)),
    getBreakpoint: (id: number) => call(`/api/breakpoints/${id}`),
    extendBreakpoint: (id: number, requestValue: ExtendBreakpointRequest) =>
      call(`/api/breakpoints/${id}/extend`, json("POST", requestValue)),
    releaseBreakpoint: (id: number) =>
      call(`/api/breakpoints/${id}/release`, json("POST")),
    executeBreakpointScript: (id: number, requestValue: ExecuteTemporaryScriptRequest) =>
      call(`/api/breakpoints/${id}/execute`, json("POST", requestValue)),
    getSessionView: (sessionId: number | null) =>
      call(query("/api/session-view", { session_id: sessionId })),
    replaceSessionView: (sessionId: number | null, requestValue: ReplaceSessionViewRequest) =>
      call(query("/api/session-view", { session_id: sessionId }), json("PUT", requestValue)),
    replaceSessionFilter: (sessionId, filter) =>
      call(`/api/sessions/${sessionId}/filter`, json("PUT", filter)),

    listColumnScripts: () => call("/api/column-scripts"),
    createColumnScript: (requestValue: ScriptRequest) =>
      call("/api/column-scripts", json("POST", requestValue)),
    getColumnScript: (name: string) => call(scriptPath("column-scripts", name)),
    updateColumnScript: (name: string, requestValue: UpdateScriptRequest) =>
      call(scriptPath("column-scripts", name), json("PUT", requestValue)),
    deleteColumnScript: (name: string) =>
      call(scriptPath("column-scripts", name), json("DELETE")),

    listFilterScripts: () => call("/api/filter-scripts"),
    createFilterScript: (requestValue: ScriptRequest) =>
      call("/api/filter-scripts", json("POST", requestValue)),
    getFilterScript: (name: string) => call(scriptPath("filter-scripts", name)),
    updateFilterScript: (name: string, requestValue: UpdateScriptRequest) =>
      call(scriptPath("filter-scripts", name), json("PUT", requestValue)),
    deleteFilterScript: (name: string) =>
      call(scriptPath("filter-scripts", name), json("DELETE")),
    debugFilterScript: (name: string, requestValue: DebugFilterScriptRequest) =>
      call(`${scriptPath("filter-scripts", name)}/debug`, json("POST", requestValue)),

    listRoutingScripts: () => call("/api/routing-scripts"),
    createRoutingScript: (requestValue: ScriptRequest) =>
      call("/api/routing-scripts", json("POST", requestValue)),
    getRoutingScript: (name: string) => call(scriptPath("routing-scripts", name)),
    updateRoutingScript: (name: string, requestValue: UpdateScriptRequest) =>
      call(scriptPath("routing-scripts", name), json("PUT", requestValue)),
    deleteRoutingScript: (name: string) =>
      call(scriptPath("routing-scripts", name), json("DELETE")),
    getRoutingSelection: () => call("/api/routing-script-selection"),
    replaceRoutingSelection: (selection: RoutingSelection) =>
      call("/api/routing-script-selection", json("PUT", selection)),

    listInterceptors: (kind: InterceptorKind) =>
      call(query("/api/interceptors", { kind })),
    createInterceptor: (requestValue: InterceptorCreateRequest) =>
      call("/api/interceptors", json("POST", requestValue)),
    getInterceptor: (kind: InterceptorKind, name: string) =>
      call(`/api/interceptors/${kind}/${encodeURIComponent(name)}`),
    updateInterceptor: (
      kind: InterceptorKind,
      name: string,
      requestValue: InterceptorUpdateRequest,
    ) => call(`/api/interceptors/${kind}/${encodeURIComponent(name)}`, json("PUT", requestValue)),
    deleteInterceptor: (kind: InterceptorKind, name: string) =>
      call(`/api/interceptors/${kind}/${encodeURIComponent(name)}`, json("DELETE")),
    getSessionInterceptors: (sessionId: number | null) =>
      call(query("/api/session-interceptors", { session_id: sessionId })),
    replaceSessionInterceptors: (
      sessionId: number | null,
      requestValue: ReplaceSessionInterceptorsRequest,
    ) => call(
      query("/api/session-interceptors", { session_id: sessionId }),
      json("PUT", requestValue),
    ),

    getCertificate: () => call("/api/ca"),
    getSystemLogs: (requestValue: SystemLogsQuery) =>
      call(query("/api/system-logs", requestValue as unknown as Record<string, unknown>)),
    clearSystemLogs: () => call("/api/system-logs", json("DELETE")),
    getBypassEntries: (requestValue: BypassQuery) =>
      call(query("/api/bypass", requestValue as unknown as Record<string, unknown>)),
    deleteBypassEntry: (id: number) => call(`/api/bypass/${id}`, json("DELETE")),
    deleteBypassEntries: (ids: number[]) =>
      call("/api/bypass/delete", json("POST", { ids })),
    clearBypassEntries: () => call("/api/bypass", json("DELETE")),

    getHttpServiceError: async () => bootstrap.http_service.error,
    getHttpServiceStatus: async () => bootstrap.http_service,
    getHttpPermissionCatalog: () => call<ApiActionView[]>("/ui-api/permissions/catalog"),
    listHttpPermissionIdentities: () =>
      call<PermissionIdentitySummary[]>("/ui-api/permissions/identities"),
    getHttpIdentityPermissions: (id: string) =>
      call<IdentityPermissions>(`/ui-api/permissions/identities/${encodeURIComponent(id)}`),
    replaceHttpIdentityPermissions: (id: string, permissions: PermissionEntry[]) =>
      call<IdentityPermissions>(
        `/ui-api/permissions/identities/${encodeURIComponent(id)}`,
        json("PUT", { permissions }),
      ),
    createHttpApiKey: (name: string) =>
      call<CreatedApiKey>("/ui-api/api-keys", json("POST", { name })),
    deleteHttpApiKey: (id: string) =>
      call(`/ui-api/api-keys/${encodeURIComponent(id)}`, json("DELETE")),
    listHttpApprovals: async () => [] as PendingApproval[],
    resolveHttpApproval: async (_id: number, _requestValue: ResolveApprovalRequest) => {
      throw new BackendError({ code: "unsupported", message: "CLI 不支持接口审批" });
    },

  };
}
