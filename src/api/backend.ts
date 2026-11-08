import type { FileWriter } from "./save-file";
import type { ActiveSession } from "./generated/ActiveSession";
import type { AgentsPresetState } from "./generated/AgentsPresetState";
import type { AppConfig } from "./generated/AppConfig";
import type { BreakpointDetailPayload } from "./generated/BreakpointDetailPayload";
import type { BreakpointQuery } from "./generated/BreakpointQuery";
import type { BreakpointSummary } from "./generated/BreakpointSummary";
import type { BypassPage } from "./generated/BypassPage";
import type { BypassQuery } from "./generated/BypassQuery";
import type { CertificateResponse } from "./generated/CertificateResponse";
import type { CreateAgentsPresetRequest } from "./generated/CreateAgentsPresetRequest";
import type { CreateSessionRequest } from "./generated/CreateSessionRequest";
import type { DebugFilterScriptRequest } from "./generated/DebugFilterScriptRequest";
import type { DeleteCount } from "./generated/DeleteCount";
import type { ExecuteTemporaryScriptRequest } from "./generated/ExecuteTemporaryScriptRequest";
import type { ExtendBreakpointRequest } from "./generated/ExtendBreakpointRequest";
import type { InterceptorCreateRequest } from "./generated/InterceptorCreateRequest";
import type { InterceptorScriptContent } from "./generated/InterceptorScriptContent";
import type { InterceptorKind } from "./generated/InterceptorKind";
import type { InterceptorLibraryList } from "./generated/InterceptorLibraryList";
import type { InterceptorSnapshot } from "./generated/InterceptorSnapshot";
import type { InterceptorUpdateRequest } from "./generated/InterceptorUpdateRequest";
import type { HttpServiceStatus } from "./generated/HttpServiceStatus";
import type { HttpApiChange } from "./generated/HttpApiChange";
import type { LogDetail } from "./generated/LogDetail";
import type { LogIdsPayload } from "./generated/LogIdsPayload";
import type { LogIdsRequest } from "./generated/LogIdsRequest";
import type { LogViewsPayload } from "./generated/LogViewsPayload";
import type { LogViewsRequest } from "./generated/LogViewsRequest";
import type { ProxyStatus } from "./generated/ProxyStatus";
import type { ReplaceSessionInterceptorsRequest } from "./generated/ReplaceSessionInterceptorsRequest";
import type { ReplaceSessionViewRequest } from "./generated/ReplaceSessionViewRequest";
import type { ReplayRequestPayload } from "./generated/ReplayRequestPayload";
import type { ReplayResult } from "./generated/ReplayResult";
import type { RoutingSelection } from "./generated/RoutingSelection";
import type { ScriptRequest } from "./generated/ScriptRequest";
import type { SessionFilter } from "./generated/SessionFilter";
import type { SessionMetadata } from "./generated/SessionMetadata";
import type { SessionsPayload } from "./generated/SessionsPayload";
import type { SessionInterceptorsPayload } from "./generated/SessionInterceptorsPayload";
import type { SessionViewPayload } from "./generated/SessionViewPayload";
import type { SystemLogEntry } from "./generated/SystemLogEntry";
import type { SystemLogsQuery } from "./generated/SystemLogsQuery";
import type { UpdateScriptRequest } from "./generated/UpdateScriptRequest";
import type { UpdateAgentsPresetRequest } from "./generated/UpdateAgentsPresetRequest";
import type { UpdateSessionRequest } from "./generated/UpdateSessionRequest";
import type { TemporaryExecutionResult } from "./generated/TemporaryExecutionResult";
import type { ApiActionView } from "./generated/ApiActionView";
import type { AssetMetadata } from "./generated/AssetMetadata";
import type { ClashConfig } from "./generated/ClashConfig";
import type { ClashSubscriptionState } from "./generated/ClashSubscriptionState";
import type { CreatedApiKey } from "./generated/CreatedApiKey";
import type { IdentityPermissions } from "./generated/IdentityPermissions";
import type { PendingApproval } from "./generated/PendingApproval";
import type { PermissionEntry } from "./generated/PermissionEntry";
import type { PermissionIdentitySummary } from "./generated/PermissionIdentitySummary";
import type { ResolveApprovalRequest } from "./generated/ResolveApprovalRequest";
import type { SessionShareState } from "./generated/SessionShareState";
import type { SessionSharesPayload } from "./generated/SessionSharesPayload";
import type { SessionShareConfig } from "./generated/SessionShareConfig";
import type { SkillManagerState } from "./generated/SkillManagerState";
import type { SkillSaveResult } from "./generated/SkillSaveResult";
import type { WorkspacePaths } from "./generated/WorkspacePaths";
import type { SaveFileRequest, Script } from "./types";
import type { BodySide } from "./generated/BodySide";
import type { BodyTarget, LoadedBody } from "./body";

export type Unsubscribe = () => void;

export interface SkillManager {
  getState(): Promise<SkillManagerState>;
  savePaths(paths: string[], deleteRemoved: boolean): Promise<SkillSaveResult>;
  sync(): Promise<SkillManagerState>;
}

export interface HostOperations {
  readonly skillManager: SkillManager;
  switchWorkspace?(): Promise<void>;
  openDevtools?(): Promise<void>;
  replaceConfig(config: AppConfig): Promise<AppConfig>;
  regenerateCertificate(): Promise<CertificateResponse>;
}

/**
 * Backend abstraction: every capability the UI needs from the host process.
 * The Tauri implementation (tauri-backend.ts) maps these onto `invoke` calls;
 * an alternative implementation (e.g. plain HTTP against the management API)
 * can be dropped in without touching any component.
 *
 * 能力位由方法是否存在表达：可选方法未实现时，UI 不展示对应入口，而不是探测
 * 一个独立的能力配置。分享后端只暴露只读接口，因此没有 replay、Session 写入、
 * 审批等方法。
 */
export interface Backend {
  readonly clashSubscription?: {
    getState(): Promise<ClashSubscriptionState>;
    enable(server: string): Promise<ClashSubscriptionState>;
    disable(): Promise<ClashSubscriptionState>;
  };
  readonly clashConfig?: {
    get(): Promise<ClashConfig>;
    save(yaml: string): Promise<ClashConfig>;
    reset(): Promise<ClashConfig>;
  };
  readonly host?: HostOperations;

  fetchBody(target: BodyTarget, side: BodySide, maxSize?: number): Promise<LoadedBody>;
  downloadBody?(target: BodyTarget, side: BodySide, filename: string): Promise<boolean>;
  subscribeChanges(handler: (change: HttpApiChange) => void): Promise<Unsubscribe>;
  subscribeApprovalChanges(handler: (count: number) => void): Promise<Unsubscribe>;
  openExternal(url: string): Promise<void>;
  /**
   * 创建本地文件 Writer：Tauri 弹保存对话框（用户取消时返回 null），
   * 浏览器在 markAsFinish 时触发下载；markAsError 放弃保存。
   */
  saveFile(request: SaveFileRequest): Promise<FileWriter | null>;

  // workspace & config
  getWorkspace(): Promise<WorkspacePaths>;
  getConfig(): Promise<AppConfig>;
  getAgentsPresets(): Promise<AgentsPresetState>;
  createAgentsPreset(request: CreateAgentsPresetRequest): Promise<AgentsPresetState>;
  updateAgentsPreset(id: string, request: UpdateAgentsPresetRequest): Promise<AgentsPresetState>;
  activateAgentsPreset(id: string): Promise<AgentsPresetState>;
  deleteAgentsPreset(id: string): Promise<AgentsPresetState>;
  reimportDefaultAgentsPresets(): Promise<AgentsPresetState>;

  // proxy lifecycle
  getProxyStatus(): Promise<ProxyStatus>;
  startProxy(): Promise<ProxyStatus>;
  stopProxy(): Promise<ProxyStatus>;
  listLocalIps?(): Promise<string[]>;

  importBundle?(
    file: File,
    acceptedIntentIds: string[],
  ): Promise<import("./generated/ImportResult").ImportResult>;
  resolveBundle?(
    id: string,
    resolutions: import("./generated/Resolution").Resolution[],
  ): Promise<import("./generated/ImportResult").ImportResult>;
  downloadSessionConfig?(): Promise<void>;

  // sessions
  listSessions(): Promise<SessionsPayload>;
  listArchivedSessions(): Promise<SessionMetadata[]>;
  createSession(request: CreateSessionRequest): Promise<SessionMetadata>;
  updateSession(id: number, request: UpdateSessionRequest): Promise<SessionMetadata>;
  archiveSession(id: number): Promise<SessionMetadata>;
  restoreSession(id: number): Promise<SessionMetadata>;
  deleteArchivedSession(id: number): Promise<void>;
  replaceActiveSession(active: ActiveSession): Promise<ActiveSession>;
  listSessionShares?(): Promise<SessionSharesPayload>;
  getSessionShare(sessionId: number): Promise<SessionShareState>;
  enableSessionShare(
    sessionId: number,
    sessionShareConfig?: SessionShareConfig,
  ): Promise<SessionShareState>;
  disableSessionShare(sessionId: number): Promise<SessionShareState>;

  getWebSocketMessages?(
    sessionId: number,
    id: number,
    side: BodySide,
    afterId: number,
    limit?: number,
  ): Promise<import("./generated/WebSocketPage").WebSocketPage>;
  fetchWebSocketMessageBody?(
    sessionId: number,
    id: number,
    side: BodySide,
    messageId: number,
  ): Promise<LoadedBody>;

  // capture logs
  getLogIds(request: LogIdsRequest): Promise<LogIdsPayload>;
  getLogViews(request: LogViewsRequest): Promise<LogViewsPayload>;
  validateFilterRegex(pattern: string): Promise<string | null>;
  getLog(sessionId: number | null, id: number): Promise<LogDetail>;
  replay?(sessionId: number, request: ReplayRequestPayload): Promise<ReplayResult>;
  listAssets(): Promise<AssetMetadata[]>;
  /** 读取资源原始字节用于详情预览；后端未暴露时资源详情只展示元数据。 */
  readAsset?(id: string): Promise<Uint8Array<ArrayBuffer>>;
  uploadAsset?(id: string, file: Blob): Promise<AssetMetadata>;
  getInterceptorContent(
    sessionId: number | null,
    id: number,
    executionId: number,
  ): Promise<InterceptorScriptContent>;
  getInterceptorSnapshot(
    sessionId: number | null,
    id: number,
    executionId: number,
  ): Promise<InterceptorSnapshot>;
  listBreakpoints(query: BreakpointQuery): Promise<BreakpointSummary[]>;
  getBreakpoint(id: number): Promise<BreakpointDetailPayload>;
  extendBreakpoint(id: number, request: ExtendBreakpointRequest): Promise<BreakpointSummary>;
  releaseBreakpoint(id: number): Promise<void>;
  executeBreakpointScript(
    id: number,
    request: ExecuteTemporaryScriptRequest,
  ): Promise<TemporaryExecutionResult>;
  getSessionView(sessionId: number | null): Promise<SessionViewPayload>;
  replaceSessionView?(
    sessionId: number | null,
    request: ReplaceSessionViewRequest,
  ): Promise<SessionViewPayload>;
  replaceSessionFilter?(sessionId: number, filter: SessionFilter): Promise<SessionViewPayload>;

  // Owner-managed redaction library and identity/share bindings.
  listRedactionScripts?(): Promise<Script[]>;
  getRedactionScript?(name: string): Promise<Script>;
  createRedactionScript?(request: ScriptRequest): Promise<void>;
  updateRedactionScript?(name: string, request: UpdateScriptRequest): Promise<void>;
  deleteRedactionScript?(name: string): Promise<void>;
  setHttpIdentityRedaction?(
    id: string,
    redactionScript: string | null,
  ): Promise<IdentityPermissions>;

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
  listHttpApprovals?(): Promise<PendingApproval[]>;
  resolveHttpApproval?(id: number, request: ResolveApprovalRequest): Promise<void>;
}
