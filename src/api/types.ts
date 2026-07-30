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

export interface AppConfig {
  proxy_host: string;
  proxy_port: number;
  api_host: string;
  api_port: number;
  active_session_id: number | null;
  active_request_interceptors: string[];
  active_response_interceptors: string[];
  columns: Column[];
  filter_history: string[];
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

export interface InterceptorInfo {
  name: string;
  enabled: boolean;
  order: number | null;
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
  | { kind: "header_append"; name: string; value: string }
  | { kind: "header_set"; name: string; value: string }
  | { kind: "header_remove"; name: string; values: string[] }
  | { kind: "body_replace_string"; content: string }
  | { kind: "body_replace_file"; path: string };

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

// ---------- dto.rs ----------

export interface CreateSessionRequest {
  name?: string | null;
  description?: string | null;
}

export interface UpdateSessionRequest {
  name?: string | null;
  description?: string | null;
}

export interface LogsQuery {
  session_id?: number | null;
  limit?: number | null;
  after_id?: number | null;
}

export interface FilterLogsRequest {
  session_id?: number | null;
  limit?: number | null;
  script: string;
}

export interface ColumnView {
  key: string;
  name: string;
  kind: string;
  width: number | null;
  script_name?: string;
}

export interface LogRow {
  id: number;
  cells: string[];
}

export interface LogsPayload {
  columns: ColumnView[];
  rows: LogRow[];
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
  source_type: string;
  source_addr: string | null;
  stage: string;
  outcome: CaptureOutcome;
  error: CaptureError | null;
  request: RequestDetail;
  response: ResponseDetail | null;
  req_modifications: Modification[];
  resp_modifications: Modification[];
}

export interface ScriptRequest {
  name: string;
  content?: string;
}

export interface UpdateScriptRequest {
  name?: string | null;
  content?: string | null;
}

export interface ColumnInput {
  kind: string;
  width?: number | null;
  script_name?: string | null;
}

export interface InterceptorCreateRequest {
  kind: InterceptorKind;
  name: string;
  content?: string;
  enabled?: boolean;
}

export interface InterceptorUpdateRequest {
  name?: string | null;
  content?: string | null;
  enabled?: boolean | null;
}

export interface InterceptorList {
  kind: InterceptorKind;
  active_order: string[];
  items: InterceptorInfo[];
}

export interface SetInterceptorOrderRequest {
  kind: InterceptorKind;
  order: string[];
}

export interface SystemLogsQuery {
  after_seq?: number | null;
  limit?: number | null;
}

export interface CertificateResponse {
  pem: string;
}
