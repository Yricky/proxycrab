import { expect, test } from "vitest";
import { isInactiveInProgress, isStaleByProxyRun } from "./capture-outcome.ts";

test("in-progress records are stale whenever the proxy is not running", () => {
  for (const status of [
    { status: "stopped" as const },
    { status: "starting" as const },
    { status: "stopping" as const },
    { status: "failed" as const, message: "bind failed" },
  ]) {
    expect(isStaleByProxyRun("in_progress", 200, status)).toBe(true);
  }
});

test("a running proxy only invalidates records older than its start time", () => {
  const status = {
    status: "running" as const,
    host: "127.0.0.1",
    port: 8089,
    started_at: 200,
    active_netlog: {},
    active_bypass_count: 0,
  };
  expect(isStaleByProxyRun("in_progress", 199, status)).toBe(true);
  expect(isStaleByProxyRun("in_progress", 200, status)).toBe(false);
  expect(isStaleByProxyRun("in_progress", 201, status)).toBe(false);
});

test("terminal outcomes are never reclassified as stale", () => {
  const status = { status: "stopped" as const };
  for (const outcome of ["success", "failed", "tunneled"] as const) {
    expect(isStaleByProxyRun(outcome, 0, status)).toBe(false);
  }
});

test("netlog activity comes only from exact status membership", () => {
  expect(isInactiveInProgress("in_progress", true)).toBe(false);
  expect(isInactiveInProgress("in_progress", false)).toBe(true);
  expect(isInactiveInProgress("success", false)).toBe(false);
});
