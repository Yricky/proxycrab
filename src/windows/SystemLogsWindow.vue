<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { Io5Refresh, Io5Trash } from "vue-icons-plus/io5";
import { useBackend } from "../api";
import type { SystemLogEntry } from "../api/types";
import VirtualList from "../components/VirtualList.vue";
import { reportError } from "../stores/app";
import { confirmDialog } from "../stores/dialog";
import { formatDateTime } from "../utils/format";

const backend = useBackend();

const LEVELS = ["全部", "ERROR", "WARN", "INFO", "DEBUG"] as const;
const MAX_ENTRIES = 5000;

const entries = ref<SystemLogEntry[]>([]);
const levelFilter = ref<(typeof LEVELS)[number]>("全部");
const autoRefresh = ref(true);

let timer: number | undefined;

const filteredEntries = computed(() =>
  levelFilter.value === "全部"
    ? entries.value
    : entries.value.filter((e) => e.level === levelFilter.value),
);

function appendEntries(batch: SystemLogEntry[]): void {
  if (!batch.length) return;
  entries.value = [...entries.value, ...batch];
  if (entries.value.length > MAX_ENTRIES) {
    entries.value = entries.value.slice(entries.value.length - MAX_ENTRIES);
  }
}

async function fetchLogs(): Promise<void> {
  try {
    const last = entries.value[entries.value.length - 1];
    const batch = await backend.getSystemLogs(
      last ? { after_seq: last.seq, limit: 1000 } : { limit: 1000 },
    );
    appendEntries(batch);
  } catch (error) {
    reportError(error, "获取系统日志失败");
  }
}

async function clearLogs(): Promise<void> {
  const ok = await confirmDialog({
    title: "清空系统日志",
    message: "确定清空所有系统日志吗？该操作不可撤销。",
    confirmText: "清空",
    danger: true,
  });
  if (!ok) return;
  try {
    await backend.clearSystemLogs();
    entries.value = [];
  } catch (error) {
    reportError(error, "清空系统日志失败");
  }
}

function levelClass(level: string): string {
  switch (level) {
    case "ERROR":
      return "lv-error";
    case "WARN":
      return "lv-warn";
    case "INFO":
      return "lv-info";
    default:
      return "lv-debug";
  }
}

onMounted(() => {
  void fetchLogs();
  timer = window.setInterval(() => {
    if (autoRefresh.value) void fetchLogs();
  }, 2000);
});

onUnmounted(() => {
  if (timer !== undefined) window.clearInterval(timer);
});
</script>

<template>
  <div class="sl-root">
    <div class="sl-toolbar">
      <select v-model="levelFilter" class="select" title="按级别过滤">
        <option v-for="lv in LEVELS" :key="lv" :value="lv">{{ lv }}</option>
      </select>
      <label class="sl-auto">
        <button
          class="switch"
          :class="{ on: autoRefresh }"
          title="自动刷新"
          @click="autoRefresh = !autoRefresh"
        />
        <span class="text-secondary">自动刷新</span>
      </label>
      <span class="sl-spacer" />
      <button class="btn icon" title="刷新" @click="fetchLogs">
        <Io5Refresh :size="14" />
      </button>
      <button class="btn icon sl-danger" title="清空日志" @click="clearLogs">
        <Io5Trash :size="14" />
      </button>
    </div>

    <div v-if="!filteredEntries.length" class="empty-hint">暂无日志</div>
    <VirtualList v-else :items="filteredEntries" :item-height="24">
      <template #default="{ item }">
        <div class="sl-row mono">
          <span class="sl-time text-faint">{{ formatDateTime((item as SystemLogEntry).timestamp) }}</span>
          <span class="badge sl-level" :class="levelClass((item as SystemLogEntry).level)">
            {{ (item as SystemLogEntry).level }}
          </span>
          <span class="sl-msg" :title="(item as SystemLogEntry).message">
            {{ (item as SystemLogEntry).message }}
          </span>
        </div>
      </template>
    </VirtualList>
  </div>
</template>

<style scoped>
.sl-root {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.sl-toolbar {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid var(--border);
  flex: none;
}

.sl-auto {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  font-size: 12px;
}

.sl-spacer {
  flex: 1;
}

.sl-danger:hover:not(:disabled) {
  color: var(--danger);
}

.sl-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  height: 24px;
  padding: 0 var(--space-3);
  overflow: hidden;
}

.sl-time {
  flex: none;
}

.sl-level {
  flex: none;
  min-width: 44px;
  text-align: center;
}
.sl-level.lv-error {
  background: var(--danger);
  color: #fff;
}
.sl-level.lv-warn {
  background: var(--warning);
  color: #fff;
}
.sl-level.lv-info {
  background: var(--accent);
  color: #fff;
}
.sl-level.lv-debug {
  background: var(--bg-active);
  color: var(--text-faint);
}

.sl-msg {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
