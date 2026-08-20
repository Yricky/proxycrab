import type { ManagerError } from "./types";

/** Stable error shared by Tauri and HTTP Backend implementations. */
export class BackendError extends Error {
  readonly code: string;

  constructor(error: ManagerError) {
    super(error.message);
    this.name = "BackendError";
    this.code = error.code;
  }
}
