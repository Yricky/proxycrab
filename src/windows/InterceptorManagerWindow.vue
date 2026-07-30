<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { Io5Add, Io5Create, Io5Pencil, Io5Trash } from "vue-icons-plus/io5";
import { useBackend } from "../api";
import type {
  InterceptorKind,
  InterceptorLibraryItem,
  InterceptorLibraryList,
} from "../api/types";
import { reportError } from "../stores/app";
import { confirmDialog } from "../stores/dialog";
import { openScriptEditor } from "./launcher";

const backend = useBackend();

const kind = ref<InterceptorKind>("request");
const list = ref<InterceptorLibraryList | null>(null);
const newName = ref("");
const editingName = ref<string | null>(null);
const nextName = ref("");

const sortedItems = computed<InterceptorLibraryItem[]>(() =>
  [...(list.value?.items ?? [])].sort((a, b) => a.name.localeCompare(b.name)),
);

function notifyChanged(): void {
  window.dispatchEvent(new CustomEvent("interceptors-changed"));
}

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
  editingName.value = null;
  void refresh();
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
    await backend.createInterceptor({ kind: kind.value, name, content: "" });
    newName.value = "";
    await refresh();
    notifyChanged();
    openScriptEditor(kind.value, name);
  } catch (error) {
    reportError(error, "创建拦截器失败");
  }
}

function startRename(item: InterceptorLibraryItem): void {
  editingName.value = item.name;
  nextName.value = item.name;
}

async function rename(item: InterceptorLibraryItem): Promise<void> {
  const name = nextName.value.trim();
  const problem = validateName(name);
  if (problem) {
    reportError(problem);
    return;
  }
  if (name === item.name) {
    editingName.value = null;
    return;
  }
  try {
    await backend.updateInterceptor(kind.value, item.name, { name });
    editingName.value = null;
    await refresh();
    notifyChanged();
  } catch (error) {
    reportError(error, "重命名拦截器失败");
  }
}

async function remove(item: InterceptorLibraryItem): Promise<void> {
  const usage =
    item.usage_count > 0
      ? `它正在被 ${item.usage_count} 个会话使用，删除后会从这些会话的链路中移除。`
      : "当前没有会话引用它。";
  const ok = await confirmDialog({
    title: "删除拦截器",
    message: `确定删除拦截器「${item.name}」吗？${usage}该操作不可撤销。`,
    confirmText: "删除",
    danger: true,
  });
  if (!ok) return;
  try {
    await backend.deleteInterceptor(kind.value, item.name);
    await refresh();
    notifyChanged();
  } catch (error) {
    reportError(error, "删除拦截器失败");
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
        placeholder="新建全局拦截器脚本"
        @keyup.enter="create"
      />
      <button class="btn primary" @click="create"><Io5Add :size="14" /> 新建</button>
    </div>

    <div class="im-list">
      <div v-if="!sortedItems.length" class="empty-hint">暂无全局拦截器脚本</div>
      <div v-for="item in sortedItems" :key="item.name" class="im-row">
        <Io5Create :size="14" class="im-script-icon" />
        <template v-if="editingName === item.name">
          <input
            v-model="nextName"
            class="input im-rename-input mono"
            autofocus
            @keyup.enter="rename(item)"
            @keyup.esc="editingName = null"
          />
          <button class="btn primary compact" @click="rename(item)">保存</button>
          <button class="btn compact" @click="editingName = null">取消</button>
        </template>
        <template v-else>
          <button class="im-name mono" @click="openScriptEditor(kind, item.name)">
            {{ item.name }}
          </button>
          <span class="im-usage">
            {{ item.usage_count ? `${item.usage_count} 个会话使用` : "未使用" }}
          </span>
          <span class="im-spacer" />
          <button class="btn icon" title="重命名" @click="startRename(item)">
            <Io5Pencil :size="14" />
          </button>
          <button class="btn icon" title="编辑脚本" @click="openScriptEditor(kind, item.name)">
            <Io5Create :size="14" />
          </button>
          <button class="btn icon danger" title="删除" @click="remove(item)">
            <Io5Trash :size="14" />
          </button>
        </template>
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
}
.im-tab {
  padding: 4px 12px;
  border: 0;
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
  min-height: 36px;
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: 4px var(--space-3);
}
.im-row:hover {
  background: var(--bg-hover);
}
.im-script-icon {
  flex: none;
  color: var(--text-faint);
}
.im-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--text);
  cursor: pointer;
  text-align: left;
}
.im-name:hover {
  color: var(--accent);
}
.im-usage {
  flex: none;
  color: var(--text-faint);
  font-size: 11px;
}
.im-spacer {
  flex: 1;
}
.im-rename-input {
  flex: 1;
  min-width: 0;
}
.btn.compact {
  padding: 4px 9px;
}
.btn.icon.danger:hover:not(:disabled) {
  color: var(--danger);
}
</style>
