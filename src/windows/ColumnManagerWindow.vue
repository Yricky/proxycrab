<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { Io5Add, Io5Trash } from "vue-icons-plus/io5";
import { useBackend } from "../api";
import type { Column, ColumnInput, Script } from "../api/types";
import { reportError } from "../stores/app";
import { confirmDialog } from "../stores/dialog";
import { openScriptEditor } from "./launcher";

const backend = useBackend();

const scripts = ref<Script[]>([]);
const columns = ref<Column[]>([]);
const newScriptName = ref("");

type AddKind = "method" | "uri" | "code" | "source" | "stage" | "script";
const addKind = ref<AddKind>("method");
const addScriptName = ref("");

const BUILTIN_KINDS: { value: AddKind; label: string }[] = [
  { value: "method", label: "方法" },
  { value: "uri", label: "URI" },
  { value: "code", label: "状态码" },
  { value: "source", label: "来源" },
  { value: "stage", label: "阶段" },
  { value: "script", label: "脚本列" },
];

const KIND_LABELS: Record<string, string> = {
  method: "方法",
  uri: "URI",
  code: "状态码",
  source: "来源",
  stage: "阶段",
};

const canAdd = computed(
  () => addKind.value !== "script" || addScriptName.value.length > 0,
);

function columnName(col: Column): string {
  return col.kind === "script" ? col.script_name : (KIND_LABELS[col.kind] ?? col.kind);
}

function validateName(name: string): string | null {
  if (!name) return "名称不能为空";
  if (name.includes("/") || name.includes("\\")) return "名称不能包含路径分隔符（/ 或 \\）";
  return null;
}

async function refreshScripts(): Promise<void> {
  try {
    scripts.value = await backend.listColumnScripts();
    if (!scripts.value.some((s) => s.name === addScriptName.value)) {
      addScriptName.value = scripts.value[0]?.name ?? "";
    }
  } catch (error) {
    reportError(error, "加载列脚本失败");
  }
}

async function refreshColumns(): Promise<void> {
  try {
    columns.value = await backend.listColumns();
  } catch (error) {
    reportError(error, "加载可见列失败");
  }
}

async function createScript(): Promise<void> {
  const name = newScriptName.value.trim();
  const problem = validateName(name);
  if (problem) {
    reportError(problem);
    return;
  }
  try {
    await backend.createColumnScript({ name, content: "" });
    newScriptName.value = "";
    await refreshScripts();
    openScriptEditor("column", name);
  } catch (error) {
    reportError(error, "创建列脚本失败");
  }
}

async function removeScript(script: Script): Promise<void> {
  const ok = await confirmDialog({
    title: "删除列脚本",
    message: `确定删除列脚本「${script.name}」吗？该操作不可撤销。`,
    confirmText: "删除",
    danger: true,
  });
  if (!ok) return;
  try {
    await backend.deleteColumnScript(script.name);
    await refreshScripts();
  } catch (error) {
    reportError(error, "删除列脚本失败");
  }
}

async function updateWidth(index: number, col: Column, event: Event): Promise<void> {
  const width = Number((event.target as HTMLInputElement).value);
  if (!Number.isFinite(width) || width <= 0) {
    reportError("宽度必须为正数");
    return;
  }
  if (width === col.width) return;
  const input: ColumnInput =
    col.kind === "script"
      ? { kind: col.kind, width, script_name: col.script_name }
      : { kind: col.kind, width };
  try {
    columns.value = await backend.replaceColumn(index, input);
  } catch (error) {
    reportError(error, "更新列宽失败");
    await refreshColumns();
  }
}

async function removeColumn(index: number): Promise<void> {
  try {
    columns.value = await backend.deleteColumn(index);
  } catch (error) {
    reportError(error, "删除列失败");
  }
}

async function addColumn(): Promise<void> {
  const input: ColumnInput =
    addKind.value === "script"
      ? { kind: "script", script_name: addScriptName.value, width: 160 }
      : { kind: addKind.value, width: 160 };
  try {
    columns.value = await backend.appendColumn(input);
  } catch (error) {
    reportError(error, "添加列失败");
  }
}

onMounted(() => {
  void refreshScripts();
  void refreshColumns();
});
</script>

<template>
  <div class="cm-root">
    <section class="cm-section">
      <h3 class="section-title">列脚本</h3>
      <div class="cm-actions">
        <input
          v-model="newScriptName"
          class="input cm-name-input"
          placeholder="新列脚本名"
          @keyup.enter="createScript"
        />
        <button class="btn primary" @click="createScript"><Io5Add :size="14" /> 新建</button>
      </div>
      <div class="cm-script-list">
        <div v-if="!scripts.length" class="empty-hint">暂无列脚本</div>
        <div v-for="script in scripts" :key="script.name" class="cm-row">
          <span
            class="cm-name mono clickable"
            :title="`编辑 ${script.name}`"
            @click="openScriptEditor('column', script.name)"
          >
            {{ script.name }}
          </span>
          <span class="cm-spacer" />
          <button class="btn icon cm-danger" title="删除" @click="removeScript(script)">
            <Io5Trash :size="14" />
          </button>
        </div>
      </div>
    </section>

    <section class="cm-section cm-columns">
      <h3 class="section-title">可见列</h3>
      <div class="cm-column-list">
        <div v-if="!columns.length" class="empty-hint">暂无可见列</div>
        <div v-for="(col, index) in columns" :key="index" class="cm-row">
          <span class="cm-col-name">{{ columnName(col) }}</span>
          <span class="badge">{{ col.kind }}</span>
          <span class="cm-spacer" />
          <input
            type="number"
            class="input cm-width"
            :value="col.width"
            min="1"
            title="列宽（像素），失焦生效"
            @change="updateWidth(index, col, $event)"
          />
          <button class="btn icon cm-danger" title="删除" @click="removeColumn(index)">
            <Io5Trash :size="14" />
          </button>
        </div>
      </div>
      <div class="cm-actions">
        <select v-model="addKind" class="select">
          <option v-for="k in BUILTIN_KINDS" :key="k.value" :value="k.value">
            {{ k.label }}
          </option>
        </select>
        <select v-if="addKind === 'script'" v-model="addScriptName" class="select">
          <option v-for="s in scripts" :key="s.name" :value="s.name">{{ s.name }}</option>
        </select>
        <button class="btn" :disabled="!canAdd" @click="addColumn">
          <Io5Add :size="14" /> 添加列
        </button>
      </div>
    </section>
  </div>
</template>

<style scoped>
.cm-root {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow-y: auto;
  padding: var(--space-3);
  gap: var(--space-4);
}

.cm-section {
  display: flex;
  flex-direction: column;
  flex: none;
}

.cm-actions {
  display: flex;
  gap: var(--space-2);
  margin-bottom: var(--space-2);
}

.cm-name-input {
  flex: 1;
  min-width: 0;
}

.cm-script-list {
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  max-height: 140px;
  overflow-y: auto;
}

.cm-columns {
  flex: 1;
  min-height: 0;
}

.cm-column-list {
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  max-height: 180px;
  overflow-y: auto;
  margin-bottom: var(--space-2);
}

.cm-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: 4px var(--space-2);
}
.cm-row:hover {
  background: var(--bg-hover);
}
.cm-row + .cm-row {
  border-top: 1px solid var(--border);
}

.cm-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.cm-name:hover {
  color: var(--accent);
}

.cm-col-name {
  min-width: 72px;
}

.cm-spacer {
  flex: 1;
}

.cm-width {
  width: 80px;
}

.cm-danger:hover:not(:disabled) {
  color: var(--danger);
}
</style>
