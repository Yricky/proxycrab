// Central launchers for floating windows. Keeping component imports here
// avoids spreading window ids / default sizes across the UI code.
import { windowsStore } from "../stores/windows";
import type { InterceptorExecution, InterceptorKind } from "../api/types";
import LogDetailWindow from "./LogDetailWindow.vue";
import ScriptEditorWindow from "./ScriptEditorWindow.vue";
import InterceptorManagerWindow from "./InterceptorManagerWindow.vue";
import ColumnManagerWindow from "./ColumnManagerWindow.vue";
import FilterManagerWindow from "./FilterManagerWindow.vue";
import Base64Window from "./Base64Window.vue";
import CertManagerWindow from "./CertManagerWindow.vue";
import SystemLogsWindow from "./SystemLogsWindow.vue";
import SettingsWindow from "./SettingsWindow.vue";
import ScriptSnapshotWindow from "./ScriptSnapshotWindow.vue";
import SkillInstallWindow from "./SkillInstallWindow.vue";
import RoutingManagerWindow from "./RoutingManagerWindow.vue";
import BypassWindow from "./BypassWindow.vue";
import BreakpointListWindow from "./BreakpointListWindow.vue";

export type ScriptEditorKind = "column" | InterceptorKind;

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

export function openScriptEditor(kind: ScriptEditorKind, name: string): void {
  const kindLabel =
    kind === "column" ? "列脚本" : kind === "request" ? "请求拦截器" : "响应拦截器";
  windowsStore.open(`script-${kind}-${name}`, {
    title: `${name} — ${kindLabel}`,
    component: ScriptEditorWindow,
    props: { kind, name },
    width: 720,
    height: 520,
  });
}

export function openScriptSnapshot(
  sessionId: number,
  logId: number,
  execution: InterceptorExecution,
): void {
  windowsStore.open(
    `script-snapshot-${sessionId}-${logId}-${execution.execution_id}`,
    {
      title: `${execution.name} — 历史脚本`,
      component: ScriptSnapshotWindow,
      props: { execution },
      width: 720,
      height: 520,
    },
  );
}

export function openInterceptorManager(): void {
  windowsStore.open("interceptor-manager", {
    title: "拦截器管理器",
    component: InterceptorManagerWindow,
    width: 640,
    height: 440,
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

export function openBase64(): void {
  windowsStore.open("tool-base64", {
    title: "Base64 编解码",
    component: Base64Window,
    width: 520,
    height: 460,
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

export function openSettings(): void {
  windowsStore.open("settings", {
    title: "设置",
    component: SettingsWindow,
    width: 560,
    height: 440,
  });
}

export function openSkillInstall(): void {
  windowsStore.open("skill-install", {
    title: "安装 ProxyCrab Skill",
    component: SkillInstallWindow,
    width: 560,
    height: 360,
  });
}
