import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { nextTick } from "vue";
import type { HttpApiChange } from "../api/generated/HttpApiChange";
import type { LogIdsRequest } from "../api/generated/LogIdsRequest";
import type { LogViewsRequest } from "../api/generated/LogViewsRequest";
import type { ProxyStatus } from "../api/generated/ProxyStatus";
import type { CaptureOutcome } from "../api/generated/CaptureOutcome";
import type { Column } from "../api/generated/Column";

const backend = vi.hoisted(() => ({
  subscribeChanges: vi.fn(),
  getSessionView: vi.fn(),
  getLogIds: vi.fn(),
  getLogViews: vi.fn(),
}));
vi.mock("../api/runtime-backend", () => ({ runtimeBackend: backend }));
vi.mock("./app", () => ({ reportError: vi.fn() }));
import { createLogsStore, type LogsStore } from "./logs";
import { proxyStore } from "./proxy";

let changed: (change: HttpApiChange) => void;
async function emitChange(change: HttpApiChange): Promise<void> {
  changed(change);
  await vi.advanceTimersByTimeAsync(40);
}

let logs: LogsStore;
const records = new Map<number, CaptureOutcome>();

function status(
  latest: Record<string, number>,
  active: Record<string, number[]> = {},
): ProxyStatus {
  return {
    status: "running",
    host: "127.0.0.1",
    port: 8089,
    started_at: 1,
    active_netlog: active,
    latest_netlog_id: latest,
    active_bypass_count: 0,
  };
}

async function flush(): Promise<void> {
  await nextTick();
  await vi.advanceTimersByTimeAsync(0);
}

beforeEach(async () => {
  vi.resetAllMocks();
  vi.useFakeTimers();
  vi.stubGlobal("window", Object.assign(new EventTarget(), { setTimeout, clearTimeout }));
  backend.subscribeChanges.mockImplementation(async (handler) => {
    changed = handler;
    return () => {};
  });
  records.clear();
  backend.getSessionView.mockResolvedValue({
    session_id: 7,
    columns: [],
    filter: { option: null, input: "" },
  });
  backend.getLogIds.mockImplementation(async (request: LogIdsRequest) => {
    const ids = [...records.keys()]
      .filter((id) => request.ids == null || request.ids.includes(id))
      .filter((id) => request.min_id == null || id > request.min_id)
      .filter((id) => request.max_id == null || id < request.max_id)
      .sort((a, b) => b - a)
      .slice(0, request.limit ?? 5000);
    return {
      matched_ids: ids,
      in_progress_ids: ids.filter((id) => records.get(id) === "in_progress"),
    };
  });
  backend.getLogViews.mockImplementation(async (request: LogViewsRequest) => ({
    columns: [],
    rows: request.logs.map(({ id }) => ({
      id,
      created_at: 1,
      updated_at: records.get(id) === "in_progress" ? 1 : 2,
      outcome: records.get(id) ?? "success",
      cells: [],
    })),
    exceptions: [],
  }));
  proxyStore.applyStatus(status({}));
  logs = createLogsStore();
  logs.view(7);
  await flush();
  expect(logs.loading).toBe(false);
  logs.startPolling();
  await flush();
  backend.getLogIds.mockClear();
});

afterEach(() => {
  logs.dispose();
  proxyStore.applyStatus({ status: "stopped" });
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

test("coalesced short requests are discovered with an empty active list and no active Session", async () => {
  records.set(1, "success");
  records.set(2, "failed");
  proxyStore.applyStatus(status({ 7: 2 }));
  await flush();
  expect(logs.ids).toEqual([2, 1]);
  expect(backend.getLogIds).toHaveBeenCalledTimes(1);
  expect(backend.getLogIds).toHaveBeenCalledWith(
    expect.objectContaining({ session_id: 7, min_id: 0 }),
  );
  await vi.advanceTimersByTimeAsync(10_000);
  expect(backend.getLogIds).toHaveBeenCalledTimes(1);
});

test("unchanged snapshots and other Sessions do not query the displayed Session", async () => {
  proxyStore.applyStatus(status({ 8: 10 }, { 8: [10] }));
  await flush();
  proxyStore.applyStatus(status({ 8: 11 }, { 8: [11] }));
  await flush();
  await vi.advanceTimersByTimeAsync(10_000);
  expect(backend.getLogIds).not.toHaveBeenCalled();

  records.set(1, "success");
  proxyStore.applyStatus(status({ 7: 1 }));
  await flush();
  backend.getLogIds.mockClear();
  proxyStore.applyStatus(status({ 7: 1 }));
  await flush();
  expect(backend.getLogIds).not.toHaveBeenCalled();
});

test("active requests poll their rows and refresh once more on completion", async () => {
  records.set(1, "in_progress");
  proxyStore.applyStatus(status({ 7: 1 }, { 7: [1] }));
  await flush();
  expect(logs.ids).toEqual([1]);
  expect(logs.row(1).outcome).toBe("in_progress");
  backend.getLogViews.mockClear();
  backend.getLogIds.mockClear();
  await vi.advanceTimersByTimeAsync(1000);
  expect(backend.getLogViews).toHaveBeenCalledTimes(1);
  expect(backend.getLogIds).not.toHaveBeenCalled();

  records.set(1, "success");
  proxyStore.applyStatus(status({ 7: 1 }));
  await flush();
  expect(logs.row(1).outcome).toBe("success");
  const queries = backend.getLogViews.mock.calls.length;
  await vi.advanceTimersByTimeAsync(10_000);
  expect(backend.getLogViews).toHaveBeenCalledTimes(queries);
});

test("empty-input Lua filters re-evaluate membership when a capture completes", async () => {
  logs.appliedFilter = { option: { kind: "script", script_name: "pending" }, input: "" };
  backend.getLogIds.mockImplementation(async (request: LogIdsRequest) => ({
    matched_ids:
      request.filter?.option?.kind === "script" && records.get(1) === "success" ? [] : [1],
    in_progress_ids: records.get(1) === "in_progress" ? [1] : [],
  }));
  records.set(1, "in_progress");
  proxyStore.applyStatus(status({ 7: 1 }, { 7: [1] }));
  await flush();
  expect(logs.ids).toEqual([1]);
  expect(backend.getLogIds).toHaveBeenCalledWith(
    expect.objectContaining({ filter: logs.appliedFilter, ids: [1] }),
  );

  records.set(1, "success");
  proxyStore.applyStatus(status({ 7: 1 }));
  await flush();
  expect(logs.ids).toEqual([]);
});

test("failed discovery retries even after every request has completed", async () => {
  records.set(1, "success");
  backend.getLogIds.mockRejectedValueOnce(new Error("temporary network failure"));
  proxyStore.applyStatus(status({ 7: 1 }));
  await flush();
  expect(logs.ids).toEqual([]);
  await vi.advanceTimersByTimeAsync(1000);
  expect(logs.ids).toEqual([1]);
  expect(backend.getLogIds).toHaveBeenCalledTimes(2);
});

test("column script changes refresh cached visible cells without changing log timestamps", async () => {
  const columns: Column[] = [{ kind: "script", script_name: "business/status", width: 100 }];
  backend.getSessionView.mockResolvedValue({
    session_id: 7,
    columns,
    filter: { option: null, input: "" },
  });
  logs.applyColumns(columns);
  logs.ids = [1];
  let value = "old";
  backend.getLogViews.mockImplementation(async (request: LogViewsRequest) => ({
    columns,
    rows: request.logs.map(({ id }) => ({
      id,
      created_at: 1,
      updated_at: 2,
      outcome: "success",
      cells: [value],
    })),
    exceptions: [],
  }));
  logs.setViewportIds([1]);
  await flush();
  expect(logs.row(1).cells).toEqual(["old"]);
  backend.getLogViews.mockClear();
  value = "new";
  await emitChange({
    resources: [{ kind: "column_scripts", names: ["business/status"] }, { kind: "session_view" }],
  });
  await flush();
  expect(logs.row(1).cells).toEqual(["new"]);
  expect(backend.getLogViews).toHaveBeenCalledWith(expect.objectContaining({ logs: [{ id: 1 }] }));
  expect(backend.getLogIds).not.toHaveBeenCalled();
});

test("unrelated column script changes keep cached rows", async () => {
  const columns: Column[] = [{ kind: "script", script_name: "status", width: 100 }];
  backend.getSessionView.mockResolvedValue({
    session_id: 7,
    columns,
    filter: { option: null, input: "" },
  });
  logs.applyColumns(columns);
  logs.ids = [1];
  logs.setViewportIds([1]);
  await flush();
  const cached = logs.rowsById.get(1);
  backend.getLogViews.mockClear();
  await emitChange({
    resources: [{ kind: "column_scripts", names: ["other"] }, { kind: "session_view" }],
  });
  await flush();
  expect(logs.rowsById.get(1)).toBe(cached);
  expect(backend.getLogViews).not.toHaveBeenCalled();
});

test("changed column scripts re-evaluate a local filter even when the column is hidden", async () => {
  const filter = {
    option: {
      kind: "column" as const,
      column: { kind: "script" as const, script_name: "status" },
      regex: false,
    },
    input: "ok",
  };
  logs.appliedFilter = filter;
  logs.ids = [1];
  records.set(2, "success");
  await emitChange({
    resources: [{ kind: "column_scripts", names: ["status"] }, { kind: "session_view" }],
  });
  await flush();
  expect(logs.ids).toEqual([2]);
  expect(logs.appliedFilter).toEqual(filter);
  expect(backend.getLogIds).toHaveBeenCalledWith(expect.objectContaining({ filter }));
});

test.each(["all", "other-session", "builtins"] as const)(
  "column cache invalidation handles %s",
  async (scenario) => {
    const columns: Column[] =
      scenario === "builtins"
        ? [{ kind: "method", width: 100 }]
        : [{ kind: "script", script_name: "status", width: 100 }];
    backend.getSessionView.mockResolvedValue({
      session_id: 7,
      columns,
      filter: { option: null, input: "" },
    });
    logs.applyColumns(columns);
    logs.ids = [1];
    logs.setViewportIds([1]);
    await flush();
    backend.getLogViews.mockClear();
    await emitChange(
      scenario === "all"
        ? { resources: [{ kind: "all" }] }
        : {
            resources: [{ kind: "column_scripts", names: ["status"] }],
            ...(scenario === "other-session" ? { session_id: 8 } : {}),
          },
    );
    await flush();
    if (scenario === "all") expect(backend.getLogIds).toHaveBeenCalled();
    else expect(backend.getLogViews).not.toHaveBeenCalled();
  },
);

test("column invalidation discards a response rendered before the script changed", async () => {
  const columns: Column[] = [{ kind: "script", script_name: "status", width: 100 }];
  backend.getSessionView.mockResolvedValue({
    session_id: 7,
    columns,
    filter: { option: null, input: "" },
  });
  logs.applyColumns(columns);
  logs.ids = [2, 1];
  let resolveOld!: (value: unknown) => void;
  backend.getLogViews.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        resolveOld = resolve;
      }),
  );
  backend.getLogViews.mockImplementation(async (request: LogViewsRequest) => ({
    columns,
    rows: request.logs.map(({ id }) => ({
      id,
      created_at: 1,
      updated_at: 2,
      outcome: "success",
      cells: ["new"],
    })),
    exceptions: [],
  }));
  logs.setViewportIds([2]);
  await flush();
  await emitChange({ resources: [{ kind: "column_scripts", names: ["status"] }] });
  await flush();
  resolveOld({
    columns,
    rows: [{ id: 2, created_at: 1, updated_at: 2, outcome: "success", cells: ["old"] }],
    exceptions: [],
  });
  await flush();
  expect(logs.row(2).cells).toEqual(["new"]);
});

test("discarded records advance discovery without keeping idle polling alive", async () => {
  proxyStore.applyStatus(status({ 7: 5 }));
  await flush();
  expect(logs.ids).toEqual([]);
  await vi.advanceTimersByTimeAsync(10_000);
  expect(backend.getLogIds).toHaveBeenCalledTimes(1);
  records.set(6, "success");
  proxyStore.applyStatus(status({ 7: 6 }));
  await flush();
  expect(logs.ids).toEqual([6]);
  expect(backend.getLogIds).toHaveBeenLastCalledWith(expect.objectContaining({ min_id: 5 }));
});
