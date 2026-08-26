<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, watch } from "vue";
import { Io5Refresh, Io5Trash } from "vue-icons-plus/io5";
import type { BypassEntry, HttpApiChange } from "../api/types";
import VirtualList from "../components/VirtualList.vue";
import { appStore } from "../stores/app";
import { bypassStore } from "../stores/bypass";
import { confirmDialog } from "../stores/dialog";
import { proxyStore } from "../stores/proxy";
import { HTTP_API_CHANGE_EVENT } from "../stores/http-api-sync";
import { isStaleByProxyRun } from "../utils/capture-outcome";
import { formatDateTime } from "../utils/format";

let timer: number | undefined;

function isStale(entry: BypassEntry): boolean {
  return isStaleByProxyRun(entry.outcome, entry.created_at, proxyStore.status);
}

function isDeletable(entry: BypassEntry): boolean {
  return entry.outcome !== "in_progress" || isStale(entry);
}

const selectedDeletableCount = computed(
  () =>
    [...bypassStore.selectedIds].filter(
      (id) => {
        const row = bypassStore.rows.find((entry) => entry.id === id);
        return row !== undefined && isDeletable(row);
      },
    ).length,
);

function outcomeLabel(entry: BypassEntry): string {
  if (isStale(entry)) return "已失效";
  return entry.outcome === "in_progress"
    ? "转发中"
    : entry.outcome === "success"
      ? "完成"
      : "失败";
}

function reasonLabel(reason: string): string {
  const labels: Record<string, string> = {
    no_active_session: "没有活跃会话",
    script_bypass: "分流规则旁路",
    routing_script_error: "规则执行失败",
    session_pin_failed: "Session 不可用",
  };
  return labels[reason] ?? reason;
}

function bytes(value: number | null): string {
  if (value === null) return "—";
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  return `${(value / 1024 / 1024).toFixed(1)} MB`;
}

async function removeSelected(): Promise<void> {
  if (!selectedDeletableCount.value) return;
  const ok = await confirmDialog({
    title: "删除透明转发记录",
    message: `确定删除选中的 ${selectedDeletableCount.value} 条记录吗？`,
    confirmText: "删除",
    danger: true,
  });
  if (ok && (await bypassStore.removeSelected())) {
    appStore.toast("记录已删除", "success");
  }
}

async function clearTerminal(): Promise<void> {
  const ok = await confirmDialog({
    title: "清空透明转发记录",
    message: "确定清空全部已完成、失败和已失效的记录吗？当前正在转发的记录会保留。",
    confirmText: "清空",
    danger: true,
  });
  if (!ok) return;
  const deleted = await bypassStore.clearTerminal();
  appStore.toast(`已清空 ${deleted} 条记录`, "success");
}

function onHttpApiChange(event: Event): void {
  const resources = (event as CustomEvent<HttpApiChange>).detail.resources;
  if (resources.includes("all") || resources.includes("bypass")) {
    void bypassStore.refresh();
  }
}

onMounted(() => {
  void bypassStore.refresh();
  window.addEventListener(HTTP_API_CHANGE_EVENT, onHttpApiChange);
  timer = window.setInterval(() => {
    if (proxyStore.running) void bypassStore.refreshNewest();
  }, 1500);
});

watch(
  () => proxyStore.running,
  () => void bypassStore.refreshNewest(),
);

onBeforeUnmount(() => {
  if (timer !== undefined) window.clearInterval(timer);
  window.removeEventListener(HTTP_API_CHANGE_EVENT, onHttpApiChange);
});
</script>

<template>
  <div class="bp-root">
    <div class="bp-toolbar">
      <span class="text-secondary">透明转发的网络请求不会被任何session捕获</span>
      <span class="text-faint">{{ bypassStore.rows.length }} 条已加载</span>
      <span class="bp-spacer" />
      <button class="btn icon" title="刷新" @click="bypassStore.refresh">
        <Io5Refresh :size="14" />
      </button>
      <button
        class="btn"
        v-if="selectedDeletableCount > 0"
        @click="removeSelected"
      >
        删除选中
      </button>
      <button class="btn danger" @click="clearTerminal">
        <Io5Trash :size="14" />清空
      </button>
    </div>

    <div class="bp-header bp-grid">
      <span />
      <span>时间</span>
      <span>Method</span>
      <span>URI / Authority</span>
      <span>Source</span>
      <span>原因</span>
      <span>状态</span>
      <span>响应</span>
      <span>上 / 下行</span>
    </div>
    <div v-if="!bypassStore.rows.length && !bypassStore.loading" class="empty-hint">
      暂无透明转发记录
    </div>
    <VirtualList v-else :items="bypassStore.rows" :item-height="30">
      <template #default="{ item }">
        <div class="bp-row bp-grid" :title="(item as BypassEntry).error ?? ''">
          <input
            type="checkbox"
            :checked="bypassStore.selectedIds.has((item as BypassEntry).id)"
            :disabled="!isDeletable(item as BypassEntry)"
            @change="bypassStore.toggle((item as BypassEntry).id)"
          />
          <span class="text-faint">{{ formatDateTime((item as BypassEntry).created_at) }}</span>
          <span class="mono">{{ (item as BypassEntry).method }}</span>
          <span class="bp-uri mono">{{ (item as BypassEntry).uri }}</span>
          <span class="mono">{{ (item as BypassEntry).source }}</span>
          <span>{{ reasonLabel((item as BypassEntry).reason) }}</span>
          <span
            class="badge"
            :class="isStale(item as BypassEntry) ? 'outcome-stale' : `outcome-${(item as BypassEntry).outcome}`"
          >
            {{ outcomeLabel(item as BypassEntry) }}
          </span>
          <span>{{ (item as BypassEntry).response_status ?? "—" }}</span>
          <span class="mono">
            {{ bytes((item as BypassEntry).upload_bytes) }} /
            {{ bytes((item as BypassEntry).download_bytes) }}
          </span>
        </div>
      </template>
    </VirtualList>
    <button
      v-if="bypassStore.hasMore"
      class="btn bp-more"
      :disabled="bypassStore.loading"
      @click="bypassStore.loadMore"
    >
      加载更多
    </button>
  </div>
</template>

<style scoped>
.bp-root {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.bp-toolbar {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid var(--border);
}
.bp-spacer {
  flex: 1;
}
.bp-grid {
  display: grid;
  grid-template-columns: 24px 142px 74px minmax(220px, 1fr) 132px 130px 72px 54px 128px;
  align-items: center;
  gap: var(--space-2);
  min-width: 980px;
}
.bp-header {
  height: 30px;
  padding: 0 var(--space-3);
  border-bottom: 1px solid var(--border);
  background: var(--bg-panel);
  color: var(--text-secondary);
  font-size: 11px;
}
.bp-row {
  height: 30px;
  padding: 0 var(--space-3);
  border-bottom: 1px solid var(--border);
  font-size: 11px;
}
.bp-row:nth-child(even) {
  background: var(--stripe);
}
.bp-row > span {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.bp-uri {
  color: var(--text-secondary);
}
.outcome-in_progress {
  color: var(--accent);
}
.outcome-success {
  color: var(--success);
}
.outcome-failed {
  color: var(--danger);
}
.outcome-stale {
  color: var(--text-faint);
}
.bp-more {
  align-self: center;
  margin: var(--space-2);
}
</style>
