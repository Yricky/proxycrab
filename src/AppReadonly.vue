<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import FilterBar from "./components/FilterBar.vue";
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
import { useBackend } from "./api";

const props = defineProps<{ focusLogId?: number }>();
const backend = useBackend();

// 只读分享页同样自持一份日志视图（会话来自分享 bootstrap）。
const logs = createLogsStore();
const logTable = ref<InstanceType<typeof LogTable> | null>(null);

// 分享链接带 id 参数时，等日志加载出目标记录后聚焦一次（滚动 + 打开底部详情）。
// 找不到（id 非法 / 不存在 / 被过滤隐藏）则静默忽略。
let focused = false;
watch(
  [logTable, () => logs.loading, () => logs.sortedIds.length],
  () => {
    if (focused || props.focusLogId === undefined || logs.loading) return;
    if (logTable.value?.focusLog(props.focusLogId)) focused = true;
  },
  { immediate: true },
);

// 只读分享页只展示单个 Session 的日志，无断点/审批能力；日志 store 会根据
// 活跃/待复算状态动态降频或停止轮询。
function syncCapturePolling(): void {
  const canPoll = proxyStore.running && logs.sessionId !== null;
  if (canPoll) {
    logs.startPolling();
  } else {
    logs.stopPolling();
  }
}

watch(
  [
    () => proxyStore.running,
    () => sessionsStore.activeSessionId,
    () => logs.sessionId,
  ],
  () => {
    syncCapturePolling();
  },
);

onMounted(async () => {
  await startHttpApiSync();
  await sessionsStore.init();
  logs.view(sessionsStore.sessions[0]?.id ?? null);
  // 确保启动时状态已刷新，避免 syncCapturePolling 拿到过期的 stopped 状态。
  await proxyStore.refresh();
  syncCapturePolling();
});

onBeforeUnmount(() => {
  proxyStore.dispose();
  logs.dispose();
  stopHttpApiSync();
});
</script>

<template>
  <div class="app-shell">
    <div class="app-main">
      <section class="app-content">
        <div class="traffic-controls">
          <FilterBar
            :filter="logs.appliedFilter"
            :session-id="logs.sessionId"
            :syncing="backend.capabilities.filterPersistence"
            :apply-filter="(filter) => logs.applyFilter(filter)"
          />
        </div>
        <LogTable ref="logTable" :logs="logs" />
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
  gap: 6px;
  padding: 6px 8px;
  border-bottom: 1px solid var(--border);
  background: color-mix(in srgb, var(--bg-panel) 72%, var(--bg-app));
}
</style>
