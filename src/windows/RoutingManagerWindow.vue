<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { Io5Add, Io5RadioButtonOn, Io5Save, Io5Trash } from "vue-icons-plus/io5";
import { useBackend } from "../api";
import type { HttpApiChange, Script } from "../api/types";
import MonacoEditor from "../components/MonacoEditor.vue";
import { appStore, reportError } from "../stores/app";
import { confirmDialog } from "../stores/dialog";
import { HTTP_API_CHANGE_EVENT } from "../stores/http-api-sync";
import { routingStore } from "../stores/routing";
import { windowsStore } from "../stores/windows";

const WINDOW_ID = "routing-manager";
const DEFAULT_SOURCE = 'return "default"\n';
const backend = useBackend();

const scripts = ref<Script[]>([]);
const selectedName = ref<string | null>(null);
const activeName = ref<string | null>(null);
const content = ref("");
const newName = ref("");
const dirty = ref(false);
const saving = ref(false);
const externalChanged = ref(false);

const selectedScript = computed(
  () => scripts.value.find((script) => script.name === selectedName.value) ?? null,
);

function validateName(name: string): string | null {
  if (!name) return "名称不能为空";
  if (name.length > 128) return "名称不能超过 128 个字符";
  if (name === "." || name === ".." || !/^[\p{L}\p{N}._-]+$/u.test(name)) {
    return "名称只能包含字母、数字、点、短横线和下划线";
  }
  return null;
}

function loadScript(script: Script | null): void {
  selectedName.value = script?.name ?? null;
  content.value = script?.content ?? "";
  dirty.value = false;
}

async function refresh(preferred?: string): Promise<void> {
  try {
    const [items, selection] = await Promise.all([
      backend.listRoutingScripts(),
      backend.getRoutingSelection(),
    ]);
    scripts.value = items;
    activeName.value = selection.name;
    const next =
      items.find((script) => script.name === preferred) ??
      items.find((script) => script.name === selectedName.value) ??
      items[0] ??
      null;
    loadScript(next);
    await routingStore.refresh();
  } catch (error) {
    reportError(error, "加载分流规则失败");
  }
}

async function confirmDiscard(): Promise<boolean> {
  if (!dirty.value) return true;
  return confirmDialog({
    title: "放弃未保存更改",
    message: `分流规则「${selectedName.value ?? ""}」包含未保存的更改，确定放弃吗？`,
    confirmText: "放弃",
    danger: true,
  });
}

async function selectScript(script: Script): Promise<void> {
  if (script.name === selectedName.value || !(await confirmDiscard())) return;
  loadScript(script);
}

async function createScript(): Promise<void> {
  const name = newName.value.trim();
  const problem = validateName(name);
  if (problem) {
    reportError(problem);
    return;
  }
  if (!(await confirmDiscard())) return;
  try {
    await backend.createRoutingScript({ name, content: DEFAULT_SOURCE });
    newName.value = "";
    await refresh(name);
    appStore.toast("分流规则已创建", "success");
  } catch (error) {
    reportError(error, "创建分流规则失败");
  }
}

async function save(): Promise<void> {
  if (!selectedName.value || saving.value) return;
  saving.value = true;
  try {
    await backend.updateRoutingScript(selectedName.value, { content: content.value });
    const item = scripts.value.find((script) => script.name === selectedName.value);
    if (item) item.content = content.value;
    dirty.value = false;
    externalChanged.value = false;
    appStore.toast("分流规则已保存", "success");
  } catch (error) {
    reportError(error, "保存分流规则失败");
  } finally {
    saving.value = false;
  }
}

async function remove(): Promise<void> {
  const name = selectedName.value;
  if (!name) return;
  const ok = await confirmDialog({
    title: "删除分流规则",
    message: `确定删除「${name}」吗？名称创建后不可恢复。`,
    confirmText: "删除",
    danger: true,
  });
  if (!ok) return;
  try {
    await backend.deleteRoutingScript(name);
    await refresh();
    appStore.toast("分流规则已删除", "success");
  } catch (error) {
    reportError(error, "删除分流规则失败");
  }
}

async function toggleActive(): Promise<void> {
  if (!selectedName.value) return;
  const name = activeName.value === selectedName.value ? null : selectedName.value;
  try {
    activeName.value = (await backend.replaceRoutingSelection({ name })).name;
    await routingStore.refresh();
    appStore.toast(name ? "分流规则已生效" : "已取消分流规则", "success");
  } catch (error) {
    reportError(error, "切换分流规则失败");
    await refresh(selectedName.value ?? undefined);
  }
}

function onHttpApiChange(event: Event): void {
  const resources = (event as CustomEvent<HttpApiChange>).detail.resources;
  if (
    !resources.includes("all") &&
    !resources.includes("routing_scripts") &&
    !resources.includes("routing_selection")
  ) {
    return;
  }
  if (dirty.value) {
    externalChanged.value = true;
    return;
  }
  void refresh(selectedName.value ?? undefined);
}

onMounted(() => {
  void refresh();
  windowsStore.registerCloseGuard(WINDOW_ID, confirmDiscard);
  window.addEventListener(HTTP_API_CHANGE_EVENT, onHttpApiChange);
});

onBeforeUnmount(() => {
  windowsStore.unregisterCloseGuard(WINDOW_ID);
  window.removeEventListener(HTTP_API_CHANGE_EVENT, onHttpApiChange);
});
</script>

<template>
  <div class="rm-root">
    <aside class="rm-sidebar">
      <div class="rm-create">
        <input
          v-model="newName"
          class="input"
          placeholder="新规则名称"
          @keyup.enter="createScript"
        />
        <button class="btn icon" title="创建" @click="createScript">
          <Io5Add :size="15" />
        </button>
      </div>
      <button
        v-for="script in scripts"
        :key="script.name"
        class="rm-item mono"
        :class="{ selected: selectedName === script.name }"
        @click="selectScript(script)"
      >
        <span>{{ script.name }}</span>
        <span v-if="activeName === script.name" class="rm-active">生效中</span>
      </button>
      <div v-if="!scripts.length" class="empty-hint">暂无分流规则</div>
    </aside>

    <section class="rm-main">
      <div class="rm-toolbar">
        <span class="rm-spacer" />
        <button class="btn" :disabled="!selectedName" @click="toggleActive">
          <Io5RadioButtonOn :size="14" />
          {{ activeName === selectedName ? "取消生效" : "设为生效" }}
        </button>
        <button class="btn danger" :disabled="!selectedName" @click="remove">
          <Io5Trash :size="14" />删除
        </button>
        <button class="btn primary" :disabled="!selectedName || !dirty || saving" @click="save">
          <Io5Save :size="14" />保存
        </button>
      </div>
      <div v-if="externalChanged" class="rm-notice">
        规则已被外部修改；保存前请关闭窗口重新加载，或继续保存覆盖内容。
      </div>
      <MonacoEditor
        v-if="selectedName"
        :model-value="content"
        language="lua"
        @update:model-value="(value) => { content = value; dirty = value !== selectedScript?.content; }"
      />
      <div v-else class="empty-hint">选择或创建一个分流规则</div>
    </section>
  </div>
</template>

<style scoped>
.rm-root {
  flex: 1;
  min-height: 0;
  display: flex;
}
.rm-sidebar {
  width: 230px;
  flex: none;
  overflow-y: auto;
  border-right: 1px solid var(--border);
  background: var(--bg-panel);
  padding: var(--space-2);
}
.rm-create,
.rm-toolbar {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}
.rm-create {
  margin-bottom: var(--space-2);
}
.rm-create .input {
  min-width: 0;
  flex: 1;
}
.rm-item {
  width: 100%;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-2);
  padding: 7px 8px;
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text);
  cursor: pointer;
  text-align: left;
}
.rm-item:hover {
  background: var(--bg-hover);
}
.rm-item.selected {
  background: var(--bg-selected);
}
.rm-active {
  flex: none;
  color: var(--accent);
  font-size: 10px;
}
.rm-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}
.rm-toolbar {
  flex: none;
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid var(--border);
}
.rm-spacer {
  flex: 1;
}
.rm-notice {
  padding: 6px var(--space-3);
  background: color-mix(in srgb, var(--warning) 14%, var(--bg-panel));
  color: var(--warning);
  font-size: 12px;
}
</style>
