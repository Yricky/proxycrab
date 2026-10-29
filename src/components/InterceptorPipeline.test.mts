import { createSSRApp } from "vue";
import { renderToString } from "vue/server-renderer";
import { afterEach, expect, test, vi } from "vitest";
const { backend, refresh } = vi.hoisted(() => ({
  backend: {} as Record<string, unknown>,
  refresh: vi.fn(),
}));
vi.mock("../api", () => ({ useBackend: () => backend }));
vi.mock("../stores/app", () => ({ appStore: {}, reportError: vi.fn() }));
vi.mock("../stores/interceptors", () => ({
  interceptorsStore: {
    request: [{ name: "auth", display_name: "Auth", valid: true, enabled: true }],
    response: [{ name: "missing", valid: false, enabled: false }],
    refresh,
  },
}));
vi.mock("../stores/sessions", () => ({ sessionsStore: { activeSessionId: 7 } }));
vi.mock("../stores/breakpoints", () => ({ breakpointsStore: { count: () => 3 } }));
vi.mock("../windows/launcher", () => ({
  openHarExport: vi.fn(),
  openBreakpointList: vi.fn(),
  openScriptEditor: vi.fn(),
}));
vi.mock("./InterceptorPicker.vue", () => ({ default: { render: () => null } }));
vi.mock("./AppTooltip.vue", () => ({
  default: {
    props: ["title", "status"],
    template: '<span :data-title="title" :data-status="status"><slot /></span>',
  },
}));
import InterceptorPipeline from "./InterceptorPipeline.vue";
afterEach(() => {
  for (const key of Object.keys(backend)) delete backend[key];
  vi.clearAllMocks();
});
function render() {
  return renderToString(
    createSSRApp(InterceptorPipeline, { logs: { sessionId: 7, loading: false } as any }),
  );
}
test("readonly pipeline retains names and state but no add, drag, breakpoint, sweep or action hints", async () => {
  backend.getSessionInterceptors = vi.fn();
  backend.getLog = vi.fn();
  backend.fetchBody = vi.fn();
  backend.saveFile = vi.fn();
  const html = await render();
  expect(html).toContain('aria-label="Auth"');
  expect(html).toContain('data-status="已开启"');
  expect(html).toContain('data-status="脚本文件不存在"');
  expect(html).toContain('draggable="false"');
  expect(html).toContain('aria-disabled="true"');
  expect(html).not.toContain('draggable="true"');
  expect(html).not.toContain("在此添加");
  expect(html).not.toContain("点击关闭");
  expect(html).not.toContain("右键可重新创建");
  expect(html).not.toContain("breakpoint-count");
  expect(html).not.toContain('aria-label="清扫会话"');
  expect(html).toContain('aria-label="导出 HAR"');
  expect(refresh).toHaveBeenCalledWith(7);
});
test("write-capable backends retain add, toggle, drag, script and breakpoint behavior", async () => {
  for (const method of [
    "getSessionInterceptors",
    "replaceSessionInterceptors",
    "listInterceptors",
    "getInterceptor",
    "createInterceptor",
    "updateInterceptor",
    "deleteInterceptor",
    "listBreakpoints",
    "getBreakpoint",
    "createSession",
    "replaceActiveSession",
    "archiveSession",
  ])
    backend[method] = vi.fn();
  const html = await render();
  expect(html).toContain("在此添加请求拦截器");
  expect(html).toContain("在此添加响应拦截器");
  expect(html).toContain('draggable="true"');
  expect(html).toContain("点击关闭");
  expect(html).toContain("右键可重新创建");
  expect(html).toContain("breakpoint-count");
  expect(html).toContain('aria-label="清扫会话"');
});
