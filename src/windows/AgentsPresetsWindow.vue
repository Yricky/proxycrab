<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import {
  Io5Add,
  Io5CheckmarkCircle,
  Io5Refresh,
  Io5Save,
  Io5Trash,
} from "vue-icons-plus/io5";
import { useBackend } from "../api";
import type { AgentsPreset, AgentsPresetState } from "../api/types";
import MonacoEditor from "../components/MonacoEditor.vue";
import { appStore, reportError } from "../stores/app";
import { confirmDialog } from "../stores/dialog";
import { windowsStore } from "../stores/windows";

const WINDOW_ID = "agents-presets";
const backend = useBackend();

const root = ref<HTMLElement | null>(null);
const editor = ref<{ focus: () => void } | null>(null);
const state = ref<AgentsPresetState>({ active_id: "", presets: [] });
const selectedId = ref<string | null>(null);
const draftName = ref("");
const draftContent = ref("");
const newPresetName = ref("");
const loading = ref(false);
const saving = ref(false);
const externalChanged = ref(false);
const pendingExternalState = ref<AgentsPresetState | null>(null);

const selectedPreset = computed(
  () => state.value.presets.find((preset) => preset.id === selectedId.value) ?? null,
);
const dirty = computed(
  () =>
    selectedPreset.value !== null &&
    (draftName.value !== selectedPreset.value.name ||
      draftContent.value !== selectedPreset.value.content),
);
const selectedIsActive = computed(() => selectedId.value === state.value.active_id);

function loadPreset(preset: AgentsPreset | null): void {
  selectedId.value = preset?.id ?? null;
  draftName.value = preset?.name ?? "";
  draftContent.value = preset?.content ?? "";
  if (preset) void nextTick(() => editor.value?.focus());
}

function applyState(next: AgentsPresetState, preferredId?: string | null): void {
  state.value = next;
  externalChanged.value = false;
  pendingExternalState.value = null;
  const preset =
    next.presets.find((item) => item.id === preferredId) ??
    next.presets.find((item) => item.id === next.active_id) ??
    next.presets[0] ??
    null;
  loadPreset(preset);
}

function statesMatch(left: AgentsPresetState, right: AgentsPresetState): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

async function refresh(): Promise<void> {
  loading.value = true;
  try {
    applyState(await backend.getAgentsPresets(), selectedId.value);
  } catch (error) {
    reportError(error, "加载 AGENTS.md 预设失败");
  } finally {
    loading.value = false;
  }
}

async function checkExternalChanges(): Promise<void> {
  try {
    const next = await backend.getAgentsPresets();
    if (statesMatch(next, state.value)) return;
    if (dirty.value) {
      pendingExternalState.value = next;
      externalChanged.value = true;
    } else {
      applyState(next, selectedId.value);
    }
  } catch (error) {
    reportError(error, "检查 AGENTS.md 外部修改失败");
  }
}

async function reloadExternal(): Promise<void> {
  if (!(await confirmDiscard())) return;
  const next = pendingExternalState.value;
  if (next) applyState(next, selectedId.value);
  else await refresh();
}

async function confirmDiscard(): Promise<boolean> {
  if (!dirty.value) return true;
  return confirmDialog({
    title: "放弃未保存更改",
    message: `预设「${selectedPreset.value?.name ?? ""}」包含未保存的更改，确定放弃吗？`,
    confirmText: "放弃",
    danger: true,
  });
}

async function selectPreset(preset: AgentsPreset): Promise<void> {
  if (preset.id === selectedId.value) return;
  if (!(await confirmDiscard())) return;
  loadPreset(preset);
}

async function save(showToast = true): Promise<boolean> {
  const id = selectedId.value;
  const name = draftName.value.trim();
  if (!id || saving.value) return false;
  if (!name) {
    reportError("名称不能为空");
    return false;
  }
  saving.value = true;
  try {
    applyState(
      await backend.updateAgentsPreset(id, {
        name,
        content: draftContent.value,
      }),
      id,
    );
    if (showToast) appStore.toast("AGENTS.md 预设已保存", "success");
    return true;
  } catch (error) {
    reportError(error, "保存 AGENTS.md 预设失败");
    return false;
  } finally {
    saving.value = false;
  }
}

async function createPreset(): Promise<void> {
  const name = newPresetName.value.trim();
  if (!name) {
    reportError("名称不能为空");
    return;
  }
  if (!(await confirmDiscard())) return;
  try {
    const next = await backend.createAgentsPreset({ name });
    const created = next.presets.find((preset) => preset.name === name);
    newPresetName.value = "";
    applyState(next, created?.id);
  } catch (error) {
    reportError(error, "新建 AGENTS.md 预设失败");
  }
}

async function activateSelected(): Promise<void> {
  const id = selectedId.value;
  if (!id || selectedIsActive.value) return;
  if (dirty.value && !(await save(false))) return;
  try {
    applyState(await backend.activateAgentsPreset(id), id);
    appStore.toast("已激活该 AGENTS.md 预设", "success");
  } catch (error) {
    reportError(error, "激活 AGENTS.md 预设失败");
  }
}

async function removePreset(preset: AgentsPreset): Promise<void> {
  const ok = await confirmDialog({
    title: "删除 AGENTS.md 预设",
    message:
      preset.id === state.value.active_id
        ? `「${preset.name}」当前已激活。删除后将自动激活下一份预设，确定继续吗？`
        : `确定删除预设「${preset.name}」吗？该操作不可撤销。`,
    confirmText: "删除",
    danger: true,
  });
  if (!ok) return;
  try {
    const preferred = preset.id === selectedId.value ? null : selectedId.value;
    applyState(await backend.deleteAgentsPreset(preset.id), preferred);
  } catch (error) {
    reportError(error, "删除 AGENTS.md 预设失败");
  }
}

async function reimportDefaults(): Promise<void> {
  if (!(await confirmDiscard())) return;
  const ok = await confirmDialog({
    title: "重新导入默认预设",
    message: "将覆盖同名默认预设的内容，其他预设和当前激活项保持不变。确定继续吗？",
    confirmText: "重新导入",
  });
  if (!ok) return;
  try {
    applyState(await backend.reimportDefaultAgentsPresets(), selectedId.value);
    appStore.toast("默认预设已重新导入", "success");
  } catch (error) {
    reportError(error, "重新导入默认预设失败");
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
  window.addEventListener("focus", checkExternalChanges);
  void refresh();
});

onBeforeUnmount(() => {
  windowsStore.unregisterCloseGuard(WINDOW_ID);
  window.removeEventListener("keydown", onKeydown);
  window.removeEventListener("focus", checkExternalChanges);
});
</script>

<template>
  <div ref="root" class="ap-root">
    <aside class="ap-sidebar">
      <div class="ap-sidebar-heading">
        <div>
          <strong>AGENTS.md</strong>
          <span>Workspace 预设</span>
        </div>
      </div>
      <div class="ap-create">
        <input
          v-model="newPresetName"
          class="input"
          placeholder="新预设名称"
          @keyup.enter="createPreset"
        />
        <button class="btn icon primary" title="新建预设" @click="createPreset">
          <Io5Add :size="15" />
        </button>
      </div>
      <div class="ap-list">
        <div v-if="loading" class="empty-hint ap-empty">正在加载…</div>
        <div
          v-for="preset in state.presets"
          :key="preset.id"
          class="ap-row"
          :class="{ selected: selectedId === preset.id }"
          role="button"
          tabindex="0"
          @click="selectPreset(preset)"
          @keydown.enter="selectPreset(preset)"
        >
          <span class="ap-row-state">
            <Io5CheckmarkCircle
              v-if="state.active_id === preset.id"
              :size="14"
              title="当前激活"
            />
          </span>
          <span class="ap-row-name" :title="preset.name">{{ preset.name }}</span>
          <button
            class="btn icon danger ap-delete"
            title="删除预设"
            :disabled="state.presets.length <= 1"
            @click.stop="removePreset(preset)"
          >
            <Io5Trash :size="13" />
          </button>
        </div>
      </div>
      <div class="ap-sidebar-footer">
        <button class="btn compact" @click="reimportDefaults">
          <Io5Refresh :size="13" />
          重新导入默认预设
        </button>
      </div>
    </aside>

    <section class="ap-editor-pane">
      <div v-if="externalChanged" class="ap-external">
        <span>预设已在 workspace 中被外部修改，未保存内容仍保留在编辑器中。</span>
        <button class="btn compact" @click="reloadExternal">重新加载</button>
      </div>
      <template v-if="selectedPreset">
        <div class="ap-toolbar">
          <input
            v-model="draftName"
            class="input ap-name"
            aria-label="预设名称"
            maxlength="128"
          />
          <span v-if="selectedIsActive" class="ap-active-label">当前激活</span>
          <span v-if="dirty" class="ap-dirty">未保存</span>
          <span class="ap-spacer" />
          <button
            v-if="!selectedIsActive"
            class="btn"
            :disabled="saving"
            @click="activateSelected"
          >
            激活
          </button>
          <button class="btn primary" :disabled="saving || !dirty" @click="save()">
            <Io5Save :size="14" />
            {{ saving ? "保存中…" : "保存" }}
          </button>
        </div>
        <MonacoEditor
          ref="editor"
          :model-value="draftContent"
          language="markdown"
          :options="{ fontSize: 13, lineNumbersMinChars: 3 }"
          @update:model-value="draftContent = $event"
        />
        <div class="ap-statusbar">
          <span>当前激活内容通过 <code>GET /api/agents.md</code> 提供给 Agent</span>
          <span class="mono">{{ selectedPreset.id }}.md</span>
        </div>
      </template>
      <div v-else class="ap-editor-empty">
        <span class="empty-hint">暂无可编辑预设</span>
      </div>
    </section>
  </div>
</template>

<style scoped>
.ap-root {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
}
.ap-sidebar {
  width: 250px;
  min-width: 210px;
  display: flex;
  flex-direction: column;
  border-right: 1px solid var(--border);
  background: var(--bg-panel);
}
.ap-sidebar-heading {
  padding: 12px 12px 8px;
}
.ap-sidebar-heading > div {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 8px;
}
.ap-sidebar-heading strong {
  font-size: 12px;
}
.ap-sidebar-heading span {
  color: var(--text-faint);
  font-size: 11px;
}
.ap-create {
  display: flex;
  gap: 4px;
  padding: 0 8px 8px;
}
.ap-create .input {
  min-width: 0;
  flex: 1;
}
.ap-list {
  min-height: 0;
  flex: 1;
  overflow: auto;
  padding: 2px 6px 8px;
}
.ap-empty {
  padding: 18px 8px;
  text-align: center;
}
.ap-row {
  width: 100%;
  min-height: 34px;
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 3px 4px 3px 7px;
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text);
  font: inherit;
  text-align: left;
  cursor: pointer;
}
.ap-row:hover {
  background: var(--bg-hover);
}
.ap-row.selected {
  background: var(--bg-selected);
}
.ap-row-state {
  width: 14px;
  height: 14px;
  flex: none;
  color: var(--accent);
}
.ap-row-name {
  min-width: 0;
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.ap-delete {
  opacity: 0;
}
.ap-row:hover .ap-delete,
.ap-row.selected .ap-delete {
  opacity: 1;
}
.ap-sidebar-footer {
  padding: 8px;
  border-top: 1px solid var(--border);
}
.ap-sidebar-footer .btn {
  width: 100%;
  justify-content: center;
}
.ap-editor-pane {
  min-width: 0;
  min-height: 0;
  flex: 1;
  display: flex;
  flex-direction: column;
  background: var(--bg-app);
}
.ap-external {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 10px;
  border-bottom: 1px solid var(--warning);
  color: var(--warning);
  background: var(--bg-panel);
  font-size: 12px;
}
.ap-external span {
  min-width: 0;
  flex: 1;
}
.ap-toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  border-bottom: 1px solid var(--border);
  background: var(--bg-panel);
}
.ap-name {
  width: min(320px, 42%);
  font-weight: 600;
}
.ap-active-label,
.ap-dirty {
  font-size: 11px;
}
.ap-active-label {
  color: var(--accent);
}
.ap-dirty {
  color: var(--warning);
}
.ap-spacer {
  flex: 1;
}
.ap-statusbar {
  min-height: 27px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 4px 10px;
  border-top: 1px solid var(--border);
  color: var(--text-faint);
  background: var(--bg-panel);
  font-size: 11px;
}
.ap-statusbar code {
  color: var(--text-secondary);
  font-family: var(--font-mono);
}
.ap-editor-empty {
  flex: 1;
  display: grid;
  place-items: center;
}
</style>
