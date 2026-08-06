<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { Io5Add, Io5Play, Io5RadioButtonOn, Io5Save, Io5Trash } from "vue-icons-plus/io5";
import type { HttpApiChange, HttpApiResource } from "../api/types";
import MonacoEditor from "./MonacoEditor.vue";
import { appStore, reportError } from "../stores/app";
import { confirmDialog } from "../stores/dialog";
import { logsStore } from "../stores/logs";
import { HTTP_API_CHANGE_EVENT } from "../stores/http-api-sync";
import { sessionsStore } from "../stores/sessions";
import { windowsStore } from "../stores/windows";

export interface ScriptLibraryEntry {
  name: string;
  usage_count?: number;
  /** Present when the list API returns full content (column/filter scripts). */
  content?: string;
}

export interface ScriptLibraryApi {
  list(): Promise<ScriptLibraryEntry[]>;
  get(name: string): Promise<{ content: string }>;
  create(name: string, content: string): Promise<void>;
  update(name: string, content: string): Promise<void>;
  remove(name: string): Promise<void>;
}

export interface ScriptLibraryDebugRun {
  /** "log-id": only a Log ID input; "log-id-input": adds a string argument input. */
  mode: "log-id" | "log-id-input";
  /** Label shown when the debug result is an empty string. */
  emptyLabel?: string;
  run(ctx: {
    name: string;
    sessionId: number;
    logId: number;
    input: string;
  }): Promise<{ result: string; failed: boolean }>;
}

export interface ScriptLibrarySelection {
  /** Load the currently active selection name (e.g. active routing rule). */
  get(): Promise<string | null>;
  /** Set the active selection; returns the new active name. */
  set(name: string | null): Promise<string | null>;
  /** Called after list/selection refresh and after toggling the selection. */
  onChanged?(): Promise<void>;
}

export interface ScriptLibraryConfig {
  /** windowsStore window id; must match the id the window was opened with. */
  windowId: string;
  /** Used in dialogs and error messages (e.g. "列脚本", "分流规则"). */
  sidebarTitle: string;
  createPlaceholder: string;
  emptyListHint: string;
  editorEmptyHint: string;
  statusbar?: string;
  defaultSource: string;
  /** HttpApiChange resources this window reacts to. */
  resources: HttpApiResource[];
  /** Custom window event dispatched after create/update/delete. */
  changeEvent?: string;
  /** Also refresh the log table view after create/delete (column scripts). */
  refreshLogsView?: boolean;
  /** Clear the debug result when the active session changes. */
  clearDebugOnSessionChange?: boolean;
  /** Active-selection support (routing rules). */
  selection?: ScriptLibrarySelection;
  /** Scope for the window-level focus-script event; only matching scope is handled. */
  focusScope?: string;
  validateName?(name: string): string | null;
  deleteMessage?(entry: ScriptLibraryEntry, dirty: boolean): string;
  debug?: ScriptLibraryDebugRun | null;
  api: ScriptLibraryApi;
}

const props = defineProps<{ config: ScriptLibraryConfig; initialName?: string }>();

const config = props.config;

const root = ref<HTMLElement | null>(null);
const editor = ref<{ focus: () => void } | null>(null);
const scripts = ref<ScriptLibraryEntry[]>([]);
const selectedName = ref<string | null>(null);
const activeName = ref<string | null>(null);
const content = ref("");
const newName = ref("");
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

const usageBadge = computed(() => {
  const script = selectedScript.value;
  if (!script || script.usage_count === undefined) return "";
  return script.usage_count ? `${script.usage_count} 个会话使用` : "未使用";
});

const displayDebugResult = computed(() => debugResult.value || config.debug?.emptyLabel || "");

function notifyChanged(): void {
  if (config.changeEvent) window.dispatchEvent(new CustomEvent(config.changeEvent));
}

function validateName(name: string): string | null {
  if (config.validateName) return config.validateName(name);
  if (!name) return "名称不能为空";
  if (name.length > 128) return "名称不能超过 128 个字符";
  if (name === "." || name === ".." || !/^[\p{L}\p{N}._-]+$/u.test(name)) {
    return "名称只能包含字母、数字、点、短横线和下划线";
  }
  return null;
}

function deleteMessage(script: ScriptLibraryEntry): string {
  if (config.deleteMessage) return config.deleteMessage(script, dirty.value);
  return `确定删除脚本「${script.name}」吗？该操作不可撤销。`;
}

function clearDebugResult(): void {
  debugResult.value = null;
  debugFailed.value = false;
}

async function loadScript(script: ScriptLibraryEntry | null): Promise<void> {
  selectedName.value = script?.name ?? null;
  clearDebugResult();
  if (!script) {
    content.value = "";
    dirty.value = false;
    return;
  }
  if (typeof script.content !== "string") {
    // List API does not include content (interceptors): fetch it lazily.
    content.value = "";
    dirty.value = false;
    try {
      const loaded = await config.api.get(script.name);
      script.content = loaded.content;
      content.value = loaded.content;
    } catch (error) {
      reportError(error, `加载${config.sidebarTitle}失败`);
      return;
    }
  } else {
    content.value = script.content;
    dirty.value = false;
  }
  void nextTick(() => editor.value?.focus());
}

async function refreshScripts(preferredName?: string): Promise<void> {
  try {
    const [items, active] = config.selection
      ? await Promise.all([config.api.list(), config.selection.get()])
      : [await config.api.list(), null];
    scripts.value = items;
    activeName.value = active ?? null;
    const next =
      scripts.value.find((script) => script.name === preferredName) ??
      scripts.value.find((script) => script.name === selectedName.value) ??
      scripts.value[0] ??
      null;
    await loadScript(next);
    if (config.selection?.onChanged) await config.selection.onChanged();
  } catch (error) {
    reportError(error, `加载${config.sidebarTitle}失败`);
  }
}

function onHttpApiChange(event: Event): void {
  const { resources } = (event as CustomEvent<HttpApiChange>).detail;
  if (
    !resources.includes("all") &&
    !config.resources.some((resource) => resources.includes(resource))
  ) {
    return;
  }
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
    message: `${config.sidebarTitle}「${selectedName.value ?? ""}」包含未保存的更改，确定放弃吗？`,
    confirmText: "放弃",
    danger: true,
  });
}

async function selectScript(script: ScriptLibraryEntry): Promise<void> {
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
  if (!name || saving.value) return { ok: false, message: `没有可保存的${config.sidebarTitle}` };
  saving.value = true;
  try {
    await config.api.update(name, content.value);
    const item = scripts.value.find((script) => script.name === name);
    if (item) item.content = content.value;
    dirty.value = false;
    externalChanged.value = false;
    notifyChanged();
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
  if (debugging.value || !config.debug) return;
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
    const out = await config.debug.run({
      name,
      sessionId,
      logId,
      input: debugInput.value,
    });
    debugResult.value = out.result;
    debugFailed.value = out.failed;
  } catch (error) {
    debugResult.value = error instanceof Error ? error.message : String(error);
    debugFailed.value = true;
  } finally {
    debugging.value = false;
  }
}

async function toggleActive(): Promise<void> {
  if (!selectedName.value || !config.selection) return;
  const name = activeName.value === selectedName.value ? null : selectedName.value;
  try {
    activeName.value = (await config.selection.set(name)) ?? null;
    if (config.selection.onChanged) await config.selection.onChanged();
    appStore.toast(name ? `${config.sidebarTitle}已生效` : "已取消分流规则", "success");
  } catch (error) {
    reportError(error, "切换分流规则失败");
    void refreshScripts(selectedName.value ?? undefined);
  }
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
    await config.api.create(name, config.defaultSource);
    newName.value = "";
    await refreshScripts(name);
    notifyChanged();
    if (config.refreshLogsView) await logsStore.refreshView();
  } catch (error) {
    reportError(error, `创建${config.sidebarTitle}失败`);
  }
}

async function removeScript(script: ScriptLibraryEntry): Promise<void> {
  const ok = await confirmDialog({
    title: `删除${config.sidebarTitle}`,
    message: deleteMessage(script),
    confirmText: "删除",
    danger: true,
  });
  if (!ok) return;
  try {
    await config.api.remove(script.name);
    scripts.value = scripts.value.filter((item) => item.name !== script.name);
    if (selectedName.value === script.name) void loadScript(scripts.value[0] ?? null);
    notifyChanged();
    if (config.refreshLogsView) await logsStore.refreshView();
  } catch (error) {
    reportError(error, `删除${config.sidebarTitle}失败`);
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

function onFocusScript(event: Event): void {
  const detail = (event as CustomEvent<{ scope?: string; name: string }>).detail;
  if (!config.focusScope || detail.scope !== config.focusScope) return;
  const script = scripts.value.find((item) => item.name === detail.name);
  if (script) {
    void selectScript(script);
  } else {
    // List not loaded yet (freshly opened window): let refresh pick the target.
    void refreshScripts(detail.name);
  }
}

watch(
  () => sessionsStore.viewingSessionId,
  () => {
    if (config.clearDebugOnSessionChange) clearDebugResult();
  },
);

onMounted(() => {
  windowsStore.registerCloseGuard(config.windowId, confirmDiscard);
  window.addEventListener("keydown", onKeydown);
  window.addEventListener(HTTP_API_CHANGE_EVENT, onHttpApiChange);
  window.addEventListener("proxycrab-focus-script", onFocusScript);
  void refreshScripts(props.initialName ?? undefined);
});

onBeforeUnmount(() => {
  windowsStore.unregisterCloseGuard(config.windowId);
  window.removeEventListener("keydown", onKeydown);
  window.removeEventListener(HTTP_API_CHANGE_EVENT, onHttpApiChange);
  window.removeEventListener("proxycrab-focus-script", onFocusScript);
});
</script>

<template>
  <div ref="root" class="slm-root">
    <aside class="slm-sidebar">
      <div class="slm-create">
        <input
          v-model="newName"
          class="input"
          :placeholder="config.createPlaceholder"
          @keyup.enter="createScript"
        />
        <button class="btn icon primary" title="新建" @click="createScript">
          <Io5Add :size="15" />
        </button>
      </div>
      <div class="slm-script-list">
        <div v-if="!scripts.length" class="empty-hint slm-list-empty">
          {{ config.emptyListHint }}
        </div>
        <div
          v-for="script in scripts"
          :key="script.name"
          class="slm-script-row"
          :class="{ active: selectedName === script.name }"
          @click="selectScript(script)"
        >
          <span class="slm-script-name mono" :title="script.name">{{ script.name }}</span>
          <span
            v-if="config.selection && activeName === script.name"
            class="slm-active"
          >生效中</span>
          <span class="slm-row-actions">
            <button class="btn icon danger" title="删除" @click.stop="removeScript(script)">
              <Io5Trash :size="14" />
            </button>
          </span>
        </div>
      </div>
    </aside>

    <section class="slm-editor-pane">
      <div v-if="externalChanged" class="slm-external">
        <span>脚本已被外部修改，未保存内容仍保留在编辑器中。</span>
        <button class="btn compact" @click="reloadExternal">重新加载</button>
      </div>
      <template v-if="selectedScript">
        <div class="slm-editor-toolbar">
          <span class="slm-current-name mono" :title="selectedName ?? ''">{{ selectedName }}</span>
          <span v-if="usageBadge" class="slm-usage">{{ usageBadge }}</span>
          <span v-if="dirty" class="slm-dirty">未保存</span>
          <span
            v-if="debugResult !== null"
            class="slm-debug-result mono"
            :class="{ error: debugFailed }"
            :title="debugResult"
          >
            {{ displayDebugResult }}
          </span>
          <span v-else class="slm-toolbar-spacer" />
          <input
            v-if="config.debug?.mode === 'log-id-input'"
            v-model="debugInput"
            class="input slm-argument mono"
            placeholder="字符串参数"
            title="通过 Lua ... 传入的字符串"
            @input="clearDebugResult"
            @keyup.enter="saveAndRun"
          />
          <input
            v-if="config.debug"
            v-model="debugLogId"
            class="input slm-log-id mono"
            inputmode="numeric"
            placeholder="Log ID"
            title="当前会话中的 Log ID"
            @input="clearDebugResult"
            @keyup.enter="saveAndRun"
          />
          <button
            v-if="config.selection"
            class="btn"
            :disabled="!selectedName || saving"
            @click="toggleActive"
          >
            <Io5RadioButtonOn :size="14" />
            {{ activeName === selectedName ? "取消生效" : "设为生效" }}
          </button>
          <button class="btn" :disabled="saving || debugging" @click="save">
            <Io5Save :size="14" />
            {{ saving && !debugging ? "保存中…" : "保存" }}
          </button>
          <button
            v-if="config.debug"
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
        <div v-if="config.statusbar" class="slm-statusbar text-faint">{{ config.statusbar }}</div>
      </template>
      <div v-else class="slm-editor-empty">
        <span class="empty-hint">{{ config.editorEmptyHint }}</span>
      </div>
    </section>
  </div>
</template>

<style scoped>
.slm-root {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
}

.slm-sidebar {
  width: 236px;
  min-width: 190px;
  display: flex;
  flex-direction: column;
  border-right: 1px solid var(--border);
  padding: 8px 0 0 0;
  background: var(--bg-panel);
}

.slm-external {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid var(--warning);
  color: var(--warning);
  font-size: 12px;
  flex: none;
}
.slm-external span {
  flex: 1;
}

.slm-create {
  display: flex;
  gap: var(--space-1);
  padding: 0 var(--space-2) var(--space-2);
}
.slm-create .input {
  min-width: 0;
  flex: 1;
}

.slm-script-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 0 var(--space-1) var(--space-2);
}
.slm-list-empty {
  padding: var(--space-3);
  text-align: center;
}

.slm-script-row {
  min-height: 32px;
  display: flex;
  align-items: center;
  gap: var(--space-1);
  padding: 3px 5px 3px 9px;
  border-radius: var(--radius-sm);
  cursor: pointer;
}
.slm-script-row:hover {
  background: var(--bg-hover);
}
.slm-script-row.active {
  background: var(--bg-selected);
}
.slm-script-name {
  min-width: 0;
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
}
.slm-active {
  flex: none;
  color: var(--accent);
  font-size: 10px;
}
.slm-row-actions {
  display: none;
  flex: none;
  align-items: center;
  gap: 1px;
}
.slm-script-row:hover .slm-row-actions,
.slm-script-row.active .slm-row-actions {
  display: flex;
}

.slm-editor-pane {
  min-width: 0;
  min-height: 0;
  flex: 1;
  display: flex;
  flex-direction: column;
}

.slm-editor-toolbar {
  min-width: 0;
  min-height: 42px;
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: 6px 10px;
  border-bottom: 1px solid var(--border);
}
.slm-current-name {
  max-width: 160px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 600;
}
.slm-usage {
  flex: none;
  color: var(--text-faint);
  font-size: 11px;
}
.slm-dirty {
  flex: none;
  color: var(--warning);
  font-size: 11px;
  font-weight: 600;
}
.slm-toolbar-spacer {
  flex: 1;
}
.slm-debug-result {
  min-width: 40px;
  flex: 1;
  overflow: hidden;
  color: var(--success);
  font-size: 11px;
  text-align: right;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.slm-debug-result.error {
  color: var(--danger);
}
.slm-argument {
  width: 150px;
  flex: none;
}
.slm-log-id {
  width: 104px;
  flex: none;
}

.slm-statusbar {
  flex: none;
  padding: 4px 12px;
  border-top: 1px solid var(--border);
  font-size: 11px;
}

.slm-editor-empty {
  flex: 1;
  display: grid;
  place-items: center;
}
</style>
