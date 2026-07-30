import type {
  AppConfig,
  CertificateResponse,
  CreateSessionRequest,
  DebugFilterScriptRequest,
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
  Script,
  ScriptRequest,
  SessionMetadata,
  SessionInterceptorsPayload,
  SessionViewPayload,
  SystemLogEntry,
  SystemLogsQuery,
  SkillInstallInfo,
  UpdateScriptRequest,
  UpdateSessionRequest,
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

  // proxy lifecycle
  getProxyStatus(): Promise<ProxyStatus>;
  startProxy(): Promise<ProxyStatus>;
  stopProxy(): Promise<ProxyStatus>;

  // sessions
  listSessions(): Promise<SessionMetadata[]>;
  createSession(request: CreateSessionRequest): Promise<SessionMetadata>;
  updateSession(id: number, request: UpdateSessionRequest): Promise<SessionMetadata>;
  deleteSession(id: number): Promise<void>;
  activateSession(id: number): Promise<SessionMetadata>;

  // capture logs
  getLogIds(request: LogIdsRequest): Promise<LogIdsPayload>;
  getLogViews(request: LogViewsRequest): Promise<LogViewsPayload>;
  getLog(sessionId: number | null, id: number): Promise<LogDetail>;
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

  // management HTTP service status
  getHttpServiceError(): Promise<string | null>;

  // bundled Agent skill
  getProxyCrabSkillInstallInfo(parent: string): Promise<SkillInstallInfo>;
  installProxyCrabSkill(parent: string, overwrite: boolean): Promise<SkillInstallInfo>;
}
