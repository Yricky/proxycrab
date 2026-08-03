<script setup lang="ts">
import { computed, onMounted } from "vue";
import type { InterceptorKind } from "../api/types";
import { breakpointsStore } from "../stores/breakpoints";
import { openBreakpointDetail } from "./launcher";

const props = defineProps<{
  sessionId: number;
  phase: InterceptorKind;
  interceptorName: string;
}>();

const items = computed(() =>
  breakpointsStore.items.filter(
    (item) =>
      item.session_id === props.sessionId &&
      item.phase === props.phase &&
      item.interceptor_name === props.interceptorName,
  ),
);

function phaseLabel(phase: InterceptorKind): string {
  return phase === "request" ? "请求" : "响应";
}

function remainingLabel(milliseconds: number): string {
  return `${Math.max(0, Math.ceil(milliseconds / 1000))} 秒`;
}

onMounted(() => void breakpointsStore.refresh());
</script>

<template>
  <div class="breakpoint-list">
    <header class="list-head">
      <div>
        <strong class="mono">{{ interceptorName }}</strong>
        <span>{{ phaseLabel(phase) }}拦截器</span>
      </div>
      <span class="count">{{ items.length }} 个断点</span>
    </header>
    <div v-if="items.length === 0" class="empty">当前没有等待中的断点</div>
    <div v-else class="rows">
      <button
        v-for="item in items"
        :key="item.id"
        class="row"
        @click="openBreakpointDetail(item.id, item.capture_id)"
      >
        <span class="method mono">{{ item.method }}</span>
        <span class="uri mono" :title="item.uri">{{ item.uri }}</span>
        <span class="position">#{{ item.position + 1 }}</span>
        <span class="remaining">{{ remainingLabel(item.remaining_ms) }}</span>
      </button>
    </div>
  </div>
</template>

<style scoped>
.breakpoint-list { flex: 1; min-height: 0; display: flex; flex-direction: column; }
.list-head { flex: none; display: flex; align-items: center; justify-content: space-between; padding: 12px 14px; border-bottom: 1px solid var(--border); background: var(--bg-panel); }
.list-head div { display: flex; align-items: baseline; gap: 8px; }
.list-head span, .count { color: var(--text-secondary); font-size: 11px; }
.rows { flex: 1; min-height: 0; overflow: auto; padding: 8px; }
.row { width: 100%; display: grid; grid-template-columns: 64px minmax(0, 1fr) 42px 64px; align-items: center; gap: 8px; border: 0; border-radius: var(--radius-md); padding: 9px 10px; background: transparent; color: var(--text); text-align: left; cursor: pointer; }
.row:hover { background: var(--bg-hover); }
.method { color: var(--warning); font-weight: 700; }
.uri { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.position { color: var(--text-faint); font-size: 11px; }
.remaining { color: var(--warning); font-size: 11px; text-align: right; }
.empty { flex: 1; display: grid; place-items: center; color: var(--text-faint); font-size: 12px; }
</style>
