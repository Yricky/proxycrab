import type { CaptureOutcome } from "../api/generated/CaptureOutcome";
import type { ProxyStatus } from "../api/generated/ProxyStatus";

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
