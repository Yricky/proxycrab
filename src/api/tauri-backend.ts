import { invoke } from "@tauri-apps/api/core";
import type { Backend } from "./backend";
import type {
  ActiveSession,
  AppConfig,
  CreateAgentsPresetRequest,
  BreakpointQuery,
  BypassQuery,
  CreateSessionRequest,
  DebugFilterScriptRequest,
  ExecuteTemporaryScriptRequest,
  ExtendBreakpointRequest,
  InterceptorCreateRequest,
  InterceptorKind,
  InterceptorUpdateRequest,
  LogDetail,
  LogIdsRequest,
  LogViewsRequest,
  ManagerError,
  ReplaceSessionInterceptorsRequest,
  ReplaceSessionViewRequest,
  RoutingSelection,
  ScriptRequest,
  SystemLogsQuery,
  UpdateScriptRequest,
  UpdateAgentsPresetRequest,
  UpdateSessionRequest,
} from "./types";

/** Error thrown by every Backend call; carries the backend's stable code. */
export class BackendError extends Error {
  readonly code: string;

  constructor(error: ManagerError) {
    super(error.message);
    this.name = "BackendError";
    this.code = error.code;
  }
}

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
    getWorkspace: () => call("get_workspace"),
    setWorkspaceForNextStart: (path) => call("set_workspace_for_next_start", { path }),
    getConfig: () => call("get_config"),
    replaceConfig: (config: AppConfig) => call("replace_config", { config }),
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
    createSession: (request: CreateSessionRequest) => call("create_session", { request }),
    updateSession: (id: number, request: UpdateSessionRequest) =>
      call("update_session", { id, request }),
    deleteSession: (id: number) => call("delete_session", { id }),
    getActiveSession: () => call("get_active_session"),
    replaceActiveSession: (active: ActiveSession) =>
      call("replace_active_session", { active }),

    getLogIds: (request: LogIdsRequest) => call("get_log_ids", { request }),
    getLogViews: (request: LogViewsRequest) => call("get_log_views", { request }),
    getLog: (sessionId: number | null, id: number) =>
      call<LogDetail>("get_log", { sessionId, id }),
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
    regenerateCertificate: () => call("regenerate_certificate"),

    getSystemLogs: (query: SystemLogsQuery) => call("get_system_logs", { query }),
    clearSystemLogs: () => call("clear_system_logs"),

    getBypassEntries: (query: BypassQuery) => call("get_bypass_entries", { query }),
    deleteBypassEntry: (id: number) => call("delete_bypass_entry", { id }),
    deleteBypassEntries: (ids: number[]) => call("delete_bypass_entries", { ids }),
    clearBypassEntries: () => call("clear_bypass_entries"),

    getHttpServiceError: () => call("get_http_service_error"),

    getProxyCrabSkillInstallInfo: (parent: string) =>
      call("get_proxycrab_skill_install_info", { parent }),
    installProxyCrabSkill: (parent: string, overwrite: boolean) =>
      call("install_proxycrab_skill", { parent, overwrite }),
  };
}
