import { createFileWriter } from "./save-file";
import { invoke } from "@tauri-apps/api/core";
import type { InvokeArgs, InvokeOptions } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { Backend } from "./backend";
import { BackendError } from "./backend-error";
import { BodyFetchError, DEFAULT_BODY_MAX_SIZE, UI_BODY_MAX_SIZE } from "./body";
import type {
  ActiveSession,
  ApiActionView,
  AppConfig,
  AssetMetadata,
  CreatedApiKey,
  CreateAgentsPresetRequest,
  BreakpointQuery,
  BypassQuery,
  ClashConfig,
  CreateSessionRequest,
  DebugFilterScriptRequest,
  ExecuteTemporaryScriptRequest,
  ExtendBreakpointRequest,
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
  ReplayRequestPayload,
  ReplayResult,
  ResolveApprovalRequest,
  RoutingSelection,
  SaveFileRequest,
  SessionShareState,
  SessionSharesPayload,
  SessionShareConfig,
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

async function call<T>(command: string, args?: InvokeArgs, options?: InvokeOptions): Promise<T> {
  try {
    return await invoke<T>(command, args, options);
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
    host: {
      skillManager: {
        getState: () => call("get_proxycrab_skill_manager_state"),
        savePaths: (paths: string[], deleteRemoved: boolean) =>
          call("save_proxycrab_skill_paths", { paths, deleteRemoved }),
        sync: () => call("sync_proxycrab_skills"),
      },
      switchWorkspace: () => call("switch_workspace"),
      replaceConfig: (config: AppConfig) => call("replace_config", { config }),
      regenerateCertificate: () => call("regenerate_certificate"),
    },
    fetchBody: async (target, side, maxSize = DEFAULT_BODY_MAX_SIZE) => {
      if (!Number.isSafeInteger(maxSize) || maxSize <= 0 || maxSize > UI_BODY_MAX_SIZE) {
        throw new BodyFetchError({ code: "bad_request", message: "无效的 Body 大小限制" });
      }
      try {
        const result = await invoke<{ bytes: number[]; contentType: string }>("fetch_body", {
          target,
          side,
          maxSize,
        });
        const bytes = new Uint8Array(result.bytes);
        return { bytes, decodedSize: bytes.byteLength, contentType: result.contentType };
      } catch (error) {
        throw new BodyFetchError(
          isManagerError(error) ? error : { code: "invoke_error", message: String(error) },
        );
      }
    },
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
    saveFile: async ({ filename }: SaveFileRequest) => {
      const handle = await call<string | null>("save_file_open", { filename });
      if (handle === null) return null;
      return createFileWriter({
        async append(data) {
          for (let offset = 0; offset < data.length; offset += 256 * 1024) {
            await call("save_file_append", data.subarray(offset, offset + 256 * 1024), {
              headers: { "x-proxycrab-writer": handle },
            });
          }
        },
        markAsFinish: () => call<void>("save_file_finish", { handle }),
        markAsError: () => call<void>("save_file_abort", { handle }),
      });
    },
    downloadBody: (target, side, filename) => call("download_body", { target, side, filename }),

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
    replaceActiveSession: (active: ActiveSession) => call("replace_active_session", { active }),
    listSessionShares: () => call<SessionSharesPayload>("list_session_shares"),
    getSessionShare: (sessionId: number) =>
      call<SessionShareState>("get_session_share", { sessionId }),
    enableSessionShare: (sessionId: number, sessionShareConfig?: SessionShareConfig) =>
      call<SessionShareState>("enable_session_share", {
        sessionId,
        sessionShareConfig: sessionShareConfig && {
          ...(sessionShareConfig.redactionScript === undefined
            ? {}
            : { redaction_script: sessionShareConfig.redactionScript }),
          persistent: sessionShareConfig.persistent,
        },
      }),
    disableSessionShare: (sessionId: number) =>
      call<SessionShareState>("disable_session_share", { sessionId }),

    getLogIds: (request: LogIdsRequest) => call("get_log_ids", { request }),
    getLogViews: (request: LogViewsRequest) => call("get_log_views", { request }),
    validateFilterRegex: (pattern: string) => call("validate_filter_regex", { pattern }),
    getWebSocketMessages: (sessionId, id, side, afterId, limit = 500) =>
      call("get_websocket_messages", { sessionId, id, side, afterId, limit }),
    fetchWebSocketMessageBody: async (sessionId, id, side, messageId) => {
      const result = await call<{ bytes: number[]; contentType: string }>(
        "fetch_websocket_message_body",
        { sessionId, id, side, messageId },
      );
      const bytes = new Uint8Array(result.bytes);
      return { bytes, decodedSize: bytes.length, contentType: result.contentType };
    },
    getLog: (sessionId: number | null, id: number) => call<LogDetail>("get_log", { sessionId, id }),
    replay: (sessionId: number, request: ReplayRequestPayload) =>
      call<ReplayResult>("replay", { sessionId, request }),
    listAssets: () => call<AssetMetadata[]>("list_assets"),
    uploadAsset: async (id, file) =>
      call<AssetMetadata>("upload_asset", await file.arrayBuffer(), {
        headers: {
          "x-asset-id": id,
          "x-asset-content-type": file.type || "application/octet-stream",
        },
      }),
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
    getSessionView: (sessionId: number | null) => call("get_session_view", { sessionId }),
    replaceSessionView: (sessionId: number | null, request: ReplaceSessionViewRequest) =>
      call("replace_session_view", { sessionId, request }),
    replaceSessionFilter: (sessionId, filter) =>
      call("replace_session_filter", { sessionId, filter }),

    listRedactionScripts: () => call("list_redaction_scripts"),
    getRedactionScript: (name) => call("get_redaction_script", { name }),
    createRedactionScript: (request) => call("create_redaction_script", { request }),
    updateRedactionScript: (name, request) => call("update_redaction_script", { name, request }),
    deleteRedactionScript: (name) => call("delete_redaction_script", { name }),
    setHttpIdentityRedaction: (id, redactionScript) =>
      call("set_http_identity_redaction", { id, redactionScript }),

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
    createRoutingScript: (request: ScriptRequest) => call("create_routing_script", { request }),
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
    clashSubscription: {
      getState: () => call("get_clash_subscription"),
      enable: (server) => call("enable_clash_subscription", { request: { server } }),
      disable: () => call("disable_clash_subscription"),
    },
    clashConfig: {
      get: () => call<ClashConfig>("get_clash_config"),
      save: (yaml) => call<ClashConfig>("save_clash_config", { request: { yaml } }),
      reset: () => call<ClashConfig>("reset_clash_config"),
    },

    getSystemLogs: (query: SystemLogsQuery) => call("get_system_logs", { query }),
    clearSystemLogs: () => call("clear_system_logs"),

    getBypassEntries: (query: BypassQuery) => call("get_bypass_entries", { query }),
    deleteBypassEntry: (id: number) => call("delete_bypass_entry", { id }),
    deleteBypassEntries: (ids: number[]) => call("delete_bypass_entries", { ids }),
    clearBypassEntries: () => call("clear_bypass_entries"),

    getHttpServiceError: () => call("get_http_service_error"),
    getHttpServiceStatus: () => call<HttpServiceStatus>("get_http_service_status"),
    getHttpPermissionCatalog: () => call<ApiActionView[]>("get_http_permission_catalog"),
    listHttpPermissionIdentities: () =>
      call<PermissionIdentitySummary[]>("list_http_permission_identities"),
    getHttpIdentityPermissions: (id: string) =>
      call<IdentityPermissions>("get_http_identity_permissions", { id }),
    replaceHttpIdentityPermissions: (id: string, permissions: PermissionEntry[]) =>
      call<IdentityPermissions>("replace_http_identity_permissions", { id, permissions }),
    createHttpApiKey: (name: string) => call<CreatedApiKey>("create_http_api_key", { name }),
    deleteHttpApiKey: (id: string) => call("delete_http_api_key", { id }),
    listHttpApprovals: () => call<PendingApproval[]>("list_http_approvals"),
    resolveHttpApproval: (id: number, request: ResolveApprovalRequest) =>
      call("resolve_http_approval", { id, request }),
  };
}
