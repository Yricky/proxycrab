<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { Io5Add, Io5ArrowDown, Io5ArrowUp, Io5Trash } from "vue-icons-plus/io5";
import { useBackend } from "../api";
import type { InterceptorInfo, InterceptorKind, InterceptorList } from "../api/types";
import { reportError } from "../stores/app";
import { confirmDialog } from "../stores/dialog";
import { openScriptEditor } from "./launcher";

const backend = useBackend();

const kind = ref<InterceptorKind>("request");
const list = ref<InterceptorList | null>(null);
const newName = ref("");

const sortedItems = computed<InterceptorInfo[]>(() => {
  if (!list.value) return [];
  const order = list.value.active_order;
  const enabled = list.value.items
    .filter((it) => it.enabled)
    .sort((a, b) => order.indexOf(a.name) - order.indexOf(b.name));
  const disabled = list.value.items
    .filter((it) => !it.enabled)
    .sort((a, b) => a.name.localeCompare(b.name));
  return [...enabled, ...disabled];
});

async function refresh(): Promise<void> {
  try {
    list.value = await backend.listInterceptors(kind.value);
  } catch (error) {
    reportError(error, "加载拦截器列表失败");
  }
}

function switchKind(next: InterceptorKind): void {
  if (kind.value === next) return;
  kind.value = next;
  void refresh();
}

function orderIndex(name: string): number {
  return list.value?.active_order.indexOf(name) ?? -1;
}

async function toggleEnabled(item: InterceptorInfo): Promise<void> {
  try {
    await backend.setInterceptorEnabled(kind.value, item.name, !item.enabled);
    await refresh();
  } catch (error) {
    reportError(error, "更新拦截器状态失败");
  }
}

async function move(item: InterceptorInfo, delta: -1 | 1): Promise<void> {
  if (!list.value) return;
  const order = [...list.value.active_order];
  const from = order.indexOf(item.name);
  const to = from + delta;
  if (from < 0 || to < 0 || to >= order.length) return;
  [order[from], order[to]] = [order[to], order[from]];
  try {
    await backend.setInterceptorOrder({ kind: kind.value, order });
    await refresh();
  } catch (error) {
    reportError(error, "调整执行顺序失败");
  }
}

async function remove(item: InterceptorInfo): Promise<void> {
  const ok = await confirmDialog({
    title: "删除拦截器",
    message: `确定删除拦截器「${item.name}」吗？该操作不可撤销。`,
    confirmText: "删除",
    danger: true,
  });
  if (!ok) return;
  try {
    await backend.deleteInterceptor(kind.value, item.name);
    await refresh();
  } catch (error) {
    reportError(error, "删除拦截器失败");
  }
}

function validateName(name: string): string | null {
  if (!name) return "名称不能为空";
  if (name.includes("/") || name.includes("\\")) return "名称不能包含路径分隔符（/ 或 \\）";
  return null;
}

async function create(): Promise<void> {
  const name = newName.value.trim();
  const problem = validateName(name);
  if (problem) {
    reportError(problem);
    return;
  }
  try {
    await backend.createInterceptor({ kind: kind.value, name, content: "", enabled: false });
    newName.value = "";
    await refresh();
    openScriptEditor(kind.value, name);
  } catch (error) {
    reportError(error, "创建拦截器失败");
  }
}

onMounted(() => void refresh());
</script>

<template>
  <div class="im-root">
    <div class="im-tabs">
      <button
        class="im-tab"
        :class="{ active: kind === 'request' }"
        @click="switchKind('request')"
      >
        请求拦截器
      </button>
      <button
        class="im-tab"
        :class="{ active: kind === 'response' }"
        @click="switchKind('response')"
      >
        响应拦截器
      </button>
    </div>

    <div class="im-actions">
      <input
        v-model="newName"
        class="input im-name-input"
        placeholder="新拦截器脚本名"
        @keyup.enter="create"
      />
      <button class="btn primary" @click="create"><Io5Add :size="14" /> 新建</button>
    </div>

    <div class="im-list">
      <div v-if="!sortedItems.length" class="empty-hint">暂无拦截器脚本</div>
      <div v-for="item in sortedItems" :key="item.name" class="im-row">
        <button
          class="switch"
          :class="{ on: item.enabled }"
          :title="item.enabled ? '点击停用' : '点击启用'"
          @click="toggleEnabled(item)"
        />
        <span
          class="im-name mono clickable"
          :title="`编辑 ${item.name}`"
          @click="openScriptEditor(kind, item.name)"
        >
          {{ item.name }}
        </span>
        <span v-if="item.enabled && orderIndex(item.name) >= 0" class="badge">
          #{{ orderIndex(item.name) + 1 }}
        </span>
        <span class="im-spacer" />
        <template v-if="item.enabled">
          <button
            class="btn icon"
            title="上移"
            :disabled="orderIndex(item.name) <= 0"
            @click="move(item, -1)"
          >
            <Io5ArrowUp :size="14" />
          </button>
          <button
            class="btn icon"
            title="下移"
            :disabled="orderIndex(item.name) >= (list?.active_order.length ?? 0) - 1"
            @click="move(item, 1)"
          >
            <Io5ArrowDown :size="14" />
          </button>
        </template>
        <button class="btn icon danger" title="删除" @click="remove(item)">
          <Io5Trash :size="14" />
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.im-root {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.im-tabs {
  display: flex;
  gap: var(--space-1);
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid var(--border);
  flex: none;
}

.im-tab {
  padding: 4px 12px;
  border: none;
  border-radius: var(--radius-md);
  background: transparent;
  color: var(--text-secondary);
  font: inherit;
  cursor: pointer;
}
.im-tab:hover {
  background: var(--bg-hover);
}
.im-tab.active {
  background: var(--bg-selected);
  color: var(--accent);
  font-weight: 600;
}

.im-actions {
  display: flex;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid var(--border);
  flex: none;
}

.im-name-input {
  flex: 1;
  min-width: 0;
}

.im-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: var(--space-1) 0;
}

.im-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: 5px var(--space-3);
}
.im-row:hover {
  background: var(--bg-hover);
}

.im-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.im-name:hover {
  color: var(--accent);
}

.im-spacer {
  flex: 1;
}

.btn.icon.danger:hover:not(:disabled) {
  color: var(--danger);
}
</style>
