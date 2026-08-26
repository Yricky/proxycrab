<script setup lang="ts">
import { onBeforeUnmount, onMounted, watch } from "vue";
import FilterBar from "./components/FilterBar.vue";
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

// 只读分享页只展示单个 Session 的日志，无断点/审批能力；日志 store 会根据
// 活跃/待复算状态动态降频或停止轮询。
function syncCapturePolling(): void {
  const canPoll = proxyStore.running && sessionsStore.viewingSessionId !== null;
  if (canPoll) {
    logsStore.startPolling();
  } else {
    logsStore.stopPolling();
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
  await sessionsStore.init();
  // 确保启动时状态已刷新，避免 syncCapturePolling 拿到过期的 stopped 状态。
  await proxyStore.refresh();
  syncCapturePolling();
});

onBeforeUnmount(() => {
  proxyStore.dispose();
  logsStore.stopPolling();
  stopHttpApiSync();
});
</script>

<template>
  <div class="app-shell">
    <div class="app-main">
      <section class="app-content">
        <div class="traffic-controls">
          <FilterBar />
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
