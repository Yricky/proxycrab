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
