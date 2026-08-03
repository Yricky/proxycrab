// Types mirroring crates/proxy-crab-mitm/src/model.rs and crates/proxy-crab-mgr/src/dto.rs.
// Field names match serde's snake_case wire format exactly.

export interface ManagerError {
  code: string;
  message: string;
}

// ---------- model.rs ----------

export type ColumnKind = "method" | "uri" | "code" | "source" | "stage" | "script";

export type Column =
  | { kind: "method"; width: number }
  | { kind: "uri"; width: number }
  | { kind: "code"; width: number }
  | { kind: "source"; width: number }
  | { kind: "stage"; width: number }
  | { kind: "script"; width: number; script_name: string };

export type FilterColumn =
  | { kind: "method" }
  | { kind: "uri" }
  | { kind: "code" }
  | { kind: "source" }
  | { kind: "stage" }
  | { kind: "script"; script_name: string };

export type FilterOption =
  | { kind: "column"; column: FilterColumn; case_sensitive: boolean }
  | { kind: "script"; script_name: string };

export interface SessionFilter {
  option: FilterOption | null;
  input: string;
}

export interface AppConfig {
  proxy_host: string;
  proxy_port: number;
  api_host: string;
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
  | { type: "text"; content: string }
  | { type: "json"; content: unknown }
  | { type: "binary"; size: number }
  | { type: "large"; size: number };

export type Modification =
  | { kind: "snapshot"; headers: Record<string, string[]> }
  | { kind: "header_append"; name: string; value: string }
  | { kind: "header_set"; name: string; value: string }
  | { kind: "header_remove"; name: string; values: string[] }
  | { kind: "body_replace_string"; content: string }
  | { kind: "body_replace_file"; path: string }
  | { kind: "tag_set"; key: string; value: string };

export type InterceptorExecutionOrigin = "saved" | "temporary";

export type ProxyStatus =
  | { status: "stopped" }
  | { status: "starting" }
  | { status: "running"; host: string; port: number }
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
  | "active_session"
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

export interface SkillInstallInfo {
  parent_path: string;
  target_path: string;
  exists: boolean;
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

export interface LogIdsRequest {
  session_id?: number | null;
  filter?: SessionFilter | null;
  min_id?: number | null;
  max_id?: number | null;
  limit?: number | null;
}

export interface LogIdsPayload {
  ids: number[];
  filter: SessionFilter;
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

export interface ColumnView {
  key: string;
  name: string;
  kind: string;
  width: number | null;
  script_name?: string;
}

export interface LogViewRow {
  id: number;
  updated_at: number;
  cells: string[];
}

export interface LogViewException {
  id: number;
  column_index?: number | null;
  code: string;
  message: string;
}

export interface LogViewsPayload {
  columns: ColumnView[];
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
  content: string;
  modifications: Modification[];
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
