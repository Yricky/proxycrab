import type { ErrorCode } from "./generated/ErrorCode";
import type { ApiError, TransportErrorCode } from "./types";

/** Stable error shared by Tauri and HTTP Backend implementations. */
export class BackendError extends Error {
  readonly code: ErrorCode | TransportErrorCode;

  constructor(error: ApiError) {
    super(error.message);
    this.name = "BackendError";
    this.code = error.code;
  }
}
