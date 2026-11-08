<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { useBackend } from "../api";
import type { InterceptorKind } from "../api/generated/InterceptorKind";
import { appStore, reportError } from "../stores/app";
import { breakpointsStore } from "../stores/breakpoints";
import { t } from "../i18n";
import { openBreakpointDetail } from "./launcher";

const props = defineProps<{
  sessionId: number;
  phase: InterceptorKind;
  interceptorName: string;
  displayName?: string;
}>();

const backend = useBackend();
const releasingIds = reactive(new Set<number>());
const releasingAll = ref(false);

const items = computed(() =>
  breakpointsStore.items.filter(
    (item) =>
      item.session_id === props.sessionId &&
      item.phase === props.phase &&
      item.interceptor_name === props.interceptorName,
  ),
);

function phaseLabel(phase: InterceptorKind): string {
  return phase === "request" ? t("请求") : t("响应");
}

function remainingLabel(milliseconds: number): string {
  return t("{n} 秒", Math.max(0, Math.ceil(milliseconds / 1000)));
}

function removeReleased(ids: Set<number>): void {
  breakpointsStore.items = breakpointsStore.items.filter((item) => !ids.has(item.id));
}

async function releaseBreakpoint(id: number): Promise<void> {
  if (releasingAll.value || releasingIds.has(id)) return;
  releasingIds.add(id);
  try {
    await backend.releaseBreakpoint(id);
    removeReleased(new Set([id]));
    appStore.toast(t("断点已放行"), "success");
  } catch (error) {
    reportError(error, t("放行断点失败"));
  } finally {
    releasingIds.delete(id);
    void breakpointsStore.refresh();
  }
}

async function releaseAll(): Promise<void> {
  if (releasingAll.value || releasingIds.size > 0 || items.value.length === 0) return;
  const snapshot = [...items.value];
  releasingAll.value = true;
  try {
    const results = await Promise.allSettled(
      snapshot.map((item) => backend.releaseBreakpoint(item.id)),
    );
    const released = new Set(
      snapshot.filter((_, index) => results[index]?.status === "fulfilled").map((item) => item.id),
    );
    removeReleased(released);
    const failed = snapshot.length - released.size;
    if (failed === 0) {
      appStore.toast(t("已放行 {n} 个断点", released.size), "success");
    } else {
      appStore.toast(
        t("已放行 {n} 个断点，{m} 个放行失败", { n: released.size, m: failed }),
        "error",
        5000,
      );
    }
  } finally {
    releasingAll.value = false;
    void breakpointsStore.refresh();
  }
}

onMounted(() => void breakpointsStore.refresh());
</script>

<template>
  <div class="breakpoint-list">
    <header class="list-head">
      <div>
        <strong class="mono" :title="interceptorName">{{ displayName ?? interceptorName }}</strong>
        <span>{{ t("{phase}拦截器", { phase: phaseLabel(phase) }) }}</span>
      </div>
      <div class="list-actions">
        <span class="count">{{ t("{n} 个断点", items.length) }}</span>
        <button
          class="btn release-all"
          :disabled="items.length === 0 || releasingAll || releasingIds.size > 0"
          @click="releaseAll"
        >
          {{ releasingAll ? t("放行中…") : t("放行全部") }}
        </button>
      </div>
    </header>
    <div v-if="items.length === 0" class="empty">{{ t("当前没有等待中的断点") }}</div>
    <div v-else class="rows">
      <div
        v-for="item in items"
        :key="item.id"
        class="row"
        role="button"
        tabindex="0"
        @click="openBreakpointDetail(item.id, item.capture_id)"
        @keydown.enter="openBreakpointDetail(item.id, item.capture_id)"
        @keydown.space.prevent="openBreakpointDetail(item.id, item.capture_id)"
      >
        <span class="method mono">{{ item.method }}</span>
        <span class="uri mono" :title="item.uri">{{ item.uri }}</span>
        <span class="position">#{{ item.position + 1 }}</span>
        <span class="remaining">{{ remainingLabel(item.remaining_ms) }}</span>
        <button
          class="btn release-one"
          :disabled="releasingAll || releasingIds.has(item.id)"
          @pointerdown.stop
          @keydown.stop
          @click.stop="releaseBreakpoint(item.id)"
        >
          {{ releasingIds.has(item.id) ? t("放行中…") : t("放行") }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.breakpoint-list {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.list-head {
  flex: none;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 8px;
  border-bottom: 1px solid var(--border);
  background: var(--bg-panel);
}
.list-head div {
  display: flex;
  align-items: baseline;
  gap: 8px;
}
.list-head span,
.count {
  color: var(--text-secondary);
  font-size: 11px;
}
.list-head .list-actions {
  align-items: center;
}
.release-all,
.release-one {
  min-height: 24px;
  padding: 2px 9px;
  font-size: 11px;
}
.rows {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: 8px;
}
.row {
  width: 100%;
  display: grid;
  grid-template-columns: 64px minmax(0, 1fr) 42px 64px auto;
  align-items: center;
  gap: 8px;
  border: 0;
  border-radius: var(--radius-md);
  padding: 6px 8px 6px 10px;
  background: transparent;
  color: var(--text);
  text-align: left;
  cursor: pointer;
}
.row:hover {
  background: var(--bg-hover);
}
.row:focus-visible {
  outline: 1px solid var(--accent);
  outline-offset: -1px;
}
.method {
  color: var(--warning);
  font-weight: 700;
}
.uri {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.position {
  color: var(--text-faint);
  font-size: 11px;
}
.remaining {
  color: var(--warning);
  font-size: 11px;
  text-align: right;
}
.release-one {
  justify-self: end;
}
.empty {
  flex: 1;
  display: grid;
  place-items: center;
  color: var(--text-faint);
  font-size: 12px;
}
</style>
