<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import { openBundleImport } from "./windows/launcher";
import { reportError } from "./stores/app";
import AppToolbar from "./components/AppToolbar.vue";
import SessionSidebar from "./components/SessionSidebar.vue";
import FilterBar from "./components/FilterBar.vue";
import InterceptorPipeline from "./components/InterceptorPipeline.vue";
import LogTable from "./components/LogTable.vue";
import FloatingWindow from "./components/FloatingWindow.vue";
import Toast from "./components/Toast.vue";
import ConfirmDialog from "./components/ConfirmDialog.vue";
import ContextMenu from "./components/ContextMenu.vue";
import { windowsStore } from "./stores/windows";
import { sessionsStore } from "./stores/sessions";
import { proxyStore } from "./stores/proxy";
import { createLogsStore } from "./stores/logs";
import { startHttpApiSync, stopHttpApiSync } from "./stores/http-api-sync";
import { breakpointsStore } from "./stores/breakpoints";
import { approvalsStore } from "./stores/approvals";
import { skillStore } from "./stores/skill";
import { useBackend } from "./api";

const backend = useBackend();

// 主内容区的日志视图：只在这里创建，由 AppMain 持有并交给子组件；
// "查看哪个会话" 就是 logs.sessionId（不再有全局 viewingSessionId）。
const logs = createLogsStore();

// 日志轮询由 store 根据当前 Session 的活跃/待复算状态动态降频或停止；
// 断点轮询只对正在运行的活跃 Session 有意义。
function syncCapturePolling(): void {
  const canPollLogs = proxyStore.running && logs.sessionId !== null;
  if (canPollLogs) {
    logs.startPolling();
  } else {
    logs.stopPolling();
  }
  const canPollBreakpoints = canPollLogs && sessionsStore.activeSessionId !== null;
  if (canPollBreakpoints) {
    breakpointsStore.startPolling({
      sessionId: () => logs.sessionId,
      hasActive: () => logs.hasActive,
    });
  } else {
    breakpointsStore.stopPolling();
  }
}

watch([() => proxyStore.running, () => sessionsStore.activeSessionId, () => logs.sessionId], () => {
  syncCapturePolling();
});

// 正在查看的会话失效（被归档/删除）时，回落到活跃会话，再不行取第一个。
watch(
  () => sessionsStore.sessions,
  (sessions) => {
    if (logs.sessionId !== null && sessions.some((s) => s.id === logs.sessionId)) return;
    const fallback = sessions.find((s) => s.id === sessionsStore.activeSessionId) ?? sessions[0];
    logs.view(fallback?.id ?? null);
  },
  { deep: true },
);

onMounted(async () => {
  window.addEventListener("proxycrab-open-session", onOpenSession);
  await startHttpApiSync();
  if (backend.listHttpApprovals) await approvalsStore.start();
  void skillStore.refresh();
  await proxyStore.init();
  await sessionsStore.init();
  const initial = sessionsStore.sessions.find(
    (session) => session.id === sessionsStore.activeSessionId,
  );
  logs.view(initial?.id ?? sessionsStore.sessions[0]?.id ?? null);
  // 确保启动时状态已刷新，避免 syncCapturePolling 拿到过期的 stopped 状态。
  await proxyStore.refresh();
  syncCapturePolling();
});

onBeforeUnmount(() => {
  window.removeEventListener("proxycrab-open-session", onOpenSession);
  proxyStore.dispose();
  logs.dispose();
  breakpointsStore.stopPolling();
  stopHttpApiSync();
  if (backend.listHttpApprovals) approvalsStore.stop();
});
// 拖放导入：dragenter/dragleave 计数决定覆盖层，避免在子元素间移动时闪烁。
const draggingFiles = ref(false);
let dragDepth = 0;
function carriesFiles(event: DragEvent): boolean {
  return Array.from(event.dataTransfer?.types ?? []).includes("Files");
}
function fileDragEnter(event: DragEvent): void {
  if (!backend.importBundle || !carriesFiles(event)) return;
  dragDepth++;
  draggingFiles.value = true;
}
function fileDragLeave(): void {
  if (!draggingFiles.value) return;
  dragDepth = Math.max(0, dragDepth - 1);
  if (dragDepth === 0) draggingFiles.value = false;
}
function fileDrag(event: DragEvent): void {
  if (backend.importBundle && carriesFiles(event)) event.preventDefault();
}
async function fileDrop(event: DragEvent): Promise<void> {
  dragDepth = 0;
  draggingFiles.value = false;
  if (!backend.importBundle || !event.dataTransfer?.files.length) return;
  event.preventDefault();
  event.stopPropagation();
  for (const file of Array.from(event.dataTransfer.files)) {
    try {
      await openBundleImport(file);
    } catch (e) {
      reportError(e, "打开分享包失败");
    }
  }
}
function onOpenSession(event: Event): void {
  const sessionId = (event as CustomEvent<number>).detail;
  if (typeof sessionId === "number") logs.view(sessionId);
}
</script>

<template>
  <div
    class="app-shell"
    @dragenter="fileDragEnter"
    @dragover="fileDrag"
    @dragleave="fileDragLeave"
    @drop="fileDrop"
  >
    <AppToolbar :session-id="logs.sessionId" />
    <div class="app-main">
      <SessionSidebar :logs="logs" />
      <section class="app-content">
        <div class="traffic-controls">
          <FilterBar
            :filter="logs.appliedFilter"
            :session-id="logs.sessionId"
            :syncing="Boolean(backend.replaceSessionFilter)"
            :apply-filter="(filter) => logs.applyFilter(filter)"
          />
          <InterceptorPipeline :logs="logs" />
        </div>
        <LogTable :logs="logs" />
      </section>
    </div>

    <div v-if="draggingFiles" class="drop-overlay">
      <span class="drop-hint">松开以导入分享包</span>
    </div>

    <FloatingWindow v-for="win in windowsStore.windows" :key="win.id" :win="win" />
    <Toast />
    <ConfirmDialog />
    <ContextMenu />
  </div>
</template>

<style scoped>
.app-shell {
  height: 100%;
  display: flex;
  flex-direction: column;
}
.app-main {
  flex: 1;
  min-height: 0;
  display: flex;
}
.app-content {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  background: var(--bg-app);
}
.traffic-controls {
  flex: none;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 6px 8px;
  border-bottom: 1px solid var(--border);
  background: color-mix(in srgb, var(--bg-panel) 72%, var(--bg-app));
}
.drop-overlay {
  position: fixed;
  inset: 0;
  z-index: 1000;
  display: flex;
  align-items: center;
  justify-content: center;
  background: color-mix(in srgb, var(--bg-app) 65%, transparent);
  pointer-events: none;
}
.drop-overlay::before {
  content: "";
  position: absolute;
  inset: 10px;
  border: 2px dashed var(--accent);
  border-radius: var(--radius-lg);
  background: color-mix(in srgb, var(--accent) 7%, transparent);
}
.drop-hint {
  position: relative;
  padding: 8px 16px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-md);
  background: var(--bg-elevated);
  box-shadow: var(--shadow-popup);
  font-size: 13px;
}
</style>
