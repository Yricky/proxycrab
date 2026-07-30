<script setup lang="ts">
import { onBeforeUnmount, onMounted } from "vue";
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

onMounted(async () => {
  await startHttpApiSync();
  proxyStore.startPolling();
  await sessionsStore.init();
  logsStore.startPolling();
});

onBeforeUnmount(() => {
  proxyStore.stopPolling();
  logsStore.stopPolling();
  stopHttpApiSync();
});
</script>

<template>
  <div class="app-shell">
    <AppToolbar />
    <div class="app-main">
      <SessionSidebar />
      <section class="app-content">
        <InterceptorPipeline />
        <FilterBar />
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
</style>
