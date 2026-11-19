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
import { HTTP_API_CHANGE_EVENT, startHttpApiSync, stopHttpApiSync } from "./http-api-sync";
let changed: (change: HttpApiChange) => void;
let events: HttpApiChange[];
beforeEach(() => {
  vi.resetAllMocks();
  vi.useFakeTimers();
  vi.stubGlobal("window", Object.assign(new EventTarget(), { setTimeout, clearTimeout }));
  events = [];
  window.addEventListener(HTTP_API_CHANGE_EVENT, (event) =>
    events.push((event as CustomEvent<HttpApiChange>).detail),
  );
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
  changed({ resources: [{ kind: "session_share" }] });
  await vi.advanceTimersByTimeAsync(40);
  expect(mocks.refreshShares).toHaveBeenCalledTimes(1);
  expect(mocks.syncSessions).not.toHaveBeenCalled();
});

test("coalesced column changes preserve every affected script name", async () => {
  await startHttpApiSync();
  changed({ resources: [{ kind: "column_scripts", names: ["first"] }, { kind: "session_view" }] });
  changed({
    resources: [{ kind: "column_scripts", names: ["second", "first"] }, { kind: "session_view" }],
  });
  await vi.advanceTimersByTimeAsync(40);
  expect(events).toEqual([
    {
      resources: [{ kind: "column_scripts", names: ["first", "second"] }, { kind: "session_view" }],
    },
  ]);
});

test("coalescing preserves each Session scope and keeps global events global", async () => {
  await startHttpApiSync();
  changed({ resources: [{ kind: "column_scripts", names: ["first"] }], session_id: 1 });
  changed({ resources: [{ kind: "column_scripts", names: ["second"] }], session_id: 2 });
  changed({ resources: [{ kind: "session_view" }] });
  await vi.advanceTimersByTimeAsync(40);
  expect(events).toEqual([
    { resources: [{ kind: "column_scripts", names: ["first"] }], session_id: 1 },
    { resources: [{ kind: "column_scripts", names: ["second"] }], session_id: 2 },
    { resources: [{ kind: "session_view" }] },
  ]);
  events.length = 0;
  changed({ resources: [{ kind: "column_scripts", names: ["third"] }], session_id: 1 });
  await vi.advanceTimersByTimeAsync(40);
  expect(events).toEqual([
    { resources: [{ kind: "column_scripts", names: ["third"] }], session_id: 1 },
  ]);
});

test("all changes refresh global resources and retain full invalidation", async () => {
  await startHttpApiSync();
  changed({ resources: [{ kind: "column_scripts", names: ["first"] }] });
  changed({ resources: [{ kind: "all" }] });
  await vi.advanceTimersByTimeAsync(40);
  expect(mocks.syncSessions).toHaveBeenCalledTimes(1);
  expect(mocks.refresh).toHaveBeenCalledTimes(3);
  expect(events[0]?.resources).toContainEqual({ kind: "all" });
});

test("Session lifecycle changes use the combined list refresh without a duplicate batch", async () => {
  await startHttpApiSync();
  changed({ resources: [{ kind: "sessions" }, { kind: "session_share" }] });
  await vi.advanceTimersByTimeAsync(40);
  expect(mocks.syncSessions).toHaveBeenCalledTimes(1);
  expect(mocks.refreshShares).not.toHaveBeenCalled();
});
