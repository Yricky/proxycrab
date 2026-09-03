import type {
  ActiveSession,
  ApiActionView,
  AgentsPresetState,
  AppConfig,
  BreakpointDetailPayload,
  BreakpointQuery,
  BreakpointSummary,
  BypassPage,
  BypassQuery,
  CertificateResponse,
  CreatedApiKey,
  CreateAgentsPresetRequest,
  CreateSessionRequest,
  DebugFilterScriptRequest,
  DeleteCount,
  ExecuteTemporaryScriptRequest,
  ExtendBreakpointRequest,
  InterceptorCreateRequest,
  InterceptorKind,
  InterceptorLibraryList,
  InterceptorUpdateRequest,
  HttpServiceStatus,
  HttpApiChange,
  IdentityPermissions,
  LogDetail,
  LogIdsPayload,
  LogIdsRequest,
  LogViewsPayload,
  LogViewsRequest,
  ProxyStatus,
  PendingApproval,
  PermissionEntry,
  PermissionIdentitySummary,
  ReplaceSessionInterceptorsRequest,
  ReplaceSessionViewRequest,
  ResolveApprovalRequest,
  RoutingSelection,
  Script,
  ScriptRequest,
  SessionFilter,
  SessionMetadata,
  SessionShareState,
  SessionInterceptorsPayload,
  SessionViewPayload,
  SystemLogEntry,
  SystemLogsQuery,
  SkillManagerState,
  SkillSaveResult,
  UpdateScriptRequest,
  UpdateAgentsPresetRequest,
  UpdateSessionRequest,
  TemporaryExecutionResult,
  WorkspacePaths,
} from "./types";
import type { BodySide, BodyTarget, LoadedBody } from "./body";

export interface BackendCapabilities {
  target: "tauri" | "cli" | "share";
  approvals: boolean;
  permissionModes: Array<"allow" | "approval" | "deny">;
  readonly: boolean;
}

export type Unsubscribe = () => void;

export interface SkillManager {
  getState(): Promise<SkillManagerState>;
  savePaths(paths: string[], deleteRemoved: boolean): Promise<SkillSaveResult>;
  sync(): Promise<SkillManagerState>;
}

export interface HostOperations {
  readonly skillManager: SkillManager;
  setWorkspaceForNextStart(path: string): Promise<WorkspacePaths>;
  replaceConfig(config: AppConfig): Promise<AppConfig>;
  regenerateCertificate(): Promise<CertificateResponse>;
}

/**
 * Backend abstraction: every capability the UI needs from the host process.
 * The Tauri implementation (tauri-backend.ts) maps these onto `invoke` calls;
 * an alternative implementation (e.g. plain HTTP against the management API)
 * can be dropped in without touching any component.
 */
export interface Backend {
  readonly capabilities: BackendCapabilities;
  readonly host?: HostOperations;

  fetchBody(
    target: BodyTarget,
    side: BodySide,
    maxSize?: number,
  ): Promise<LoadedBody>;
  subscribeChanges(handler: (change: HttpApiChange) => void): Promise<Unsubscribe>;
  subscribeApprovalChanges(handler: (count: number) => void): Promise<Unsubscribe>;
  openExternal(url: string): Promise<void>;

  // workspace & config
  getWorkspace(): Promise<WorkspacePaths>;
  getConfig(): Promise<AppConfig>;
  getAgentsPresets(): Promise<AgentsPresetState>;
  createAgentsPreset(request: CreateAgentsPresetRequest): Promise<AgentsPresetState>;
  updateAgentsPreset(
    id: string,
    request: UpdateAgentsPresetRequest,
  ): Promise<AgentsPresetState>;
  activateAgentsPreset(id: string): Promise<AgentsPresetState>;
  deleteAgentsPreset(id: string): Promise<AgentsPresetState>;
  reimportDefaultAgentsPresets(): Promise<AgentsPresetState>;

  // proxy lifecycle
  getProxyStatus(): Promise<ProxyStatus>;
  startProxy(): Promise<ProxyStatus>;
  stopProxy(): Promise<ProxyStatus>;
  listLocalIps(): Promise<string[]>;

  // sessions
  listSessions(): Promise<SessionMetadata[]>;
  listArchivedSessions(): Promise<SessionMetadata[]>;
  createSession(request: CreateSessionRequest): Promise<SessionMetadata>;
  updateSession(id: number, request: UpdateSessionRequest): Promise<SessionMetadata>;
  archiveSession(id: number): Promise<SessionMetadata>;
  restoreSession(id: number): Promise<SessionMetadata>;
  deleteArchivedSession(id: number): Promise<void>;
  getActiveSession(): Promise<ActiveSession>;
  replaceActiveSession(active: ActiveSession): Promise<ActiveSession>;
  getSessionShare(sessionId: number): Promise<SessionShareState>;
  enableSessionShare(sessionId: number): Promise<SessionShareState>;
  disableSessionShare(sessionId: number): Promise<SessionShareState>;

  // capture logs
  getLogIds(request: LogIdsRequest): Promise<LogIdsPayload>;
  getLogViews(request: LogViewsRequest): Promise<LogViewsPayload>;
  validateFilterRegex(pattern: string): Promise<string | null>;
  getLog(sessionId: number | null, id: number): Promise<LogDetail>;
  listBreakpoints(query: BreakpointQuery): Promise<BreakpointSummary[]>;
  getBreakpoint(id: number): Promise<BreakpointDetailPayload>;
  extendBreakpoint(
    id: number,
    request: ExtendBreakpointRequest,
  ): Promise<BreakpointSummary>;
  releaseBreakpoint(id: number): Promise<void>;
  executeBreakpointScript(
    id: number,
    request: ExecuteTemporaryScriptRequest,
  ): Promise<TemporaryExecutionResult>;
  getSessionView(sessionId: number | null): Promise<SessionViewPayload>;
  replaceSessionView(
    sessionId: number | null,
    request: ReplaceSessionViewRequest,
  ): Promise<SessionViewPayload>;
  replaceSessionFilter(
    sessionId: number,
    filter: SessionFilter,
  ): Promise<SessionViewPayload>;

  // column scripts
  listColumnScripts(): Promise<Script[]>;
  createColumnScript(request: ScriptRequest): Promise<void>;
  getColumnScript(name: string): Promise<Script>;
  updateColumnScript(name: string, request: UpdateScriptRequest): Promise<void>;
  deleteColumnScript(name: string): Promise<void>;

  // filter scripts
  listFilterScripts(): Promise<Script[]>;
  createFilterScript(request: ScriptRequest): Promise<void>;
  getFilterScript(name: string): Promise<Script>;
  updateFilterScript(name: string, request: UpdateScriptRequest): Promise<void>;
  deleteFilterScript(name: string): Promise<void>;
  debugFilterScript(name: string, request: DebugFilterScriptRequest): Promise<boolean>;

  // routing scripts
  listRoutingScripts(): Promise<Script[]>;
  createRoutingScript(request: ScriptRequest): Promise<void>;
  getRoutingScript(name: string): Promise<Script>;
  updateRoutingScript(name: string, request: UpdateScriptRequest): Promise<void>;
  deleteRoutingScript(name: string): Promise<void>;
  getRoutingSelection(): Promise<RoutingSelection>;
  replaceRoutingSelection(selection: RoutingSelection): Promise<RoutingSelection>;

  // interceptors
  listInterceptors(kind: InterceptorKind): Promise<InterceptorLibraryList>;
  createInterceptor(request: InterceptorCreateRequest): Promise<void>;
  getInterceptor(kind: InterceptorKind, name: string): Promise<Script>;
  updateInterceptor(
    kind: InterceptorKind,
    name: string,
    request: InterceptorUpdateRequest,
  ): Promise<void>;
  deleteInterceptor(kind: InterceptorKind, name: string): Promise<void>;
  getSessionInterceptors(sessionId: number | null): Promise<SessionInterceptorsPayload>;
  replaceSessionInterceptors(
    sessionId: number | null,
    request: ReplaceSessionInterceptorsRequest,
  ): Promise<SessionInterceptorsPayload>;

  // certificate
  getCertificate(): Promise<CertificateResponse>;

  // system logs
  getSystemLogs(query: SystemLogsQuery): Promise<SystemLogEntry[]>;
  clearSystemLogs(): Promise<void>;

  // bypass traffic
  getBypassEntries(query: BypassQuery): Promise<BypassPage>;
  deleteBypassEntry(id: number): Promise<void>;
  deleteBypassEntries(ids: number[]): Promise<DeleteCount>;
  clearBypassEntries(): Promise<DeleteCount>;

  // management HTTP service status
  getHttpServiceError(): Promise<string | null>;
  getHttpServiceStatus(): Promise<HttpServiceStatus>;
  getHttpPermissionCatalog(): Promise<ApiActionView[]>;
  listHttpPermissionIdentities(): Promise<PermissionIdentitySummary[]>;
  getHttpIdentityPermissions(id: string): Promise<IdentityPermissions>;
  replaceHttpIdentityPermissions(
    id: string,
    permissions: PermissionEntry[],
  ): Promise<IdentityPermissions>;
  createHttpApiKey(name: string): Promise<CreatedApiKey>;
  deleteHttpApiKey(id: string): Promise<void>;
  listHttpApprovals(): Promise<PendingApproval[]>;
  resolveHttpApproval(id: number, request: ResolveApprovalRequest): Promise<void>;

}
