import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { Backend } from "./backend";
import { BackendError } from "./backend-error";
import { fetchBodyFromHttp } from "./body";
import type {
  ActiveSession,
  ApiActionView,
  AppConfig,
  CreatedApiKey,
  CreateAgentsPresetRequest,
  BreakpointQuery,
  BypassQuery,
  CreateSessionRequest,
  DebugFilterScriptRequest,
  ExecuteTemporaryScriptRequest,
  ExtendBreakpointRequest,
  EnableHarShareRequest,
  HarShareState,
  InterceptorCreateRequest,
  InterceptorContent,
  InterceptorKind,
  InterceptorSnapshotPayload,
  InterceptorUpdateRequest,
  HttpServiceStatus,
  IdentityPermissions,
  LogDetail,
  LogIdsRequest,
  LogViewsRequest,
  ManagerError,
  PendingApproval,
  PermissionEntry,
  PermissionIdentitySummary,
  ReplaceSessionInterceptorsRequest,
  ReplaceSessionViewRequest,
  ResolveApprovalRequest,
  RoutingSelection,
  SessionShareState,
  ScriptRequest,
  SystemLogsQuery,
  UpdateScriptRequest,
  UpdateAgentsPresetRequest,
  UpdateSessionRequest,
} from "./types";

function isManagerError(value: unknown): value is ManagerError {
  return (
    typeof value === "object" &&
    value !== null &&
    "code" in value &&
    "message" in value &&
    typeof (value as ManagerError).code === "string" &&
    typeof (value as ManagerError).message === "string"
  );
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    if (isManagerError(error)) {
      throw new BackendError(error);
    }
    throw new BackendError({ code: "invoke_error", message: String(error) });
  }
}

/**
 * Backend implementation backed by Tauri commands.
 * Argument objects are serialized by the Tauri runtime, which expects
 * camelCase keys for the snake_case Rust parameters.
 */
export function createTauriBackend(): Backend {
  return {
    capabilities: {
      target: "tauri",
      approvals: true,
      permissionModes: ["allow", "approval", "deny"],
      readonly: false,
    },
    host: {
      skillManager: {
        getState: () => call("get_proxycrab_skill_manager_state"),
        savePaths: (paths: string[], deleteRemoved: boolean) =>
          call("save_proxycrab_skill_paths", { paths, deleteRemoved }),
        sync: () => call("sync_proxycrab_skills"),
      },
      setWorkspaceForNextStart: (path) =>
        call("set_workspace_for_next_start", { path }),
      replaceConfig: (config: AppConfig) => call("replace_config", { config }),
      regenerateCertificate: () => call("regenerate_certificate"),
    },
    fetchBody: async (target, side, maxSize) =>
      fetchBodyFromHttp(await call("get_config"), target, side, maxSize),
    subscribeChanges: async (handler) => {
      const unlisten = await listen("proxycrab://http-api-change", (event) =>
        handler(event.payload as import("./types").HttpApiChange),
      );
      return unlisten;
    },
    subscribeApprovalChanges: async (handler) => {
      const unlisten = await listen<number>("proxycrab://approval-change", (event) =>
        handler(event.payload),
      );
      return unlisten;
    },
    openExternal: (url) => openUrl(url),

    getWorkspace: () => call("get_workspace"),
    getConfig: () => call("get_config"),
    getAgentsPresets: () => call("get_agents_presets"),
    createAgentsPreset: (request: CreateAgentsPresetRequest) =>
      call("create_agents_preset", { request }),
    updateAgentsPreset: (id: string, request: UpdateAgentsPresetRequest) =>
      call("update_agents_preset", { id, request }),
    activateAgentsPreset: (id: string) => call("activate_agents_preset", { id }),
    deleteAgentsPreset: (id: string) => call("delete_agents_preset", { id }),
    reimportDefaultAgentsPresets: () => call("reimport_default_agents_presets"),

    getProxyStatus: () => call("get_proxy_status"),
    startProxy: () => call("start_proxy"),
    stopProxy: () => call("stop_proxy"),
    listLocalIps: () => call("list_local_ips"),

    listSessions: () => call("list_sessions"),
    listArchivedSessions: () => call("list_archived_sessions"),
    createSession: (request: CreateSessionRequest) => call("create_session", { request }),
    updateSession: (id: number, request: UpdateSessionRequest) =>
      call("update_session", { id, request }),
    archiveSession: (id: number) => call("archive_session", { id }),
    restoreSession: (id: number) => call("restore_session", { id }),
    deleteArchivedSession: (id: number) => call("delete_archived_session", { id }),
    getActiveSession: () => call("get_active_session"),
    replaceActiveSession: (active: ActiveSession) =>
      call("replace_active_session", { active }),
    getSessionShare: (sessionId: number) =>
      call<SessionShareState>("get_session_share", { sessionId }),
    enableSessionShare: (sessionId: number) =>
      call<SessionShareState>("enable_session_share", { sessionId }),
    disableSessionShare: (sessionId: number) =>
      call<SessionShareState>("disable_session_share", { sessionId }),
    getHarShare: (sessionId: number) =>
      call<HarShareState>("get_har_share", { sessionId }),
    enableHarShare: (request: EnableHarShareRequest) =>
      call<HarShareState>("enable_har_share", { request }),
    disableHarShare: (sessionId: number) =>
      call<HarShareState>("disable_har_share", { sessionId }),

    getLogIds: (request: LogIdsRequest) => call("get_log_ids", { request }),
    getLogViews: (request: LogViewsRequest) => call("get_log_views", { request }),
    validateFilterRegex: (pattern: string) =>
      call("validate_filter_regex", { pattern }),
    getLog: (sessionId: number | null, id: number) =>
      call<LogDetail>("get_log", { sessionId, id }),
    getInterceptorContent: (sessionId: number | null, id: number, executionId: number) =>
      call<InterceptorContent>("get_interceptor_content", { sessionId, id, executionId }),
    getInterceptorSnapshot: (sessionId: number | null, id: number, executionId: number) =>
      call<InterceptorSnapshotPayload>("get_interceptor_snapshot", {
        sessionId,
        id,
        executionId,
      }),
    listBreakpoints: (query: BreakpointQuery) => call("list_breakpoints", { query }),
    getBreakpoint: (id: number) => call("get_breakpoint", { id }),
    extendBreakpoint: (id: number, request: ExtendBreakpointRequest) =>
      call("extend_breakpoint", { id, request }),
    releaseBreakpoint: (id: number) => call("release_breakpoint", { id }),
    executeBreakpointScript: (id: number, request: ExecuteTemporaryScriptRequest) =>
      call("execute_breakpoint_script", { id, request }),
    getSessionView: (sessionId: number | null) =>
      call("get_session_view", { sessionId }),
    replaceSessionView: (sessionId: number | null, request: ReplaceSessionViewRequest) =>
      call("replace_session_view", { sessionId, request }),
    replaceSessionFilter: (sessionId, filter) =>
      call("replace_session_filter", { sessionId, filter }),

    listColumnScripts: () => call("list_column_scripts"),
    createColumnScript: (request: ScriptRequest) => call("create_column_script", { request }),
    getColumnScript: (name: string) => call("get_column_script", { name }),
    updateColumnScript: (name: string, request: UpdateScriptRequest) =>
      call("update_column_script", { name, request }),
    deleteColumnScript: (name: string) => call("delete_column_script", { name }),

    listFilterScripts: () => call("list_filter_scripts"),
    createFilterScript: (request: ScriptRequest) => call("create_filter_script", { request }),
    getFilterScript: (name: string) => call("get_filter_script", { name }),
    updateFilterScript: (name: string, request: UpdateScriptRequest) =>
      call("update_filter_script", { name, request }),
    deleteFilterScript: (name: string) => call("delete_filter_script", { name }),
    debugFilterScript: (name: string, request: DebugFilterScriptRequest) =>
      call("debug_filter_script", { name, request }),

    listRoutingScripts: () => call("list_routing_scripts"),
    createRoutingScript: (request: ScriptRequest) =>
      call("create_routing_script", { request }),
    getRoutingScript: (name: string) => call("get_routing_script", { name }),
    updateRoutingScript: (name: string, request: UpdateScriptRequest) =>
      call("update_routing_script", { name, request }),
    deleteRoutingScript: (name: string) => call("delete_routing_script", { name }),
    getRoutingSelection: () => call("get_routing_selection"),
    replaceRoutingSelection: (selection: RoutingSelection) =>
      call("replace_routing_selection", { selection }),

    listInterceptors: (kind: InterceptorKind) => call("list_interceptors", { kind }),
    createInterceptor: (request: InterceptorCreateRequest) =>
      call("create_interceptor", { request }),
    getInterceptor: (kind: InterceptorKind, name: string) =>
      call("get_interceptor", { kind, name }),
    updateInterceptor: (kind: InterceptorKind, name: string, request: InterceptorUpdateRequest) =>
      call("update_interceptor", { kind, name, request }),
    deleteInterceptor: (kind: InterceptorKind, name: string) =>
      call("delete_interceptor", { kind, name }),
    getSessionInterceptors: (sessionId: number | null) =>
      call("get_session_interceptors", { sessionId }),
    replaceSessionInterceptors: (
      sessionId: number | null,
      request: ReplaceSessionInterceptorsRequest,
    ) => call("replace_session_interceptors", { sessionId, request }),

    getCertificate: () => call("get_certificate"),

    getSystemLogs: (query: SystemLogsQuery) => call("get_system_logs", { query }),
    clearSystemLogs: () => call("clear_system_logs"),

    getBypassEntries: (query: BypassQuery) => call("get_bypass_entries", { query }),
    deleteBypassEntry: (id: number) => call("delete_bypass_entry", { id }),
    deleteBypassEntries: (ids: number[]) => call("delete_bypass_entries", { ids }),
    clearBypassEntries: () => call("clear_bypass_entries"),

    getHttpServiceError: () => call("get_http_service_error"),
    getHttpServiceStatus: () => call<HttpServiceStatus>("get_http_service_status"),
    getHttpPermissionCatalog: () =>
      call<ApiActionView[]>("get_http_permission_catalog"),
    listHttpPermissionIdentities: () =>
      call<PermissionIdentitySummary[]>("list_http_permission_identities"),
    getHttpIdentityPermissions: (id: string) =>
      call<IdentityPermissions>("get_http_identity_permissions", { id }),
    replaceHttpIdentityPermissions: (id: string, permissions: PermissionEntry[]) =>
      call<IdentityPermissions>("replace_http_identity_permissions", { id, permissions }),
    createHttpApiKey: (name: string) =>
      call<CreatedApiKey>("create_http_api_key", { name }),
    deleteHttpApiKey: (id: string) => call("delete_http_api_key", { id }),
    listHttpApprovals: () => call<PendingApproval[]>("list_http_approvals"),
    resolveHttpApproval: (id: number, request: ResolveApprovalRequest) =>
      call("resolve_http_approval", { id, request }),

  };
}
