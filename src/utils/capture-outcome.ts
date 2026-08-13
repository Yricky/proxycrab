import type { CaptureOutcome, ProxyStatus } from "../api/types";

export function isStaleInProgress(
  outcome: CaptureOutcome,
  createdAt: number,
  proxyStatus: ProxyStatus,
): boolean {
  return (
    outcome === "in_progress" &&
    (proxyStatus.status !== "running" || createdAt < proxyStatus.started_at)
  );
}

