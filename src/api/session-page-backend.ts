import type { Backend as ApplicationBackend } from "./backend";

/** Public page contract: shared reads are present; other host capabilities may be absent. */
export type Backend = Pick<
  ApplicationBackend,
  | "fetchBody"
  | "saveFile"
  | "openExternal"
  | "subscribeChanges"
  | "subscribeApprovalChanges"
  | "getConfig"
  | "getProxyStatus"
  | "listSessions"
  | "listArchivedSessions"
  | "getLogIds"
  | "getLogViews"
  | "validateFilterRegex"
  | "getLog"
  | "getInterceptorContent"
  | "getInterceptorSnapshot"
  | "getSessionView"
  | "getSessionInterceptors"
  | "listColumnScripts"
  | "listFilterScripts"
> &
  Partial<ApplicationBackend>;
