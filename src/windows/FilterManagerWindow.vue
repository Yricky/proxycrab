<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import {
  Io5Add,
  Io5Play,
  Io5Save,
  Io5Trash,
} from "vue-icons-plus/io5";
import { useBackend } from "../api";
import type { HttpApiChange, Script } from "../api/types";
import MonacoEditor from "../components/MonacoEditor.vue";
import { appStore, reportError } from "../stores/app";
import { confirmDialog } from "../stores/dialog";
import { HTTP_API_CHANGE_EVENT } from "../stores/http-api-sync";
import { sessionsStore } from "../stores/sessions";
import { windowsStore } from "../stores/windows";

const WINDOW_ID = "filter-manager";
const DEFAULT_SOURCE = "local input = ...\nreturn false\n";
const backend = useBackend();

const root = ref<HTMLElement | null>(null);
const editor = ref<{ focus: () => void } | null>(null);
const scripts = ref<Script[]>([]);
const selectedName = ref<string | null>(null);
const content = ref("");
const newScriptName = ref("");
const dirty = ref(false);
const saving = ref(false);
const debugLogId = ref("");
const debugInput = ref("");
const debugging = ref(false);
const debugResult = ref<string | null>(null);
const debugFailed = ref(false);
const externalChanged = ref(false);

const selectedScript = computed(
  () => scripts.value.find((script) => script.name === selectedName.value) ?? null,
);

function notifyChanged(): void {
  window.dispatchEvent(new CustomEvent("filter-scripts-changed"));
}

function validateName(name: string): string | null {
  if (!name) return "名称不能为空";
  if (name.length > 128) return "名称不能超过 128 个字符";
  if (name === "." || name === ".." || !/^[\p{L}\p{N}._-]+$/u.test(name)) {
    return "名称只能包含字母、数字、点、短横线和下划线";
  }
  return null;
}

function clearDebugResult(): void {
  debugResult.value = null;
  debugFailed.value = false;
}

function loadScript(script: Script | null): void {
  selectedName.value = script?.name ?? null;
  content.value = script?.content ?? "";
  dirty.value = false;
  clearDebugResult();
  if (script) void nextTick(() => editor.value?.focus());
}

async function refreshScripts(preferredName?: string): Promise<void> {
  try {
    scripts.value = await backend.listFilterScripts();
    const next =
      scripts.value.find((script) => script.name === preferredName) ??
      scripts.value.find((script) => script.name === selectedName.value) ??
      scripts.value[0] ??
      null;
    loadScript(next);
  } catch (error) {
    reportError(error, "加载过滤脚本失败");
  }
}

function onHttpApiChange(event: Event): void {
  const { resources } = (event as CustomEvent<HttpApiChange>).detail;
  if (!resources.includes("all") && !resources.includes("filter_scripts")) return;
  if (dirty.value) {
    externalChanged.value = true;
    return;
  }
  externalChanged.value = false;
  void refreshScripts(selectedName.value ?? undefined);
}

function reloadExternal(): void {
  externalChanged.value = false;
  void refreshScripts(selectedName.value ?? undefined);
}

async function confirmDiscard(): Promise<boolean> {
  if (!dirty.value) return true;
  return confirmDialog({
    title: "放弃未保存更改",
    message: `过滤脚本「${selectedName.value ?? ""}」包含未保存的更改，确定放弃吗？`,
    confirmText: "放弃",
    danger: true,
  });
}

async function selectScript(script: Script): Promise<void> {
  if (script.name === selectedName.value) return;
  if (!(await confirmDiscard())) return;
  loadScript(script);
}

function onContentChange(value: string): void {
  content.value = value;
  dirty.value = value !== selectedScript.value?.content;
  clearDebugResult();
}

async function persist(showToast: boolean): Promise<{ ok: true } | { ok: false; message: string }> {
  const name = selectedName.value;
  if (!name || saving.value) return { ok: false, message: "没有可保存的过滤脚本" };
  saving.value = true;
  try {
    await backend.updateFilterScript(name, { content: content.value });
    const item = scripts.value.find((script) => script.name === name);
    if (item) item.content = content.value;
    dirty.value = false;
    externalChanged.value = false;
    notifyChanged();
    if (showToast) appStore.toast("已保存", "success");
    return { ok: true };
  } catch (error) {
    return { ok: false, message: error instanceof Error ? error.message : String(error) };
  } finally {
    saving.value = false;
  }
}

async function save(): Promise<void> {
  const result = await persist(true);
  if (!result.ok) reportError(result.message, "保存失败");
}

async function saveAndRun(): Promise<void> {
  if (debugging.value) return;
  const sessionId = sessionsStore.viewingSessionId;
  if (sessionId === null) {
    debugResult.value = "请先选择会话";
    debugFailed.value = true;
    return;
  }
  const logId = Number(debugLogId.value.trim());
  if (!Number.isSafeInteger(logId) || logId <= 0) {
    debugResult.value = "Log ID 必须为正整数";
    debugFailed.value = true;
    return;
  }
  const name = selectedName.value;
  if (!name) return;

  debugging.value = true;
  clearDebugResult();
  const saved = await persist(false);
  if (!saved.ok) {
    debugResult.value = saved.message;
    debugFailed.value = true;
    debugging.value = false;
    return;
  }
  try {
    const result = await backend.debugFilterScript(name, {
      session_id: sessionId,
      log_id: logId,
      input: debugInput.value,
    });
    debugResult.value = String(result);
  } catch (error) {
    debugResult.value = error instanceof Error ? error.message : String(error);
    debugFailed.value = true;
  } finally {
    debugging.value = false;
  }
}

async function createScript(): Promise<void> {
  const name = newScriptName.value.trim();
  const problem = validateName(name);
  if (problem) {
    reportError(problem);
    return;
  }
  if (!(await confirmDiscard())) return;
  try {
    await backend.createFilterScript({ name, content: DEFAULT_SOURCE });
    newScriptName.value = "";
    await refreshScripts(name);
    notifyChanged();
  } catch (error) {
    reportError(error, "创建过滤脚本失败");
  }
}

async function removeScript(script: Script): Promise<void> {
  const ok = await confirmDialog({
    title: "删除过滤脚本",
    message: `确定删除过滤脚本「${script.name}」吗？引用它的会话会清除过滤。该操作不可撤销。`,
    confirmText: "删除",
    danger: true,
  });
  if (!ok) return;
  try {
    await backend.deleteFilterScript(script.name);
    scripts.value = scripts.value.filter((item) => item.name !== script.name);
    if (selectedName.value === script.name) loadScript(scripts.value[0] ?? null);
    notifyChanged();
  } catch (error) {
    reportError(error, "删除过滤脚本失败");
  }
}

function onKeydown(event: KeyboardEvent): void {
  if (
    (event.metaKey || event.ctrlKey) &&
    event.key.toLowerCase() === "s" &&
    root.value?.contains(document.activeElement)
  ) {
    event.preventDefault();
    void save();
  }
}

onMounted(() => {
  windowsStore.registerCloseGuard(WINDOW_ID, confirmDiscard);
  window.addEventListener("keydown", onKeydown);
  window.addEventListener(HTTP_API_CHANGE_EVENT, onHttpApiChange);
  void refreshScripts();
});

onBeforeUnmount(() => {
  windowsStore.unregisterCloseGuard(WINDOW_ID);
  window.removeEventListener("keydown", onKeydown);
  window.removeEventListener(HTTP_API_CHANGE_EVENT, onHttpApiChange);
});
</script>

<template>
  <div ref="root" class="fm-root">
    <aside class="fm-sidebar">
      <div class="fm-sidebar-title">过滤脚本</div>
      <div class="fm-create">
        <input
          v-model="newScriptName"
          class="input"
          placeholder="新过滤脚本名"
          @keyup.enter="createScript"
        />
        <button class="btn icon primary" title="新建" @click="createScript">
          <Io5Add :size="15" />
        </button>
      </div>
      <div class="fm-script-list">
        <div v-if="!scripts.length" class="empty-hint fm-list-empty">暂无过滤脚本</div>
        <div
          v-for="script in scripts"
          :key="script.name"
          class="fm-script-row"
          :class="{ active: selectedName === script.name }"
          @click="selectScript(script)"
        >
          <span class="fm-script-name mono" :title="script.name">{{ script.name }}</span>
          <span class="fm-row-actions">
            <button class="btn icon danger" title="删除" @click.stop="removeScript(script)">
              <Io5Trash :size="14" />
            </button>
          </span>
        </div>
      </div>
    </aside>

    <section class="fm-editor-pane">
      <div v-if="externalChanged" class="fm-external">
        <span>过滤脚本已被外部修改，未保存内容仍保留在编辑器中。</span>
        <button class="btn compact" @click="reloadExternal">重新加载</button>
      </div>
      <template v-if="selectedScript">
        <div class="fm-editor-toolbar">
          <span class="fm-current-name mono" :title="selectedName ?? ''">{{ selectedName }}</span>
          <span v-if="dirty" class="fm-dirty">未保存</span>
          <span
            v-if="debugResult !== null"
            class="fm-debug-result mono"
            :class="{ error: debugFailed }"
            :title="debugResult"
          >
            {{ debugResult }}
          </span>
          <span v-else class="fm-toolbar-spacer" />
          <input
            v-model="debugInput"
            class="input fm-argument mono"
            placeholder="字符串参数"
            title="通过 Lua ... 传入的字符串"
            @input="clearDebugResult"
            @keyup.enter="saveAndRun"
          />
          <input
            v-model="debugLogId"
            class="input fm-log-id mono"
            inputmode="numeric"
            placeholder="Log ID"
            title="当前会话中的 Log ID"
            @input="clearDebugResult"
            @keyup.enter="saveAndRun"
          />
          <button class="btn" :disabled="saving || debugging" @click="save">
            <Io5Save :size="14" />
            {{ saving && !debugging ? "保存中…" : "保存" }}
          </button>
          <button class="btn primary" :disabled="saving || debugging" @click="saveAndRun">
            <Io5Play :size="14" />
            {{ debugging ? "运行中…" : "保存并运行" }}
          </button>
        </div>
        <MonacoEditor
          ref="editor"
          :model-value="content"
          language="lua"
          @update:model-value="onContentChange"
        />
        <div class="fm-statusbar text-faint">
          entry 为当前记录 · 字符串参数通过 ... 传入 · 必须返回布尔值
        </div>
      </template>
      <div v-else class="fm-editor-empty">
        <span class="empty-hint">新建一个过滤脚本后即可开始编辑</span>
      </div>
    </section>
  </div>
</template>

<style scoped>
.fm-root {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
}
.fm-sidebar {
  width: 236px;
  min-width: 190px;
  display: flex;
  flex-direction: column;
  border-right: 1px solid var(--border);
  background: var(--bg-panel);
}
.fm-external {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid var(--warning);
  color: var(--warning);
  font-size: 12px;
  flex: none;
}
.fm-external span {
  flex: 1;
}
.fm-sidebar-title {
  padding: 10px 12px 6px;
  color: var(--text-secondary);
  font-size: 12px;
  font-weight: 600;
}
.fm-create {
  display: flex;
  gap: var(--space-1);
  padding: 0 var(--space-2) var(--space-2);
}
.fm-create .input {
  min-width: 0;
  flex: 1;
}
.fm-script-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 0 var(--space-1) var(--space-2);
}
.fm-list-empty {
  padding: var(--space-3);
  text-align: center;
}
.fm-script-row {
  min-height: 32px;
  display: flex;
  align-items: center;
  gap: var(--space-1);
  padding: 3px 5px 3px 9px;
  border-radius: var(--radius-sm);
  cursor: pointer;
}
.fm-script-row:hover {
  background: var(--bg-hover);
}
.fm-script-row.active {
  background: var(--bg-selected);
}
.fm-script-name {
  min-width: 0;
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
}
.fm-row-actions {
  display: none;
  flex: none;
  align-items: center;
  gap: 1px;
}
.fm-script-row:hover .fm-row-actions,
.fm-script-row.active .fm-row-actions {
  display: flex;
}
.fm-editor-pane {
  min-width: 0;
  min-height: 0;
  flex: 1;
  display: flex;
  flex-direction: column;
}
.fm-editor-toolbar {
  min-width: 0;
  min-height: 42px;
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: 6px 10px;
  border-bottom: 1px solid var(--border);
}
.fm-current-name {
  max-width: 140px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 600;
}
.fm-dirty {
  flex: none;
  color: var(--warning);
  font-size: 11px;
  font-weight: 600;
}
.fm-toolbar-spacer {
  flex: 1;
}
.fm-debug-result {
  min-width: 36px;
  flex: 1;
  overflow: hidden;
  color: var(--success);
  font-size: 11px;
  text-align: right;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.fm-debug-result.error {
  color: var(--danger);
}
.fm-argument {
  width: 150px;
  flex: none;
}
.fm-log-id {
  width: 92px;
  flex: none;
}
.fm-statusbar {
  flex: none;
  padding: 4px 12px;
  border-top: 1px solid var(--border);
  font-size: 11px;
}
.fm-editor-empty {
  flex: 1;
  display: grid;
  place-items: center;
}
</style>
