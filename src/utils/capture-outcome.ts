import type { CaptureOutcome, ProxyStatus } from "../api/types";

export function isStaleByProxyRun(
  outcome: CaptureOutcome,
  createdAt: number,
  proxyStatus: ProxyStatus,
): boolean {
  return (
    outcome === "in_progress" &&
    (proxyStatus.status !== "running" || createdAt < proxyStatus.started_at)
  );
}

export function isInactiveInProgress(outcome: CaptureOutcome, active: boolean): boolean {
  return outcome === "in_progress" && !active;
}
