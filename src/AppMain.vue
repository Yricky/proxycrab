<script setup lang="ts">
import { onBeforeUnmount, onMounted, watch } from "vue";
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
import { logsStore } from "./stores/logs";
import { startHttpApiSync, stopHttpApiSync } from "./stores/http-api-sync";
import { breakpointsStore } from "./stores/breakpoints";
import { approvalsStore } from "./stores/approvals";
import { useBackend } from "./api";

const backend = useBackend();

// 日志与断点轮询仅在代理运行且存在活跃、正在查看的 Session 时才有意义：
// 代理停止后不会有新抓包或断点进展；无活跃 Session 时流量被直接放行（no_active_session），
// 无查看 Session 时轮询请求也只是空转。
function syncCapturePolling(): void {
  const canPoll =
    proxyStore.running &&
    sessionsStore.activeSessionId !== null &&
    sessionsStore.viewingSessionId !== null;
  if (canPoll) {
    logsStore.startPolling();
    breakpointsStore.startPolling();
  } else {
    logsStore.stopPolling();
    breakpointsStore.stopPolling();
  }
}

watch(
  [
    () => proxyStore.running,
    () => sessionsStore.activeSessionId,
    () => sessionsStore.viewingSessionId,
  ],
  () => {
    syncCapturePolling();
  },
);

onMounted(async () => {
  await startHttpApiSync();
  if (backend.capabilities.approvals) await approvalsStore.start();
  await proxyStore.init();
  await sessionsStore.init();
  // 确保启动时状态已刷新，避免 syncCapturePolling 拿到过期的 stopped 状态。
  await proxyStore.refresh();
  syncCapturePolling();
});

onBeforeUnmount(() => {
  proxyStore.dispose();
  logsStore.stopPolling();
  breakpointsStore.stopPolling();
  stopHttpApiSync();
  if (backend.capabilities.approvals) approvalsStore.stop();
});
</script>

<template>
  <div class="app-shell">
    <AppToolbar />
    <div class="app-main">
      <SessionSidebar />
      <section class="app-content">
        <div class="traffic-controls">
          <FilterBar />
          <InterceptorPipeline />
        </div>
        <LogTable />
      </section>
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
  border-bottom: 1px solid var(--border);
  background: color-mix(in srgb, var(--bg-panel) 72%, var(--bg-app));
}
</style>
