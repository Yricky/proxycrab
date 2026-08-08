<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref } from "vue";
import { Io5Checkmark, Io5Close } from "vue-icons-plus/io5";
import type { PendingApproval } from "../api/types";
import CustomSelect, { type CustomSelectOption } from "../components/CustomSelect.vue";
import { approvalsStore } from "../stores/approvals";
import { formatBytes } from "../utils/format";

const now = ref(Date.now());
const durations = reactive(new Map<number, number | null>());
const durationOptions: CustomSelectOption[] = [
  { value: "once", label: "仅本次" },
  { value: "300", label: "5 分钟" },
  { value: "1800", label: "30 分钟" },
  { value: "3600", label: "1 小时" },
];
let clock: number | undefined;

const items = computed(() =>
  [...approvalsStore.items].sort(
    (left, right) => left.deadline_at - right.deadline_at || left.id - right.id,
  ),
);

function durationFor(id: number): number | null {
  return durations.get(id) ?? null;
}

function setDuration(id: number, value: string): void {
  durations.set(id, value === "once" ? null : Number(value));
}

function remainingLabel(item: PendingApproval): string {
  return `${Math.max(0, Math.ceil((item.deadline_at - now.value) / 1000))} 秒`;
}

function requestTarget(item: PendingApproval): string {
  return item.query ? `${item.actual_path}?${item.query}` : item.actual_path;
}

function identityLabel(item: PendingApproval): string {
  return item.identity.kind === "local"
    ? "本机无 API Key"
    : `${item.identity.name} · ${item.identity.prefix ?? ""}`;
}

function contentSummary(item: PendingApproval): string | null {
  const parts: string[] = [];
  if (item.content_type) parts.push(item.content_type);
  if (item.content_length !== null) parts.push(formatBytes(item.content_length));
  return parts.length > 0 ? parts.join(" · ") : null;
}

function resolve(item: PendingApproval, decision: "allow" | "deny"): void {
  void approvalsStore.resolve(item.id, {
    decision,
    duration_seconds: durationFor(item.id),
  });
}

onMounted(() => {
  void approvalsStore.refresh();
  clock = window.setInterval(() => {
    now.value = Date.now();
  }, 250);
});

onBeforeUnmount(() => {
  if (clock !== undefined) window.clearInterval(clock);
});
</script>

<template>
  <div class="approval-root">
    <div v-if="items.length === 0" class="approval-empty">
      <Io5Checkmark :size="24" />
      <strong>没有待审批请求</strong>
      <span>需要确认的管理接口调用会显示在这里</span>
    </div>

    <div v-else class="approval-list">
      <article v-for="item in items" :key="item.id" class="approval-item">
        <div class="approval-main">
          <div class="approval-title">
            <span class="method mono">{{ item.method }}</span>
            <span class="target mono" :title="requestTarget(item)">{{
              requestTarget(item)
            }}</span>
            <span class="remaining">{{ remainingLabel(item) }}</span>
          </div>
          <div class="approval-meta">
            <span>{{ identityLabel(item) }}</span>
            <span class="mono">{{ item.route_template }}</span>
            <span v-if="item.source" class="mono">{{ item.source }}</span>
            <span v-if="contentSummary(item)">{{ contentSummary(item) }}</span>
          </div>
          <pre v-if="item.body_preview" class="body-preview">{{ item.body_preview }}<span
              v-if="item.body_preview_truncated"
              class="text-faint"
            >
…（已截断）</span
            ></pre>
        </div>
        <div class="approval-actions">
          <CustomSelect
            class="duration-select"
            :model-value="String(durationFor(item.id) ?? 'once')"
            :options="durationOptions"
            :disabled="approvalsStore.resolving.has(item.id)"
            aria-label="决定有效期"
            @update:model-value="setDuration(item.id, $event)"
          />
          <button
            class="btn deny-btn"
            :disabled="approvalsStore.resolving.has(item.id)"
            @click="resolve(item, 'deny')"
          >
            <Io5Close :size="14" />
            拒绝
          </button>
          <button
            class="btn primary"
            :disabled="approvalsStore.resolving.has(item.id)"
            @click="resolve(item, 'allow')"
          >
            <Io5Checkmark :size="14" />
            {{ approvalsStore.resolving.has(item.id) ? "处理中…" : "允许" }}
          </button>
        </div>
      </article>
    </div>
  </div>
</template>

<style scoped>
.approval-root { flex: 1; min-height: 0; display: flex; flex-direction: column; }
.approval-list { flex: 1; min-height: 0; overflow-y: auto; }
.approval-item { display: flex; align-items: flex-end; gap: 16px; padding: 13px 14px; border-bottom: 1px solid var(--border); }
.approval-item:last-child { border-bottom: 0; }
.approval-main { flex: 1; min-width: 0; }
.approval-title { display: flex; align-items: center; gap: 9px; }
.method { flex: none; color: var(--warning); font-weight: 700; }
.target { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 600; user-select: text; }
.remaining { flex: none; color: var(--warning); font-size: 11px; font-variant-numeric: tabular-nums; }
.approval-meta { display: flex; flex-wrap: wrap; gap: 4px 12px; margin-top: 5px; color: var(--text-secondary); font-size: 11px; }
.approval-meta span { user-select: text; }
.body-preview { max-height: 112px; margin: 9px 0 0; overflow: auto; padding: 8px 10px; border-left: 2px solid var(--border-strong); background: var(--bg-app); color: var(--text-secondary); font: 11px/1.55 var(--font-mono); white-space: pre-wrap; overflow-wrap: anywhere; user-select: text; }
.approval-actions { flex: none; display: flex; align-items: center; gap: 7px; }
.duration-select { width: 92px; font-size: 11px; }
.deny-btn { color: var(--danger); }
.approval-empty { flex: 1; display: grid; place-content: center; justify-items: center; gap: 5px; color: var(--text-faint); }
.approval-empty svg { margin-bottom: 4px; color: var(--success); }
.approval-empty strong { color: var(--text-secondary); font-size: 13px; }
.approval-empty span { font-size: 11px; }
@media (max-width: 650px) {
  .approval-item { align-items: stretch; flex-direction: column; }
  .approval-actions { justify-content: flex-end; }
}
</style>
