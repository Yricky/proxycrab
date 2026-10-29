import { afterEach, expect, test, vi } from "vitest";
import { windowsStore } from "../stores/windows";
import { openReplay } from "./launcher";

const { backend } = vi.hoisted(() => ({ backend: { replay: vi.fn() as unknown } }));
vi.mock("../api/runtime-backend", () => ({
  getBackend: () => backend,
  runtimeBackend: {},
}));
vi.mock("../stores/app", () => ({ appStore: {}, reportError: vi.fn() }));

afterEach(() => {
  windowsStore.windows = [];
  backend.replay = vi.fn();
  vi.unstubAllGlobals();
});

test("replay windows open independently without secure-context crypto", async () => {
  vi.stubGlobal("crypto", {});
  openReplay();
  openReplay();
  const ids = windowsStore.windows.map((win) => win.id);
  expect(ids).toHaveLength(2);
  expect(new Set(ids).size).toBe(2);
  expect(windowsStore.windows.every((win) => win.title === "发送请求")).toBe(true);
  await windowsStore.close(ids[0]!);
  openReplay();
  expect(windowsStore.windows).toHaveLength(2);
  expect(ids).not.toContain(windowsStore.windows[1]!.id);
});

test("replay capability remains required to open a window", () => {
  backend.replay = undefined;
  openReplay();
  expect(windowsStore.windows).toHaveLength(0);
});

test("HAR opens a progress window without starting export and requires read/save capabilities", async () => {
  const { openHarExport } = await import("./launcher");
  const api = backend as typeof backend & Record<string, unknown>;
  const logs = { sessionId: 7, sortedIds: [9, 3], loading: false };
  api.getLog = vi.fn();
  api.fetchBody = vi.fn();
  api.saveFile = vi.fn();
  openHarExport(logs as any);
  expect(windowsStore.windows).toHaveLength(1);
  expect(windowsStore.windows[0]!.title).toBe("导出 HAR");
  expect(windowsStore.windows[0]!.props.logs).toEqual(logs);
  expect(api.getLog).not.toHaveBeenCalled();
  expect(api.saveFile).not.toHaveBeenCalled();
  windowsStore.windows = [];
  api.saveFile = undefined;
  openHarExport(logs as any);
  expect(windowsStore.windows).toHaveLength(0);
  delete api.getLog;
  delete api.fetchBody;
  delete api.saveFile;
});
