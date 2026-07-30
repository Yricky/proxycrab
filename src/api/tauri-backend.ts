import { invoke } from "@tauri-apps/api/core";
import type { Backend } from "./backend";
import type {
  AppConfig,
  CreateSessionRequest,
  InterceptorCreateRequest,
  InterceptorKind,
  InterceptorUpdateRequest,
  LogDetail,
  LogIdsRequest,
  LogViewsRequest,
  ManagerError,
  ReplaceSessionViewRequest,
  ScriptRequest,
  SetInterceptorOrderRequest,
  SystemLogsQuery,
  UpdateScriptRequest,
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

    getProxyStatus: () => call("get_proxy_status"),
    startProxy: () => call("start_proxy"),
    stopProxy: () => call("stop_proxy"),

    listSessions: () => call("list_sessions"),
    createSession: (request: CreateSessionRequest) => call("create_session", { request }),
    updateSession: (id: number, request: UpdateSessionRequest) =>
      call("update_session", { id, request }),
    deleteSession: (id: number) => call("delete_session", { id }),
    activateSession: (id: number) => call("activate_session", { id }),

    getLogIds: (request: LogIdsRequest) => call("get_log_ids", { request }),
    getLogViews: (request: LogViewsRequest) => call("get_log_views", { request }),
    getLog: (sessionId: number | null, id: number) =>
      call<LogDetail>("get_log", { sessionId, id }),
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

    listInterceptors: (kind: InterceptorKind) => call("list_interceptors", { kind }),
    createInterceptor: (request: InterceptorCreateRequest) =>
      call("create_interceptor", { request }),
    getInterceptor: (kind: InterceptorKind, name: string) =>
      call("get_interceptor", { kind, name }),
    updateInterceptor: (kind: InterceptorKind, name: string, request: InterceptorUpdateRequest) =>
      call("update_interceptor", { kind, name, request }),
    deleteInterceptor: (kind: InterceptorKind, name: string) =>
      call("delete_interceptor", { kind, name }),
    setInterceptorEnabled: (kind: InterceptorKind, name: string, enabled: boolean) =>
      call("set_interceptor_enabled", { kind, name, enabled }),
    setInterceptorOrder: (request: SetInterceptorOrderRequest) =>
      call("set_interceptor_order", { request }),

    getFilterHistory: () => call("get_filter_history"),
    addFilterHistory: (script: string) => call("add_filter_history", { script }),
    removeFilterHistory: (script: string | null) =>
      call("remove_filter_history", { script }),

    getCertificate: () => call("get_certificate"),
    regenerateCertificate: () => call("regenerate_certificate"),

    getSystemLogs: (query: SystemLogsQuery) => call("get_system_logs", { query }),
    clearSystemLogs: () => call("clear_system_logs"),

    getHttpServiceError: () => call("get_http_service_error"),
  };
}
