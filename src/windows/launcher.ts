// Central launchers for floating windows. Keeping component imports here
// avoids spreading window ids / default sizes across the UI code.
import { windowsStore } from "../stores/windows";
import type { InterceptorExecution, InterceptorKind } from "../api/types";
import LogDetailWindow from "./LogDetailWindow.vue";
import InterceptorManagerWindow from "./InterceptorManagerWindow.vue";
import ColumnManagerWindow from "./ColumnManagerWindow.vue";
import FilterManagerWindow from "./FilterManagerWindow.vue";
import Base64Window from "./Base64Window.vue";
import JwtWindow from "./JwtWindow.vue";
import CertManagerWindow from "./CertManagerWindow.vue";
import SystemLogsWindow from "./SystemLogsWindow.vue";
import SettingsWindow from "./SettingsWindow.vue";
import ReadonlyViewerWindow from "./ReadonlyViewerWindow.vue";
import SkillManagerWindow from "./SkillManagerWindow.vue";
import RoutingManagerWindow from "./RoutingManagerWindow.vue";
import BypassWindow from "./BypassWindow.vue";
import BreakpointListWindow from "./BreakpointListWindow.vue";
import AgentsPresetsWindow from "./AgentsPresetsWindow.vue";
import ArchivedSessionsWindow from "./ArchivedSessionsWindow.vue";
import ApprovalWindow from "./ApprovalWindow.vue";
import BrowserWindow from "./BrowserWindow.vue";
import SessionExportShareWindow from "./SessionExportShareWindow.vue";

/** 内置浏览器窗口：同一 URL 复用已有窗口。 */
export function openBrowser(url: string): void {
  let host = url;
  try {
    host = new URL(url).host;
  } catch {
    // 保留原始 url 作为标题
  }
  windowsStore.open(`browser-${url}`, {
    title: host,
    component: BrowserWindow,
    props: { url },
    width: 960,
    height: 640,
  });
}

export function openLogDetail(sessionId: number, logId: number): void {
  windowsStore.open(`detail-${sessionId}-${logId}`, {
    title: `#${logId} 详情`,
    component: LogDetailWindow,
    props: { sessionId, logId },
    width: 860,
    height: 560,
  });
}

export function openBreakpointList(
  sessionId: number,
  phase: InterceptorKind,
  interceptorName: string,
): void {
  windowsStore.open(`breakpoints-${sessionId}-${phase}-${interceptorName}`, {
    title: `${interceptorName} — 断点列表`,
    component: BreakpointListWindow,
    props: { sessionId, phase, interceptorName },
    width: 680,
    height: 420,
  });
}

export function openBreakpointDetail(breakpointId: number, captureId: number): void {
  windowsStore.open(`breakpoint-${breakpointId}`, {
    title: `#${captureId} — 断点详情`,
    component: LogDetailWindow,
    props: { breakpointId },
    width: 900,
    height: 650,
  });
}

export function openScriptEditor(kind: InterceptorKind, name: string): void {
  const kindLabel = kind === "request" ? "请求拦截器" : "响应拦截器";
  windowsStore.open(`interceptor-manager-${kind}`, {
    title: kindLabel,
    component: InterceptorManagerWindow,
    props: { kind, initialName: name },
    width: 980,
    height: 600,
  });
  // Fresh windows pick the script via initialName; already-open ones react to this event.
  window.dispatchEvent(
    new CustomEvent("proxycrab-focus-script", { detail: { scope: kind, name } }),
  );
}

/** 通用只读编辑器窗口：展示完整内容，底部状态栏可切换高亮格式与自动换行。 */
export function openReadonlyViewer(
  id: string,
  title: string,
  content: string,
  language = "plaintext",
): void {
  windowsStore.open(`readonly-viewer-${id}`, {
    title,
    component: ReadonlyViewerWindow,
    props: { content, language },
    width: 720,
    height: 520,
  });
}

export function openScriptSnapshot(
  sessionId: number,
  logId: number,
  execution: InterceptorExecution,
): void {
  openReadonlyViewer(
    `script-snapshot-${sessionId}-${logId}-${execution.execution_id}`,
    `${execution.name} — 历史脚本`,
    execution.content,
    "lua",
  );
}

export function openModificationValue(
  id: string,
  label: string,
  value: string,
): void {
  openReadonlyViewer(`mod-value-${id}`, `${label} — 完整内容`, value);
}

export function openRequestInterceptorManager(): void {
  windowsStore.open("interceptor-manager-request", {
    title: "请求拦截器",
    component: InterceptorManagerWindow,
    props: { kind: "request" },
    width: 980,
    height: 600,
  });
}

export function openResponseInterceptorManager(): void {
  windowsStore.open("interceptor-manager-response", {
    title: "响应拦截器",
    component: InterceptorManagerWindow,
    props: { kind: "response" },
    width: 980,
    height: 600,
  });
}

export function openColumnManager(): void {
  windowsStore.open("column-manager", {
    title: "自定义列管理器",
    component: ColumnManagerWindow,
    width: 960,
    height: 600,
  });
}

export function openFilterManager(): void {
  windowsStore.open("filter-manager", {
    title: "过滤脚本管理器",
    component: FilterManagerWindow,
    width: 1000,
    height: 600,
  });
}

export function openRoutingManager(): void {
  windowsStore.open("routing-manager", {
    title: "分流规则管理器",
    component: RoutingManagerWindow,
    width: 980,
    height: 600,
  });
}

export function openBypass(): void {
  windowsStore.open("bypass-records", {
    title: "透明转发记录",
    component: BypassWindow,
    width: 1100,
    height: 560,
  });
}

export function openArchivedSessions(): void {
  windowsStore.open("archived-sessions", {
    title: "已归档 Session",
    component: ArchivedSessionsWindow,
    width: 720,
    height: 480,
  });
}

export function openSessionExportShare(sessionId: number, sessionName: string): void {
  windowsStore.open(`session-export-share-${sessionId}`, {
    title: `导出和分享 — ${sessionName}`,
    component: SessionExportShareWindow,
    props: { sessionId },
    width: 700,
    height: 420,
  });
  window.dispatchEvent(
    new CustomEvent("proxycrab-refresh-session-share", { detail: sessionId }),
  );
}

export function openBase64(): void {
  windowsStore.open("tool-base64", {
    title: "Base64 编解码",
    component: Base64Window,
    width: 520,
    height: 460,
  });
}

export function openJwt(): void {
  windowsStore.open("tool-jwt", {
    title: "JWT 解码 / 验签",
    component: JwtWindow,
    width: 940,
    height: 640,
  });
}

export function openCertManager(): void {
  windowsStore.open("cert-manager", {
    title: "证书管理",
    component: CertManagerWindow,
    width: 640,
    height: 520,
  });
}

export function openSystemLogs(): void {
  windowsStore.open("system-logs", {
    title: "系统日志",
    component: SystemLogsWindow,
    width: 720,
    height: 480,
  });
}

export function openApprovals(): void {
  windowsStore.open("http-approvals", {
    title: "管理接口审批",
    component: ApprovalWindow,
    width: 760,
    height: 520,
  });
}

export function openSettings(): void {
  windowsStore.open("settings", {
    title: "设置",
    component: SettingsWindow,
    width: 900,
    height: 640,
  });
}

export function openSkillManager(): void {
  windowsStore.open("skill-manager", {
    title: "ProxyCrab Skill 管理",
    component: SkillManagerWindow,
    width: 680,
    height: 480,
  });
}

export function openAgentsPresets(): void {
  windowsStore.open("agents-presets", {
    title: "AGENTS.md 预设",
    component: AgentsPresetsWindow,
    width: 980,
    height: 620,
  });
}
