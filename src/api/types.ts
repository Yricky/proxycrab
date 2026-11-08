// API 类型契约：
// - `./generated/`：ts-rs 从 Rust（crates/*/src/{model,dto,websocket,bypass}.rs）生成的绑定，
//   由 `cargo test --workspace` 产出并提交入库，CI 校验未漂移——新增/修改 Rust 类型后重跑即可。
// - 本文件：仅保留尚未迁移到 ts-rs 的手写类型（Script 的 display_name 由 Rust 自定义
//   序列化器派生；session_share / permission / skill / bundle / clash / websocket 消息等
//   属后续批次），以及传输层合成的错误码扩展。

import type { ErrorCode } from "./generated/ErrorCode";
import type { ManagerError } from "./generated/ManagerError";
import type { SessionMetadata } from "./generated/SessionMetadata";
import type { WsInterceptor } from "./generated/WsInterceptor";

export type { ErrorCode, ManagerError, SessionMetadata, WsInterceptor };

/** 传输层合成错误码：网络失败、Tauri invoke 失败、只读分享或非 JSON 响应时前端兜底构造。 */
export type TransportErrorCode =
  "network_error" | "readonly_share" | "invoke_error" | `http_${number}`;

/** API 层实际可抛出的错误形态：后端契约错误 + 传输层合成错误。 */
export type ApiError = Omit<ManagerError, "code"> & { code: ErrorCode | TransportErrorCode };

// Script 的 display_name 由 Rust 侧自定义序列化器从内容注释派生（存在时才输出），暂保留手写。
export interface Script {
  name: string;
  display_name?: string | null;
  content: string;
}

/** 内置列 kind 字面量（Column 的窄化辅助）。 */
export type ColumnKind =
  "method" | "uri" | "code" | "source" | "stage" | "created_at" | "updated_at" | "script";

export interface WebSocketInvocation extends WsInterceptor {
  invocation_id: number;
}
export interface WebSocketMessageRef {
  side: "request" | "response";
  id: number;
}
export interface WebSocketMessage {
  id: number;
  sequence?: number;
  root?: WebSocketMessageRef;
  parent?: WebSocketMessageRef;
  producer?: WebSocketInvocation & { send_index: number };
  dropped_by?: WebSocketInvocation;
  status?: "sent" | "dropped" | "unsent" | "failed";
  error?: string;
  errors?: (WebSocketInvocation & { error: string })[];
  type: "text" | "binary";
  started_at: number;
  completed_at: number;
  size: number;
  content_available: boolean;
  omitted_reason: string | null;
}
export interface WebSocketPage {
  messages: WebSocketMessage[];
  next_after_id: number;
  has_more: boolean;
}

export interface WorkspacePaths {
  current_path: string;
  configured_path: string;
}

/** 创建一次保存，字节通过返回的 Writer 追加。 */
export interface SaveFileRequest {
  filename: string;
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

export interface SessionShareSummary {
  session_id: number;
  persistent: boolean;
}

export interface SessionSharesPayload {
  shares: SessionShareSummary[];
}

export interface SessionShareConfig {
  sessionConfigShare?: { assets: string[] } | null;
  redactionScript?: string | null;
  persistent: boolean;
}

export interface SessionShareState {
  session_config_share?: { assets: string[] } | null;
  redaction_identity?: string | null;
  persistent: boolean;
  redaction_script?: string | null;
  session_id: number;
  enabled: boolean;
  token?: string;
}

export interface ClashSubscriptionState {
  enabled: boolean;
  server?: string;
}

export interface ClashConfig {
  yaml: string;
  default_yaml: string;
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
  redaction_script?: string | null;
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

export interface AssetMetadata {
  id: string;
  size: number;
  content_type: string;
  sha256: string;
  created_at: number;
}

export interface BundleIntent {
  id: string;
  type: string;
  path: string;
  assets: string[];
}
export interface BundleFileStatus {
  path: string;
  status: "same" | "conflict" | "new";
  fingerprint: string | null;
  can_rename: boolean;
}
export interface BundleResolution {
  path: string;
  action: "keep" | "overwrite" | "rename";
  fingerprint: string | null;
  name?: string;
}
export interface BundleImportResult {
  import_id: string | null;
  files: BundleFileStatus[];
  sessions: SessionMetadata[];
}
