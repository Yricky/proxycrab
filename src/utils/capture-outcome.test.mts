import assert from "node:assert/strict";
import test from "node:test";
import { isStaleInProgress } from "./capture-outcome.ts";

test("in-progress records are stale whenever the proxy is not running", () => {
  for (const status of [
    { status: "stopped" as const },
    { status: "starting" as const },
    { status: "stopping" as const },
    { status: "failed" as const, message: "bind failed" },
  ]) {
    assert.equal(isStaleInProgress("in_progress", 200, status), true);
  }
});

test("a running proxy only invalidates records older than its start time", () => {
  const status = {
    status: "running" as const,
    host: "127.0.0.1",
    port: 8089,
    started_at: 200,
  };
  assert.equal(isStaleInProgress("in_progress", 199, status), true);
  assert.equal(isStaleInProgress("in_progress", 200, status), false);
  assert.equal(isStaleInProgress("in_progress", 201, status), false);
});

test("terminal outcomes are never reclassified as stale", () => {
  const status = { status: "stopped" as const };
  for (const outcome of ["success", "failed", "tunneled"] as const) {
    assert.equal(isStaleInProgress(outcome, 0, status), false);
  }
});

