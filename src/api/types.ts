// API 类型契约：
// - `./generated/`：ts-rs 从 Rust（crates/*/src 与 src-tauri）生成的绑定，由
//   `cargo test --workspace` 产出并提交入库，CI 校验未漂移——修改 Rust 类型后重跑即可。
// - 本文件：仅前端自有契约。Script 的 display_name 由 Rust 自定义序列化器从
//   内容注释派生（存在时才输出），无法声明式表达，保留手写；其余为纯前端 API 形态。

import type { ErrorCode } from "./generated/ErrorCode";
import type { ManagerError } from "./generated/ManagerError";

export type { ErrorCode, ManagerError };

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

/** 创建一次保存，字节通过返回的 Writer 追加。 */
export interface SaveFileRequest {
  filename: string;
}
