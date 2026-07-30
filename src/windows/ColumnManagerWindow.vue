<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import {
  Io5Add,
  Io5Checkmark,
  Io5Close,
  Io5Create,
  Io5Play,
  Io5Save,
  Io5Trash,
} from "vue-icons-plus/io5";
import { useBackend } from "../api";
import type { Script } from "../api/types";
import MonacoEditor from "../components/MonacoEditor.vue";
import { appStore, reportError } from "../stores/app";
import { confirmDialog } from "../stores/dialog";
import { logsStore } from "../stores/logs";
import { sessionsStore } from "../stores/sessions";
import { windowsStore } from "../stores/windows";

const WINDOW_ID = "column-manager";
const backend = useBackend();

const root = ref<HTMLElement | null>(null);
const editor = ref<{ focus: () => void } | null>(null);
const renameInput = ref<HTMLInputElement | null>(null);
const scripts = ref<Script[]>([]);
const selectedName = ref<string | null>(null);
const content = ref("");
const newScriptName = ref("");
const dirty = ref(false);
const saving = ref(false);
const renamingName = ref<string | null>(null);
const renameValue = ref("");
const debugLogId = ref("");
const debugging = ref(false);
const debugResult = ref<string | null>(null);
const debugFailed = ref(false);

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

function clearDebugResult(): void {
  debugResult.value = null;
  debugFailed.value = false;
}

function loadScript(script: Script | null): void {
  selectedName.value = script?.name ?? null;
  content.value = script?.content ?? "";
  dirty.value = false;
  renamingName.value = null;
  clearDebugResult();
  if (script) void nextTick(() => editor.value?.focus());
}

async function refreshScripts(preferredName?: string): Promise<void> {
  try {
    scripts.value = await backend.listColumnScripts();
    const next =
      scripts.value.find((script) => script.name === preferredName) ??
      scripts.value.find((script) => script.name === selectedName.value) ??
      scripts.value[0] ??
      null;
    loadScript(next);
  } catch (error) {
    reportError(error, "加载列脚本失败");
  }
}

async function confirmDiscard(): Promise<boolean> {
  if (!dirty.value) return true;
  return confirmDialog({
    title: "放弃未保存更改",
    message: `列脚本「${selectedName.value ?? ""}」包含未保存的更改，确定放弃吗？`,
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
  if (!name || saving.value) return { ok: false, message: "没有可保存的列脚本" };
  saving.value = true;
  try {
    await backend.updateColumnScript(name, { content: content.value });
    const item = scripts.value.find((script) => script.name === name);
    if (item) item.content = content.value;
    dirty.value = false;
    if (showToast) appStore.toast("已保存", "success");
    return { ok: true };
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    return { ok: false, message };
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

  debugging.value = true;
  clearDebugResult();
  const saveResult = await persist(false);
  if (!saveResult.ok) {
    debugResult.value = saveResult.message;
    debugFailed.value = true;
    debugging.value = false;
    return;
  }

  try {
    const payload = await backend.getLogViews({
      session_id: sessionId,
      logs: [{ id: logId }],
      view: {
        columns: [{ kind: "script", script_name: selectedName.value!, width: 160 }],
      },
    });
    const exception = payload.exceptions.find((item) => item.id === logId);
    if (exception) {
      debugResult.value = exception.message;
      debugFailed.value = true;
      return;
    }
    const row = payload.rows.find((item) => item.id === logId);
    if (!row) {
      debugResult.value = `Log ${logId} 不存在`;
      debugFailed.value = true;
      return;
    }
    debugResult.value = row.cells[0] ?? "";
    debugFailed.value = false;
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
    await backend.createColumnScript({ name, content: "" });
    newScriptName.value = "";
    await refreshScripts(name);
  } catch (error) {
    reportError(error, "创建列脚本失败");
  }
}

function startRename(script: Script): void {
  renamingName.value = script.name;
  renameValue.value = script.name;
  void nextTick(() => {
    renameInput.value?.focus();
    renameInput.value?.select();
  });
}

function cancelRename(): void {
  renamingName.value = null;
  renameValue.value = "";
}

async function commitRename(script: Script): Promise<void> {
  const nextName = renameValue.value.trim();
  const problem = validateName(nextName);
  if (problem) {
    reportError(problem);
    return;
  }
  if (nextName === script.name) {
    cancelRename();
    return;
  }
  try {
    await backend.updateColumnScript(script.name, { name: nextName });
    script.name = nextName;
    if (selectedName.value === renamingName.value) selectedName.value = nextName;
    cancelRename();
    clearDebugResult();
    await logsStore.refreshView();
  } catch (error) {
    reportError(error, "重命名列脚本失败");
  }
}

async function removeScript(script: Script): Promise<void> {
  const selectedAndDirty = script.name === selectedName.value && dirty.value;
  const ok = await confirmDialog({
    title: "删除列脚本",
    message: selectedAndDirty
      ? `列脚本「${script.name}」包含未保存的更改。确定删除吗？该操作不可撤销。`
      : `确定删除列脚本「${script.name}」吗？该操作不可撤销。`,
    confirmText: "删除",
    danger: true,
  });
  if (!ok) return;
  try {
    await backend.deleteColumnScript(script.name);
    scripts.value = scripts.value.filter((item) => item.name !== script.name);
    if (selectedName.value === script.name) loadScript(scripts.value[0] ?? null);
    await logsStore.refreshView();
  } catch (error) {
    reportError(error, "删除列脚本失败");
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

watch(
  () => sessionsStore.viewingSessionId,
  () => clearDebugResult(),
);

onMounted(() => {
  windowsStore.registerCloseGuard(WINDOW_ID, confirmDiscard);
  window.addEventListener("keydown", onKeydown);
  void refreshScripts();
});

onBeforeUnmount(() => {
  windowsStore.unregisterCloseGuard(WINDOW_ID);
  window.removeEventListener("keydown", onKeydown);
});
</script>

<template>
  <div ref="root" class="cm-root">
    <aside class="cm-sidebar">
      <div class="cm-sidebar-title">列脚本</div>
      <div class="cm-create">
        <input
          v-model="newScriptName"
          class="input"
          placeholder="新列脚本名"
          @keyup.enter="createScript"
        />
        <button class="btn icon primary" title="新建" @click="createScript">
          <Io5Add :size="15" />
        </button>
      </div>
      <div class="cm-script-list">
        <div v-if="!scripts.length" class="empty-hint cm-list-empty">暂无列脚本</div>
        <div
          v-for="script in scripts"
          :key="script.name"
          class="cm-script-row"
          :class="{ active: selectedName === script.name }"
          @click="selectScript(script)"
        >
          <template v-if="renamingName === script.name">
            <input
              ref="renameInput"
              v-model="renameValue"
              class="input cm-rename-input mono"
              @click.stop
              @keyup.enter="commitRename(script)"
              @keyup.esc="cancelRename"
            />
            <button
              class="btn icon"
              title="确认重命名"
              @click.stop="commitRename(script)"
            >
              <Io5Checkmark :size="14" />
            </button>
            <button class="btn icon" title="取消" @click.stop="cancelRename">
              <Io5Close :size="14" />
            </button>
          </template>
          <template v-else>
            <span class="cm-script-name mono" :title="script.name">{{ script.name }}</span>
            <span class="cm-row-actions">
              <button class="btn icon" title="重命名" @click.stop="startRename(script)">
                <Io5Create :size="14" />
              </button>
              <button
                class="btn icon danger"
                title="删除"
                @click.stop="removeScript(script)"
              >
                <Io5Trash :size="14" />
              </button>
            </span>
          </template>
        </div>
      </div>
    </aside>

    <section class="cm-editor-pane">
      <template v-if="selectedScript">
        <div class="cm-editor-toolbar">
          <span class="cm-current-name mono" :title="selectedName ?? ''">{{ selectedName }}</span>
          <span v-if="dirty" class="cm-dirty">未保存</span>
          <span
            v-if="debugResult !== null"
            class="cm-debug-result mono"
            :class="{ error: debugFailed }"
            :title="debugResult"
          >
            {{ debugResult || "（空字符串）" }}
          </span>
          <span v-else class="cm-toolbar-spacer" />
          <input
            v-model="debugLogId"
            class="input cm-log-id mono"
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
          <button
            class="btn primary"
            :disabled="saving || debugging"
            @click="saveAndRun"
          >
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
        <div class="cm-statusbar text-faint">
          Lua 5.4 沙箱 · 10 万指令上限 · 16 MiB 内存上限
        </div>
      </template>
      <div v-else class="cm-editor-empty">
        <span class="empty-hint">新建一个列脚本后即可开始编辑</span>
      </div>
    </section>
  </div>
</template>

<style scoped>
.cm-root {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
}

.cm-sidebar {
  width: 236px;
  min-width: 190px;
  display: flex;
  flex-direction: column;
  border-right: 1px solid var(--border);
  background: var(--bg-panel);
}

.cm-sidebar-title {
  padding: 10px 12px 6px;
  color: var(--text-secondary);
  font-size: 12px;
  font-weight: 600;
}

.cm-create {
  display: flex;
  gap: var(--space-1);
  padding: 0 var(--space-2) var(--space-2);
}

.cm-create .input {
  min-width: 0;
  flex: 1;
}

.cm-script-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 0 var(--space-1) var(--space-2);
}

.cm-list-empty {
  padding: var(--space-3);
  text-align: center;
}

.cm-script-row {
  min-height: 32px;
  display: flex;
  align-items: center;
  gap: var(--space-1);
  padding: 3px 5px 3px 9px;
  border-radius: var(--radius-sm);
  cursor: pointer;
}

.cm-script-row:hover {
  background: var(--bg-hover);
}

.cm-script-row.active {
  background: var(--bg-selected);
}

.cm-script-name {
  min-width: 0;
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
}

.cm-row-actions {
  display: none;
  flex: none;
  align-items: center;
  gap: 1px;
}

.cm-script-row:hover .cm-row-actions,
.cm-script-row.active .cm-row-actions {
  display: flex;
}

.cm-rename-input {
  min-width: 0;
  flex: 1;
  height: 25px;
}

.cm-editor-pane {
  min-width: 0;
  min-height: 0;
  flex: 1;
  display: flex;
  flex-direction: column;
}

.cm-editor-toolbar {
  min-width: 0;
  min-height: 42px;
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: 6px 10px;
  border-bottom: 1px solid var(--border);
}

.cm-current-name {
  max-width: 160px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 600;
}

.cm-dirty {
  flex: none;
  color: var(--warning);
  font-size: 11px;
  font-weight: 600;
}

.cm-toolbar-spacer {
  flex: 1;
}

.cm-debug-result {
  min-width: 40px;
  flex: 1;
  overflow: hidden;
  color: var(--success);
  font-size: 11px;
  text-align: right;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.cm-debug-result.error {
  color: var(--danger);
}

.cm-log-id {
  width: 104px;
  flex: none;
}

.cm-statusbar {
  flex: none;
  padding: 4px 12px;
  border-top: 1px solid var(--border);
  font-size: 11px;
}

.cm-editor-empty {
  flex: 1;
  display: grid;
  place-items: center;
}
</style>
