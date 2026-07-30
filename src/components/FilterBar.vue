<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import {
  Io5Checkmark,
  Io5ChevronDown,
  Io5Close,
  Io5CodeSlash,
  Io5ReturnDownBack,
  Io5Text,
} from "vue-icons-plus/io5";
import { useBackend } from "../api";
import type {
  FilterColumn,
  FilterOption,
  HttpApiChange,
  Script,
  SessionFilter,
} from "../api/types";
import { reportError } from "../stores/app";
import {
  cloneSessionFilter,
  emptySessionFilter,
  logsStore,
} from "../stores/logs";
import { sessionsStore } from "../stores/sessions";
import { HTTP_API_CHANGE_EVENT } from "../stores/http-api-sync";

interface BuiltinColumnChoice {
  kind: Exclude<FilterColumn["kind"], "script">;
  label: string;
}

const BUILTIN_COLUMNS: BuiltinColumnChoice[] = [
  { kind: "method", label: "Method" },
  { kind: "uri", label: "URI" },
  { kind: "code", label: "Code" },
  { kind: "source", label: "Source" },
  { kind: "stage", label: "Stage" },
];

const backend = useBackend();
const root = ref<HTMLElement | null>(null);
const inputEl = ref<HTMLInputElement | null>(null);
const menuOpen = ref(false);
const applying = ref(false);
const columnScripts = ref<Script[]>([]);
const filterScripts = ref<Script[]>([]);
const columnCaseSensitive = ref(false);
const draft = ref<SessionFilter>(emptySessionFilter());

const dirty = computed(
  () => JSON.stringify(draft.value) !== JSON.stringify(logsStore.appliedFilter),
);
const selectedColumn = computed(() =>
  draft.value.option?.kind === "column" ? draft.value.option.column : null,
);
const targetLabel = computed(() => {
  const option = draft.value.option;
  if (!option) return "未选择";
  if (option.kind === "script") return `过滤脚本 · ${option.script_name}`;
  if (option.column.kind === "script") {
    return `自定义列 · ${option.column.script_name}`;
  }
  return `列 · ${BUILTIN_COLUMNS.find((item) => item.kind === option.column.kind)?.label ?? option.column.kind}`;
});
const placeholder = computed(() =>
  draft.value.option?.kind === "script"
    ? "输入脚本参数，回车应用"
    : "输入包含文本，回车应用",
);

watch(
  () => logsStore.appliedFilter,
  (filter) => {
    draft.value = cloneSessionFilter(filter);
    if (filter.option?.kind === "column") {
      columnCaseSensitive.value = filter.option.case_sensitive;
    }
  },
  { deep: true, immediate: true },
);

async function refreshScripts(): Promise<void> {
  try {
    const [columns, filters] = await Promise.all([
      backend.listColumnScripts(),
      backend.listFilterScripts(),
    ]);
    columnScripts.value = columns;
    filterScripts.value = filters;
  } catch (error) {
    reportError(error, "加载过滤选项失败");
  }
}

async function refreshAfterScriptChange(): Promise<void> {
  await refreshScripts();
  if (sessionsStore.viewingSessionId !== null) {
    await logsStore.loadSession();
  }
}

async function apply(): Promise<void> {
  if (!draft.value.option || !dirty.value || applying.value) return;
  applying.value = true;
  try {
    await logsStore.applyFilter(cloneSessionFilter(draft.value));
  } finally {
    applying.value = false;
  }
}

async function selectNone(): Promise<void> {
  menuOpen.value = false;
  const previous = cloneSessionFilter(logsStore.appliedFilter);
  draft.value = emptySessionFilter();
  applying.value = true;
  try {
    const applied = await logsStore.applyFilter(emptySessionFilter());
    if (!applied) draft.value = previous;
  } finally {
    applying.value = false;
  }
}

function selectColumn(column: FilterColumn): void {
  const option: FilterOption = {
    kind: "column",
    column,
    case_sensitive: columnCaseSensitive.value,
  };
  draft.value = { option, input: draft.value.input };
  menuOpen.value = false;
  inputEl.value?.focus();
}

function selectFilterScript(scriptName: string): void {
  draft.value = {
    option: { kind: "script", script_name: scriptName },
    input: draft.value.input,
  };
  menuOpen.value = false;
  inputEl.value?.focus();
}

function setCaseSensitive(event: Event): void {
  const checked = (event.target as HTMLInputElement).checked;
  columnCaseSensitive.value = checked;
  if (draft.value.option?.kind === "column") {
    draft.value = {
      option: {
        ...draft.value.option,
        case_sensitive: checked,
      },
      input: draft.value.input,
    };
  }
}

function clearInput(): void {
  draft.value = { ...draft.value, input: "" };
  inputEl.value?.focus();
}

function onDocumentPointerDown(event: PointerEvent): void {
  if (!root.value?.contains(event.target as Node)) menuOpen.value = false;
}

function onHttpApiChange(event: Event): void {
  const { resources } = (event as CustomEvent<HttpApiChange>).detail;
  if (
    resources.includes("all") ||
    resources.includes("column_scripts") ||
    resources.includes("filter_scripts")
  ) {
    void refreshScripts();
  }
}

onMounted(() => {
  void refreshScripts();
  document.addEventListener("pointerdown", onDocumentPointerDown);
  window.addEventListener("column-scripts-changed", refreshAfterScriptChange);
  window.addEventListener("filter-scripts-changed", refreshAfterScriptChange);
  window.addEventListener(HTTP_API_CHANGE_EVENT, onHttpApiChange);
});

onBeforeUnmount(() => {
  document.removeEventListener("pointerdown", onDocumentPointerDown);
  window.removeEventListener("column-scripts-changed", refreshAfterScriptChange);
  window.removeEventListener("filter-scripts-changed", refreshAfterScriptChange);
  window.removeEventListener(HTTP_API_CHANGE_EVENT, onHttpApiChange);
});
</script>

<template>
  <div ref="root" class="filter-bar">
    <div class="fb-target">
      <button
        class="btn fb-target-trigger"
        :class="{ active: menuOpen }"
        :aria-expanded="menuOpen"
        @click="menuOpen = !menuOpen"
      >
        <span class="fb-target-label" :title="targetLabel">{{ targetLabel }}</span>
        <Io5ChevronDown :size="12" />
      </button>

      <div v-if="menuOpen" class="fb-menu">
        <button class="fb-option" :class="{ selected: !draft.option }" @click="selectNone">
          <span>未选择</span>
          <Io5Checkmark v-if="!draft.option" :size="13" />
        </button>

        <div class="fb-section">
          <div class="fb-section-heading">
            <span>列</span>
            <label class="fb-case" @click.stop>
              <input
                type="checkbox"
                :checked="columnCaseSensitive"
                @change="setCaseSensitive"
              />
              <span>区分大小写</span>
            </label>
          </div>
          <button
            v-for="column in BUILTIN_COLUMNS"
            :key="column.kind"
            class="fb-option"
            :class="{ selected: selectedColumn?.kind === column.kind }"
            @click="selectColumn({ kind: column.kind })"
          >
            <Io5Text :size="13" class="fb-option-icon" />
            <span>{{ column.label }}</span>
            <Io5Checkmark
              v-if="selectedColumn?.kind === column.kind"
              :size="13"
              class="fb-option-check"
            />
          </button>
          <button
            v-for="script in columnScripts"
            :key="`column:${script.name}`"
            class="fb-option"
            :class="{
              selected:
                selectedColumn?.kind === 'script' &&
                selectedColumn.script_name === script.name,
            }"
            @click="selectColumn({ kind: 'script', script_name: script.name })"
          >
            <Io5CodeSlash :size="13" class="fb-option-icon" />
            <span class="fb-option-name">{{ script.name }}</span>
            <Io5Checkmark
              v-if="
                selectedColumn?.kind === 'script' &&
                selectedColumn.script_name === script.name
              "
              :size="13"
              class="fb-option-check"
            />
          </button>
        </div>

        <div class="fb-section">
          <div class="fb-section-heading"><span>过滤脚本</span></div>
          <div v-if="filterScripts.length === 0" class="fb-empty">暂无过滤脚本</div>
          <button
            v-for="script in filterScripts"
            :key="`filter:${script.name}`"
            class="fb-option"
            :class="{
              selected:
                draft.option?.kind === 'script' &&
                draft.option.script_name === script.name,
            }"
            @click="selectFilterScript(script.name)"
          >
            <Io5CodeSlash :size="13" class="fb-option-icon" />
            <span class="fb-option-name">{{ script.name }}</span>
            <Io5Checkmark
              v-if="
                draft.option?.kind === 'script' &&
                draft.option.script_name === script.name
              "
              :size="13"
              class="fb-option-check"
            />
          </button>
        </div>
      </div>
    </div>

    <div class="fb-input-wrap">
      <Io5ReturnDownBack
        v-if="dirty && draft.option"
        :size="15"
        class="fb-enter-hint"
        title="按回车应用"
      />
      <input
        ref="inputEl"
        v-model="draft.input"
        class="input fb-input mono"
        :class="{
          active: logsStore.filterActive && !dirty,
          dirty: dirty && draft.option,
        }"
        :disabled="!draft.option || applying"
        :placeholder="draft.option ? placeholder : '请先选择过滤列或脚本'"
        spellcheck="false"
        @keyup.enter="apply"
        @keyup.esc="clearInput"
      />
      <button
        v-if="draft.input && draft.option"
        class="btn icon fb-clear"
        title="清空输入，按回车应用"
        @click="clearInput"
      >
        <Io5Close :size="13" />
      </button>
    </div>
  </div>
</template>

<style scoped>
.filter-bar {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 8px 10px;
  flex: none;
}
.fb-target {
  position: relative;
  flex: none;
}
.fb-target-trigger {
  width: 178px;
  justify-content: space-between;
}
.fb-target-label {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.fb-menu {
  position: absolute;
  left: 0;
  top: calc(100% + 4px);
  width: 300px;
  max-height: min(460px, calc(100vh - 170px));
  overflow-y: auto;
  padding: 4px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-popup);
  z-index: 1000;
}
.fb-section {
  border-top: 1px solid var(--border);
  margin-top: 4px;
  padding-top: 4px;
}
.fb-section-heading {
  min-height: 28px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 3px 8px;
  color: var(--text-faint);
  font-size: 11px;
  font-weight: 600;
}
.fb-case {
  display: flex;
  align-items: center;
  gap: 5px;
  color: var(--text-secondary);
  font-weight: 400;
  cursor: pointer;
}
.fb-case input {
  margin: 0;
}
.fb-option {
  width: 100%;
  min-height: 30px;
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 4px 8px;
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text);
  font: inherit;
  text-align: left;
  cursor: pointer;
}
.fb-option:hover {
  background: var(--bg-hover);
}
.fb-option.selected {
  background: var(--bg-selected);
  color: var(--accent);
}
.fb-option-icon {
  flex: none;
  color: var(--text-faint);
}
.fb-option-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.fb-option-check {
  flex: none;
  margin-left: auto;
}
.fb-empty {
  padding: 6px 8px;
  color: var(--text-faint);
  font-size: 11px;
}
.fb-input-wrap {
  position: relative;
  flex: 1;
  display: flex;
  min-width: 0;
}
.fb-input {
  flex: 1;
  min-width: 0;
  padding-right: 28px;
}
.fb-input.dirty {
  padding-left: 31px;
}
.fb-input.active {
  border-color: var(--accent);
  background: var(--bg-selected);
}
.fb-enter-hint {
  position: absolute;
  left: 9px;
  top: 50%;
  z-index: 1;
  color: var(--accent);
  pointer-events: none;
  animation: enter-pulse 1.1s ease-in-out infinite;
}
.fb-clear {
  position: absolute;
  right: 4px;
  top: 50%;
  transform: translateY(-50%);
}
@keyframes enter-pulse {
  0%,
  100% {
    opacity: 0.6;
    transform: translateY(-50%) scale(0.82);
  }
  50% {
    opacity: 1;
    transform: translateY(-50%) scale(1.24);
  }
}
@media (prefers-reduced-motion: reduce) {
  .fb-enter-hint {
    animation: none;
    transform: translateY(-50%);
  }
}
</style>
