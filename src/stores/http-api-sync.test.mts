import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { HttpApiChange } from "../api/generated/HttpApiChange";
const mocks = vi.hoisted(() => ({
  subscribe: vi.fn(),
  refreshShares: vi.fn(),
  syncSessions: vi.fn(),
  refresh: vi.fn(),
  reportError: vi.fn(),
}));
vi.mock("../api/runtime-backend", () => ({
  runtimeBackend: { subscribeChanges: mocks.subscribe },
}));
vi.mock("./sessions", () => ({
  sessionsStore: { refreshShares: mocks.refreshShares, syncFromBackend: mocks.syncSessions },
}));
vi.mock("./app", () => ({ reportError: mocks.reportError }));
vi.mock("./proxy", () => ({ proxyStore: { refresh: mocks.refresh } }));
vi.mock("./routing", () => ({ routingStore: { refresh: mocks.refresh } }));
vi.mock("./bypass", () => ({ bypassStore: { refresh: mocks.refresh } }));
import { startHttpApiSync, stopHttpApiSync } from "./http-api-sync";
let changed: (change: HttpApiChange) => void;
beforeEach(() => {
  vi.resetAllMocks();
  vi.useFakeTimers();
  vi.stubGlobal("window", Object.assign(new EventTarget(), { setTimeout, clearTimeout }));
  mocks.subscribe.mockImplementation(async (handler) => {
    changed = handler;
    return () => {};
  });
});
afterEach(() => {
  stopHttpApiSync();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

test("external share changes refresh the batch without reloading Session lists", async () => {
  await startHttpApiSync();
  changed({ resources: ["session_share"] });
  await vi.advanceTimersByTimeAsync(40);
  expect(mocks.refreshShares).toHaveBeenCalledTimes(1);
  expect(mocks.syncSessions).not.toHaveBeenCalled();
});

test("Session lifecycle changes use the combined list refresh without a duplicate batch", async () => {
  await startHttpApiSync();
  changed({ resources: ["sessions", "session_share"] });
  await vi.advanceTimersByTimeAsync(40);
  expect(mocks.syncSessions).toHaveBeenCalledTimes(1);
  expect(mocks.refreshShares).not.toHaveBeenCalled();
});
