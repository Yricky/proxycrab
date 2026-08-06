import type {
  ActiveSession,
  AgentsPresetState,
  AppConfig,
  BreakpointDetailPayload,
  BreakpointQuery,
  BreakpointSummary,
  BypassPage,
  BypassQuery,
  CertificateResponse,
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
  LogDetail,
  LogIdsPayload,
  LogIdsRequest,
  LogViewsPayload,
  LogViewsRequest,
  ProxyStatus,
  ReplaceSessionInterceptorsRequest,
  ReplaceSessionViewRequest,
  RoutingSelection,
  Script,
  ScriptRequest,
  SessionMetadata,
  SessionInterceptorsPayload,
  SessionViewPayload,
  SystemLogEntry,
  SystemLogsQuery,
  SkillInstallInfo,
  UpdateScriptRequest,
  UpdateAgentsPresetRequest,
  UpdateSessionRequest,
  TemporaryExecutionResult,
  WorkspacePaths,
} from "./types";

/**
 * Backend abstraction: every capability the UI needs from the host process.
 * The Tauri implementation (tauri-backend.ts) maps these onto `invoke` calls;
 * an alternative implementation (e.g. plain HTTP against the management API)
 * can be dropped in without touching any component.
 */
export interface Backend {
  // workspace & config
  getWorkspace(): Promise<WorkspacePaths>;
  setWorkspaceForNextStart(path: string): Promise<WorkspacePaths>;
  getConfig(): Promise<AppConfig>;
  replaceConfig(config: AppConfig): Promise<AppConfig>;
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

  // capture logs
  getLogIds(request: LogIdsRequest): Promise<LogIdsPayload>;
  getLogViews(request: LogViewsRequest): Promise<LogViewsPayload>;
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
  regenerateCertificate(): Promise<CertificateResponse>;

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

  // bundled Agent skill
  getProxyCrabSkillInstallInfo(parent: string): Promise<SkillInstallInfo>;
  installProxyCrabSkill(parent: string, overwrite: boolean): Promise<SkillInstallInfo>;
}
