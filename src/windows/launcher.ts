import BundleImportWindow from "./BundleImportWindow.vue";
import { parseBundle, MAX_BUNDLE_BYTES } from "../utils/bundle";
import AssetManagerWindow from "./AssetManagerWindow.vue";
import HarExportWindow from "./HarExportWindow.vue";
import type { LogsStore } from "../stores/logs";
import { scriptTitle } from "../utils/script-label";
// Central launchers for floating windows. Keeping component imports here
// avoids spreading window ids / default sizes across the UI code.
import { windowsStore } from "../stores/windows";
import type { InterceptorExecution } from "../api/generated/InterceptorExecution";
import type { InterceptorKind } from "../api/generated/InterceptorKind";
import LogDetailWindow from "./LogDetailWindow.vue";
import InterceptorManagerWindow from "./InterceptorManagerWindow.vue";
import ColumnManagerWindow from "./ColumnManagerWindow.vue";
import RedactionManagerWindow from "./RedactionManagerWindow.vue";
import FilterManagerWindow from "./FilterManagerWindow.vue";
import Base64Window from "./Base64Window.vue";
import JwtWindow from "./JwtWindow.vue";
import QrCodeWindow from "./QrCodeWindow.vue";
import TextDiffWindow from "./TextDiffWindow.vue";
import CertManagerWindow from "./CertManagerWindow.vue";
import SystemLogsWindow from "./SystemLogsWindow.vue";
import SettingsWindow from "./SettingsWindow.vue";
import AboutWindow from "./AboutWindow.vue";
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
import InterceptorSnapshotWindow from "./InterceptorSnapshotWindow.vue";
import ReplayWindow from "./ReplayWindow.vue";
import type { ReplayDraft } from "../utils/replay";
import { getBackend } from "../api/runtime-backend";
import { t } from "../i18n";

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

let replayWindowId = 0;

export function openReplay(prefill?: ReplayDraft): void {
  // 后端没有 replay 接口时，任何入口都不应打开发送请求窗口。
  if (!getBackend().replay) return;
  windowsStore.open(`replay-${++replayWindowId}`, {
    title: t("发送请求"),
    component: ReplayWindow,
    props: { prefill },
    width: 920,
    height: 640,
  });
}

export function openLogDetail(sessionId: number, logId: number): void {
  windowsStore.open(`detail-${sessionId}-${logId}`, {
    title: t("#{id} 详情", { id: logId }),
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
  displayName = interceptorName,
): void {
  windowsStore.open(`breakpoints-${sessionId}-${phase}-${interceptorName}`, {
    title: t("{name} — 断点列表", { name: displayName }),
    component: BreakpointListWindow,
    props: { sessionId, phase, interceptorName, displayName },
    width: 680,
    height: 420,
  });
}

export function openBreakpointDetail(breakpointId: number, captureId: number): void {
  windowsStore.open(`breakpoint-${breakpointId}`, {
    title: t("#{id} — 断点详情", { id: captureId }),
    component: LogDetailWindow,
    props: { breakpointId },
    width: 900,
    height: 650,
  });
}

export function openScriptEditor(kind: InterceptorKind, name: string): void {
  const kindLabel = kind === "request" ? t("请求拦截器") : t("响应拦截器");
  windowsStore.open(`interceptor-manager-${kind}`, {
    title: kindLabel,
    component: InterceptorManagerWindow,
    props: { kind, initialName: name },
    width: 980,
    height: 600,
  });
  // Fresh windows pick the script via initialName; already-open ones react to this event.
  window.dispatchEvent(
    new CustomEvent("proxycrab-focus-script", {
      detail: { scope: kind, name },
    }),
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

export function openInterceptorSnapshot(
  sessionId: number,
  logId: number,
  execution: InterceptorExecution,
): void {
  windowsStore.open(`interceptor-snapshot-${sessionId}-${logId}-${execution.execution_id}`, {
    title: t("{name} — 进入拦截器前的快照", { name: scriptTitle(execution) }),
    component: InterceptorSnapshotWindow,
    props: { sessionId, logId, execution },
    width: 820,
    height: 600,
  });
}

export function openModificationValue(id: string, label: string, value: string): void {
  openReadonlyViewer(`mod-value-${id}`, t("{name} — 完整内容", { name: label }), value);
}

export function openRequestInterceptorManager(): void {
  windowsStore.open("interceptor-manager-request", {
    title: t("请求拦截器"),
    component: InterceptorManagerWindow,
    props: { kind: "request" },
    width: 980,
    height: 600,
  });
}

export function openResponseInterceptorManager(): void {
  windowsStore.open("interceptor-manager-response", {
    title: t("响应拦截器"),
    component: InterceptorManagerWindow,
    props: { kind: "response" },
    width: 980,
    height: 600,
  });
}

export function openColumnManager(initialName?: string, sessionId: number | null = null): void {
  windowsStore.open("column-manager", {
    title: t("自定义列管理器"),
    component: ColumnManagerWindow,
    props: { initialName, sessionId },
    width: 960,
    height: 600,
  });
  if (initialName) {
    window.dispatchEvent(
      new CustomEvent("proxycrab-focus-script", {
        detail: { scope: "column", name: initialName },
      }),
    );
  }
}

export function openFilterManager(sessionId: number | null = null): void {
  windowsStore.open("filter-manager", {
    title: t("过滤脚本管理器"),
    component: FilterManagerWindow,
    props: { sessionId },
    width: 1000,
    height: 600,
  });
}

export function openRoutingManager(): void {
  windowsStore.open("routing-manager", {
    title: t("分流规则管理器"),
    component: RoutingManagerWindow,
    width: 980,
    height: 600,
  });
}

export function openBypass(): void {
  windowsStore.open("bypass-records", {
    title: t("透明转发记录"),
    component: BypassWindow,
    width: 1100,
    height: 560,
  });
}

export function openArchivedSessions(): void {
  windowsStore.open("archived-sessions", {
    title: t("已归档 Session"),
    component: ArchivedSessionsWindow,
    width: 720,
    height: 480,
  });
}

export function openSessionExportShare(sessionId: number, sessionName: string): void {
  windowsStore.open(`session-export-share-${sessionId}`, {
    title: t("链接分享 — {name}", { name: sessionName }),
    component: SessionExportShareWindow,
    props: { sessionId },
    width: 700,
    height: 420,
  });
  window.dispatchEvent(new CustomEvent("proxycrab-refresh-session-share", { detail: sessionId }));
}

export function openBase64(): void {
  windowsStore.open("tool-base64", {
    title: t("Base64 编解码"),
    component: Base64Window,
    width: 520,
    height: 460,
  });
}

export function openJwt(): void {
  windowsStore.open("tool-jwt", {
    title: t("JWT 解码 / 验签"),
    component: JwtWindow,
    width: 940,
    height: 640,
  });
}

export function openQrCode(): void {
  windowsStore.open("tool-qrcode", {
    title: t("二维码生成"),
    component: QrCodeWindow,
    width: 520,
    height: 560,
  });
}

export function openTextDiff(): void {
  windowsStore.open("tool-text-diff", {
    title: t("文本比对"),
    component: TextDiffWindow,
    width: 940,
    height: 640,
  });
}

export function openCertManager(): void {
  windowsStore.open("cert-manager", {
    title: t("如何连接到ProxyCrab抓包"),
    component: CertManagerWindow,
    width: 640,
    height: 600,
  });
}

export function openSystemLogs(): void {
  windowsStore.open("system-logs", {
    title: t("系统日志"),
    component: SystemLogsWindow,
    width: 720,
    height: 480,
  });
}

export function openApprovals(): void {
  windowsStore.open("http-approvals", {
    title: t("管理接口审批"),
    component: ApprovalWindow,
    width: 760,
    height: 520,
  });
}

export function openSettings(): void {
  windowsStore.open("settings", {
    title: t("设置"),
    component: SettingsWindow,
    width: 900,
    height: 640,
  });
}

export function openAbout(): void {
  windowsStore.open("about", {
    title: t("关于 ProxyCrab"),
    component: AboutWindow,
    width: 900,
    height: 620,
  });
}

export function openSkillManager(): void {
  windowsStore.open("skill-manager", {
    title: t("ProxyCrab Skill 管理"),
    component: SkillManagerWindow,
    width: 680,
    height: 480,
  });
}

export function openAgentsPresets(): void {
  windowsStore.open("agents-presets", {
    title: t("AGENTS.md 预设"),
    component: AgentsPresetsWindow,
    width: 980,
    height: 620,
  });
}

export function openRedactionManager(): void {
  windowsStore.open("redaction-manager", {
    title: t("脱敏脚本管理器"),
    component: RedactionManagerWindow,
    width: 1000,
    height: 600,
  });
}

/** A single export window follows the list until the user starts a frozen export. */
export function openHarExport(logs: LogsStore): void {
  const backend = getBackend();
  if (!backend.getLog || !backend.fetchBody || !backend.saveFile) return;
  windowsStore.open("har-export", {
    title: t("导出 HAR"),
    component: HarExportWindow,
    props: { logs },
    width: 720,
    height: 460,
  });
}

export function openAssetManager(): void {
  if (!getBackend().listAssets) return;
  windowsStore.open("asset-manager", {
    title: t("工作区资源"),
    component: AssetManagerWindow,
    width: 760,
    height: 520,
  });
}

let bundleWindowId = 0;
export async function openBundleImport(file: File): Promise<void> {
  if (!getBackend().importBundle || !getBackend().resolveBundle) return;
  if (file.size > MAX_BUNDLE_BYTES) throw new Error(t("分享包超过 128 MiB"));
  const preview = parseBundle(new Uint8Array(await file.arrayBuffer()));
  const id = `bundle-import-${++bundleWindowId}`;
  windowsStore.open(id, {
    title: t("导入分享包"),
    component: BundleImportWindow,
    props: { file, preview, windowId: id },
    width: 680,
    height: 460,
  });
}
