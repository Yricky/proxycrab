// Types mirroring crates/proxy-crab-mitm/src/model.rs and crates/proxy-crab-mgr/src/dto.rs.
// Field names match serde's snake_case wire format exactly.

export interface ManagerError {
  code: string;
  message: string;
  actual_size?: number;
  max_size?: number;
}

// ---------- model.rs ----------

export type ColumnKind =
  | "method"
  | "uri"
  | "code"
  | "source"
  | "stage"
  | "created_at"
  | "updated_at"
  | "script";

export type Column =
  | { kind: "method"; width: number }
  | { kind: "uri"; width: number }
  | { kind: "code"; width: number }
  | { kind: "source"; width: number }
  | { kind: "stage"; width: number }
  | { kind: "created_at"; width: number }
  | { kind: "updated_at"; width: number }
  | { kind: "script"; width: number; script_name: string };

export type FilterColumn =
  | { kind: "method" }
  | { kind: "uri" }
  | { kind: "code" }
  | { kind: "source" }
  | { kind: "stage" }
  | { kind: "script"; script_name: string };

export type FilterOption =
  | { kind: "column"; column: FilterColumn; regex: boolean }
  | { kind: "script"; script_name: string };

export interface SessionFilter {
  option: FilterOption | null;
  input: string;
}

export interface AppConfig {
  proxy_host: string;
  proxy_port: number;
  api_port: number;
  routing_script_name: string | null;
  active_session_id: number | null;
}

export interface SessionMetadata {
  id: number;
  name: string;
  created_at: number;
  description: string | null;
}

export interface Script {
  name: string;
  content: string;
}

export interface AgentsPreset {
  id: string;
  name: string;
  content: string;
}

export interface AgentsPresetState {
  active_id: string;
  presets: AgentsPreset[];
}

export type InterceptorKind = "request" | "response";

export interface InterceptorLibraryItem {
  name: string;
  usage_count: number;
}

export interface InterceptorLibraryList {
  kind: InterceptorKind;
  items: InterceptorLibraryItem[];
}

export interface SessionInterceptorItem {
  name: string;
  enabled: boolean;
  valid: boolean;
}

export interface SessionInterceptorsPayload {
  session_id: number;
  request: SessionInterceptorItem[];
  response: SessionInterceptorItem[];
}

export interface ReplaceSessionInterceptorsRequest {
  request: Array<{ name: string; enabled: boolean }>;
  response: Array<{ name: string; enabled: boolean }>;
}

export type CaptureOutcome = "in_progress" | "success" | "failed" | "tunneled";

export type ErrorStage =
  | "connect"
  | "tls_handshake"
  | "request_body"
  | "interceptor"
  | "upstream"
  | "response_body";

export interface CaptureError {
  stage: ErrorStage;
  kind: string;
  message: string;
}

export type BodyPayload =
  | { type: "empty" }
  | { type: "text"; content: string; size: number; path: string | null }
  | { type: "json"; content: unknown; size: number; path: string | null }
  | { type: "binary"; size: number; path: string | null }
  | { type: "large"; size: number; path: string | null };

export type BodySourceType =
  | { type: "original" }
  | { type: "string"; content: string }
  | { type: "asset"; asset_id: string };

export interface RequestInterceptorSnapshot {
  method: string;
  uri: string;
  version: string;
  headers: Record<string, string[]>;
  body: BodySourceType;
}

export interface ResponseInterceptorSnapshot {
  status: number;
  version: string;
  headers: Record<string, string[]>;
  body: BodySourceType;
}

/** Returned by the dedicated interceptor snapshot endpoint. Exactly one side is present. */
export interface InterceptorSnapshotPayload {
  request: RequestInterceptorSnapshot | null;
  response: ResponseInterceptorSnapshot | null;
}

/** Returned by the dedicated interceptor script content endpoint. */
export interface InterceptorContent {
  hash: string;
  content: string;
}

export type Modification =
  | {
      kind: "snapshot";
      request: RequestInterceptorSnapshot | null;
      response: ResponseInterceptorSnapshot | null;
    }
  | { kind: "method_set"; method: string }
  | { kind: "uri_set"; uri: string }
  | { kind: "status_set"; status: number }
  | { kind: "header_append"; name: string; value: string }
  | { kind: "header_set"; name: string; value: string }
  | { kind: "header_remove"; name: string; values: string[] }
  | { kind: "body_replace_string"; content: string }
  | { kind: "body_replace_asset"; asset_id: string }
  | { kind: "tag_set"; key: string; value: string };

/** Log detail omits the snapshot entry; fetch it from the snapshot endpoint instead. */
export type InterceptorModification = Exclude<Modification, { kind: "snapshot" }>;

export type InterceptorExecutionOrigin = "saved" | "temporary";

export type ProxyStatus =
  | { status: "stopped" }
  | { status: "starting" }
  | {
      status: "running";
      host: string;
      port: number;
      started_at: number;
      active_netlog: Record<string, number[]>;
      active_bypass_count?: number;
    }
  | { status: "stopping" }
  | { status: "failed"; message: string };

export interface SystemLogEntry {
  seq: number;
  timestamp: number;
  level: string;
  message: string;
}

export interface WorkspacePaths {
  current_path: string;
  configured_path: string;
}

export type HttpApiResource =
  | "workspace"
  | "config"
  | "proxy"
  | "sessions"
  | "archived_sessions"
  | "active_session"
  | "session_share"
  | "session_har_share"
  | "session_view"
  | "column_scripts"
  | "filter_scripts"
  | "routing_scripts"
  | "routing_selection"
  | "bypass"
  | "interceptors"
  | "session_interceptors"
  | "certificate"
  | "system_logs"
  | "all";

export interface HttpApiChange {
  resources: HttpApiResource[];
  session_id?: number;
}

export interface SkillPathState {
  parent_path: string;
  target_path: string;
  error?: string;
}

export interface SkillManagerState {
  configured: boolean;
  config_error?: string;
  paths: SkillPathState[];
}

export interface SkillRemovalError {
  parent_path: string;
  message: string;
}

export interface SkillSaveResult {
  state: SkillManagerState;
  removal_errors: SkillRemovalError[];
}

// ---------- dto.rs ----------

export interface CreateSessionRequest {
  name?: string | null;
  description?: string | null;
}

export interface UpdateSessionRequest {
  name?: string | null;
  description?: string | null;
}

export interface ActiveSession {
  session_id: number | null;
}

export interface SessionShareState {
  session_id: number;
  enabled: boolean;
  token?: string;
}

export type HarShareScope = "all" | "filtered";

export interface EnableHarShareRequest {
  session_id: number;
  scope: HarShareScope;
  log_ids: number[];
}

export interface HarShareState {
  session_id: number;
  enabled: boolean;
  scope?: HarShareScope;
  log_count: number;
  token?: string;
}

export interface LogIdsRequest {
  session_id?: number | null;
  filter?: SessionFilter | null;
  ids?: number[] | null;
  min_id?: number | null;
  max_id?: number | null;
  limit?: number | null;
}

export interface CreateAgentsPresetRequest {
  name: string;
}

export interface UpdateAgentsPresetRequest {
  name?: string | null;
  content?: string | null;
}

export interface LogIdsPayload {
  matched_ids: number[];
  in_progress_ids: number[];
}

export interface LogViewItem {
  id: number;
  updated_at?: number | null;
}

export interface SessionViewInput {
  columns: Column[];
}

export interface LogViewsRequest {
  session_id?: number | null;
  logs: LogViewItem[];
  view?: SessionViewInput | null;
}

export interface LogViewRow {
  id: number;
  created_at: number;
  updated_at: number;
  outcome: CaptureOutcome;
  cells: string[];
}

export interface LogViewException {
  id: number;
  column_index?: number | null;
  code: string;
  message: string;
}

export interface LogViewsPayload {
  columns: Column[];
  rows: LogViewRow[];
  exceptions: LogViewException[];
}

export interface SessionViewPayload {
  session_id: number;
  columns: Column[];
  filter: SessionFilter;
}

export interface ReplaceSessionViewRequest {
  columns: Column[];
}

export interface HeaderItem {
  name: string;
  value: string;
}

export interface RequestDetail {
  method: string;
  uri: string;
  version: string;
  headers: HeaderItem[];
  tags: Record<string, string>;
  body: BodyPayload;
}

export interface ResponseDetail {
  status: number;
  status_text: string;
  version: string;
  headers: HeaderItem[];
  body: BodyPayload;
}

export interface LogDetail {
  id: number;
  session_id: number;
  created_at: number;
  updated_at: number;
  source_type: string;
  source_addr: string | null;
  stage: string;
  outcome: CaptureOutcome;
  error: CaptureError | null;
  request: RequestDetail;
  response: ResponseDetail | null;
  request_interceptors: InterceptorExecution[];
  response_interceptors: InterceptorExecution[];
}

export interface InterceptorExecution {
  execution_id: number;
  origin: InterceptorExecutionOrigin;
  completed: boolean;
  phase: InterceptorKind;
  position: number;
  name: string;
  script_hash: string;
  has_snapshot: boolean;
  modifications: InterceptorModification[];
  error: string | null;
}

export interface BreakpointSummary {
  id: number;
  session_id: number;
  capture_id: number;
  phase: InterceptorKind;
  position: number;
  interceptor_name: string;
  method: string;
  uri: string;
  created_at: number;
  expires_at: number;
  remaining_ms: number;
}

export interface BreakpointQuery {
  session_id?: number | null;
  phase?: InterceptorKind | null;
  interceptor_name?: string | null;
}

export interface BreakpointDetailPayload {
  breakpoint: BreakpointSummary;
  log: LogDetail;
}

export interface ExtendBreakpointRequest {
  timeout_ms: number;
}

export interface ExecuteTemporaryScriptRequest {
  content: string;
}

export interface TemporaryExecutionResult {
  execution: InterceptorExecution;
  breakpoint: BreakpointSummary;
}

export interface ScriptRequest {
  name: string;
  content?: string;
}

export interface UpdateScriptRequest {
  content: string;
}

export interface DebugFilterScriptRequest {
  session_id?: number | null;
  log_id: number;
  input: string;
}

export interface InterceptorCreateRequest {
  kind: InterceptorKind;
  name: string;
  content?: string;
}

export interface InterceptorUpdateRequest {
  content: string;
}

export interface RoutingSelection {
  name: string | null;
}

export type BypassOutcome = "in_progress" | "success" | "failed";

export interface BypassEntry {
  id: number;
  created_at: number;
  updated_at: number;
  source: string;
  method: string;
  uri: string;
  version: string;
  reason: string;
  outcome: BypassOutcome;
  response_status: number | null;
  error: string | null;
  upload_bytes: number | null;
  download_bytes: number | null;
}

export interface BypassQuery {
  before_id?: number | null;
  limit?: number | null;
}

export interface BypassPage {
  rows: BypassEntry[];
  has_more: boolean;
}

export interface DeleteCount {
  deleted: number;
}

export interface SystemLogsQuery {
  after_seq?: number | null;
  limit?: number | null;
}

export interface CertificateResponse {
  pem: string;
}

export interface HttpServiceStatus {
  running: boolean;
  host: string;
  port: number;
  error: string | null;
}

export type PermissionMode = "allow" | "approval" | "deny";
export type PermissionIdentityKind = "local" | "api_key";

export interface ApiActionView {
  id: string;
  method: string;
  route_template: string;
  default_mode: PermissionMode;
}

export interface PermissionIdentitySummary {
  id: string;
  kind: PermissionIdentityKind;
  name: string;
  prefix: string | null;
  created_at: number | null;
  last_used_at: number | null;
}

export interface PermissionEntry {
  action_id: string;
  mode: PermissionMode;
}

export interface IdentityPermissions {
  identity: PermissionIdentitySummary;
  permissions: PermissionEntry[];
}

export interface CreatedApiKey {
  identity: PermissionIdentitySummary;
  api_key: string;
}

export interface PendingApproval {
  id: number;
  identity: PermissionIdentitySummary;
  action_id: string;
  method: string;
  route_template: string;
  actual_path: string;
  query: string | null;
  source: string | null;
  content_type: string | null;
  content_length: number | null;
  body_preview: string | null;
  body_preview_truncated: boolean;
  created_at: number;
  deadline_at: number;
}

export type ApprovalDecision = "allow" | "deny";

export interface ResolveApprovalRequest {
  decision: ApprovalDecision;
  duration_seconds: number | null;
}

export type ReplayBodyPayload =
  | { type: "text"; text: string; charset?: "utf8" }
  | {
      type: "body_ref";
      session_id: number;
      log_id: number;
      side: "request" | "response";
    }
  | { type: "asset"; asset_id: string };

export interface ReplayRequestPayload {
  method: string;
  url: string;
  headers: Array<[string, string]>;
  body?: ReplayBodyPayload;
}

export interface ReplayResult {
  log_id: number;
}

export interface AssetMetadata {
  id: string;
  size: number;
  content_type: string;
  sha256: string;
  created_at: number;
}
