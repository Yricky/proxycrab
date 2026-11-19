import { createSSRApp } from "vue";
import { renderToString } from "vue/server-renderer";
import { expect, test, vi } from "vitest";

const { backend, openBundleImport } = vi.hoisted(() => ({
  backend: { importBundle: vi.fn() } as { importBundle?: ReturnType<typeof vi.fn> },
  openBundleImport: vi.fn(),
}));
vi.mock("./api", () => ({ useBackend: () => backend }));
vi.mock("./windows/launcher", () => ({ openBundleImport }));
vi.mock("./stores/app", () => ({ reportError: vi.fn() }));
vi.mock("./stores/windows", () => ({ windowsStore: { windows: [] } }));
vi.mock("./stores/sessions", () => ({
  sessionsStore: { sessions: [], activeSessionId: null, init: vi.fn() },
}));
vi.mock("./stores/proxy", () => ({
  proxyStore: { init: vi.fn(), refresh: vi.fn(), dispose: vi.fn() },
}));
vi.mock("./stores/logs", () => ({
  createLogsStore: () => ({ view: vi.fn(), stopPolling: vi.fn(), dispose: vi.fn() }),
}));
vi.mock("./stores/breakpoints", () => ({ breakpointsStore: { stopPolling: vi.fn() } }));
vi.mock("./stores/approvals", () => ({ approvalsStore: {} }));
vi.mock("./stores/skill", () => ({ skillStore: { refresh: vi.fn() } }));
vi.mock("./components/AppToolbar.vue", () => ({ default: { render: () => null } }));
vi.mock("./components/SessionSidebar.vue", () => ({ default: { render: () => null } }));
vi.mock("./components/FilterBar.vue", () => ({ default: { render: () => null } }));
vi.mock("./components/InterceptorPipeline.vue", () => ({ default: { render: () => null } }));
vi.mock("./components/LogTable.vue", () => ({ default: { render: () => null } }));
vi.mock("./components/FloatingWindow.vue", () => ({ default: { render: () => null } }));
vi.mock("./components/Toast.vue", () => ({ default: { render: () => null } }));
vi.mock("./components/ConfirmDialog.vue", () => ({ default: { render: () => null } }));
vi.mock("./components/ContextMenu.vue", () => ({ default: { render: () => null } }));
import AppMain from "./AppMain.vue";

async function mount() {
  let state: any;
  const app = createSSRApp(AppMain);
  app.mixin({
    created() {
      if (this.$options.__name === "AppMain") state = this.$.setupState;
    },
  });
  await renderToString(app);
  return state;
}
function dragEvent() {
  return {
    dataTransfer: { types: ["Files"], files: [{ name: "bundle.zip" }] },
    preventDefault: vi.fn(),
    stopPropagation: vi.fn(),
  };
}

test("internal file drags do not show the import overlay or open a bundle", async () => {
  const view = await mount();
  const event = dragEvent();
  openBundleImport.mockClear();
  view.fileDragStart();
  view.fileDragEnter(event);
  view.fileDrag(event);
  expect(view.draggingFiles).toBe(false);
  expect(event.preventDefault).not.toHaveBeenCalled();
  await view.fileDrop(event);
  expect(openBundleImport).not.toHaveBeenCalled();

  view.fileDragEnd();
  view.fileDragEnter(event);
  view.fileDrag(event);
  expect(view.draggingFiles).toBe(true);
  expect(event.preventDefault).toHaveBeenCalled();
  // Moving across children retains the overlay until the last leave.
  view.fileDragEnter(event);
  view.fileDragLeave();
  expect(view.draggingFiles).toBe(true);
  view.fileDragLeave();
  expect(view.draggingFiles).toBe(false);

  view.fileDragEnter(event);
  await view.fileDrop(event);
  expect(view.draggingFiles).toBe(false);
  expect(openBundleImport).toHaveBeenCalledWith(event.dataTransfer.files[0]);
});

test("file import stays unavailable without the backend capability", async () => {
  const view = await mount();
  const importBundle = backend.importBundle;
  delete backend.importBundle;
  openBundleImport.mockClear();
  try {
    const event = dragEvent();
    view.fileDragEnter(event);
    view.fileDrag(event);
    await view.fileDrop(event);
    expect(view.draggingFiles).toBe(false);
    expect(event.preventDefault).not.toHaveBeenCalled();
    expect(openBundleImport).not.toHaveBeenCalled();
  } finally {
    backend.importBundle = importBundle;
  }
});
