<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import {
  Io5AlertCircle,
  Io5Checkmark,
  Io5ChevronDown,
  Io5Close,
  Io5CodeSlash,
  Io5Layers,
  Io5ReturnDownBack,
  Io5Text,
} from "vue-icons-plus/io5";
import ColumnGroupPopup from "./ColumnGroupPopup.vue";
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
import {
  closeColumnGroups,
  columnGroupsState,
  getColumnGroupCache,
  invalidateScriptColumnGroups,
  openColumnGroups,
} from "../stores/column-groups";
import {
  filterAndSortGroups,
  type ColumnGroupEntry,
  type ColumnGroupSort,
} from "../utils/column-groups";

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
const groupOpen = ref(false);
const applying = ref(false);
const columnScripts = ref<Script[]>([]);
const filterScripts = ref<Script[]>([]);
const columnRegex = ref(false);
const draft = ref<SessionFilter>(emptySessionFilter());
const regexError = ref<string | null>(null);
const regexValidating = ref(false);
let validationSequence = 0;

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
    : draft.value.option?.regex
      ? "输入正则表达式，回车应用"
      : "输入包含文本，回车应用",
);
const activeGroupCache = computed(() => {
  const sessionId = sessionsStore.viewingSessionId;
  const column = selectedColumn.value;
  if (!groupOpen.value || sessionId === null || !column) return null;
  return getColumnGroupCache(sessionId, column);
});
const groupEntries = computed(() => {
  const cache = activeGroupCache.value;
  if (!cache) return [];
  return filterAndSortGroups(
    cache.groups,
    cache.emptyCount,
    cache.errorCount,
    cache.search,
    cache.sort,
    cache.descending,
  );
});

watch(
  () => logsStore.appliedFilter,
  (filter) => {
    draft.value = cloneSessionFilter(filter);
    if (filter.option?.kind === "column") {
      columnRegex.value = filter.option.regex;
    }
  },
  { deep: true, immediate: true },
);

watch(
  () => ({
    enabled:
      draft.value.option?.kind === "column" && draft.value.option.regex,
    input: draft.value.input,
  }),
  async ({ enabled, input }) => {
    const sequence = ++validationSequence;
    regexError.value = null;
    regexValidating.value = false;
    if (!enabled) return;
    regexValidating.value = true;
    let error: string | null;
    try {
      error = await backend.validateFilterRegex(input);
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    }
    if (sequence !== validationSequence) return;
    regexError.value = error;
    regexValidating.value = false;
  },
  { immediate: true },
);

watch(
  () => sessionsStore.viewingSessionId,
  () => closeGroupPopup(),
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
  const reopenGroups =
    groupOpen.value && selectedColumn.value?.kind === "script";
  if (reopenGroups) closeGroupPopup();
  invalidateScriptColumnGroups();
  await refreshScripts();
  if (sessionsStore.viewingSessionId !== null) {
    await logsStore.loadSession();
  }
  if (reopenGroups && selectedColumn.value?.kind === "script") openGroupPopup();
}

async function apply(): Promise<void> {
  if (
    !draft.value.option ||
    !dirty.value ||
    applying.value ||
    regexValidating.value ||
    regexError.value
  ) {
    return;
  }
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
    regex: columnRegex.value,
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

function setRegex(event: Event): void {
  const checked = (event.target as HTMLInputElement).checked;
  columnRegex.value = checked;
  if (draft.value.option?.kind === "column") {
    draft.value = {
      option: {
        ...draft.value.option,
        regex: checked,
      },
      input: draft.value.input,
    };
  }
}

function toggleTargetMenu(): void {
  if (!menuOpen.value) closeGroupPopup();
  menuOpen.value = !menuOpen.value;
}

function openGroupPopup(): void {
  const sessionId = sessionsStore.viewingSessionId;
  const column = selectedColumn.value;
  if (sessionId === null || !column) return;
  menuOpen.value = false;
  groupOpen.value = true;
  openColumnGroups(backend, sessionId, column);
}

function closeGroupPopup(): void {
  if (!groupOpen.value && columnGroupsState.activeKey === null) return;
  groupOpen.value = false;
  closeColumnGroups();
}

function toggleGroupPopup(): void {
  if (groupOpen.value) closeGroupPopup();
  else openGroupPopup();
}

async function selectGroup(entry: ColumnGroupEntry): Promise<void> {
  const column = selectedColumn.value;
  if (!column || entry.exactPattern === null || applying.value) return;
  columnRegex.value = true;
  regexError.value = null;
  regexValidating.value = false;
  draft.value = {
    option: { kind: "column", column, regex: true },
    input: entry.exactPattern,
  };
  applying.value = true;
  try {
    await logsStore.applyFilter(cloneSessionFilter(draft.value));
  } finally {
    applying.value = false;
  }
}

function setGroupSearch(value: string): void {
  if (activeGroupCache.value) activeGroupCache.value.search = value;
}

function setGroupSort(value: ColumnGroupSort): void {
  if (activeGroupCache.value) activeGroupCache.value.sort = value;
}

function toggleGroupDirection(): void {
  if (activeGroupCache.value) {
    activeGroupCache.value.descending = !activeGroupCache.value.descending;
  }
}

function clearInput(): void {
  draft.value = { ...draft.value, input: "" };
  inputEl.value?.focus();
}

function onDocumentPointerDown(event: PointerEvent): void {
  if (!root.value?.contains(event.target as Node)) {
    menuOpen.value = false;
    closeGroupPopup();
  }
}

function onHttpApiChange(event: Event): void {
  const { resources } = (event as CustomEvent<HttpApiChange>).detail;
  if (resources.includes("all") || resources.includes("column_scripts")) {
    void refreshAfterScriptChange();
  } else if (resources.includes("filter_scripts")) void refreshScripts();
}

onMounted(() => {
  void refreshScripts();
  document.addEventListener("pointerdown", onDocumentPointerDown);
  window.addEventListener("column-scripts-changed", refreshAfterScriptChange);
  window.addEventListener("filter-scripts-changed", refreshAfterScriptChange);
  window.addEventListener(HTTP_API_CHANGE_EVENT, onHttpApiChange);
});

onBeforeUnmount(() => {
  closeGroupPopup();
  document.removeEventListener("pointerdown", onDocumentPointerDown);
  window.removeEventListener("column-scripts-changed", refreshAfterScriptChange);
  window.removeEventListener("filter-scripts-changed", refreshAfterScriptChange);
  window.removeEventListener(HTTP_API_CHANGE_EVENT, onHttpApiChange);
});
</script>

<template>
  <div
    ref="root"
    class="filter-bar"
    :class="{
      empty: !draft.option,
      applied: logsStore.filterActive && !dirty,
      dirty: dirty && draft.option,
    }"
  >
    <div class="fb-target">
      <button
        class="btn fb-target-trigger"
        :class="{ active: menuOpen }"
        :aria-expanded="menuOpen"
        @click="toggleTargetMenu"
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
            <label class="fb-regex" @click.stop>
              <input
                type="checkbox"
                :checked="columnRegex"
                @change="setRegex"
              />
              <span>正则表达式</span>
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

    <div v-if="draft.option?.kind === 'column'" class="fb-group">
      <button
        class="btn icon fb-group-trigger"
        :class="{ active: groupOpen }"
        :aria-expanded="groupOpen"
        :disabled="sessionsStore.viewingSessionId === null"
        title="按当前列分组"
        @click="toggleGroupPopup"
      >
        <Io5Layers :size="14" />
      </button>
      <ColumnGroupPopup
        v-if="groupOpen && activeGroupCache"
        :cache="activeGroupCache"
        :entries="groupEntries"
        :current-input="draft.input"
        @select="selectGroup"
        @update:search="setGroupSearch"
        @update:sort="setGroupSort"
        @toggle-direction="toggleGroupDirection"
      />
    </div>

    <div class="fb-input-wrap">
      <Io5AlertCircle
        v-if="regexError"
        :size="15"
        class="fb-regex-error"
        :title="regexError"
      />
      <Io5ReturnDownBack
        v-else-if="dirty && draft.option && !applying"
        :size="15"
        class="fb-enter-hint"
        title="按回车应用"
      />
      <div
        v-else-if="applying && dirty && draft.option"
        class="fb-loading"
        aria-label="正在应用过滤"
      >
        <span class="fb-spinner" />
      </div>
      <input
        ref="inputEl"
        v-model="draft.input"
        class="input fb-input mono"
        :class="{
          active: logsStore.filterActive && !dirty,
          dirty: dirty && draft.option,
          invalid: regexError,
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
  min-height: 32px;
  margin: 0;
  padding: 0;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--bg-panel);
  flex: none;
  transition:
    background 0.12s,
    border-color 0.12s;
}
.filter-bar.empty {
  border-style: dashed;
  background: transparent;
}
.filter-bar:hover {
  border-color: var(--border-strong);
}
.filter-bar:focus-within,
.filter-bar.dirty {
  border-color: var(--accent);
}
.filter-bar.applied {
  border-color: var(--accent);
}
.fb-target {
  position: relative;
  flex: none;
  align-self: stretch;
  display: flex;
  border-right: 1px solid var(--border);
}
.fb-target-trigger {
  width: 178px;
  height: 100%;
  justify-content: space-between;
  border: 0;
  border-radius: var(--radius-md) 0 0 var(--radius-md);
  background: transparent;
}
.fb-target-trigger:hover:not(:disabled),
.fb-target-trigger.active {
  border-color: transparent;
  background: var(--bg-hover);
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
.fb-regex {
  display: flex;
  align-items: center;
  gap: 5px;
  color: var(--text-secondary);
  font-weight: 400;
  cursor: pointer;
}
.fb-regex input {
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
.fb-group {
  position: relative;
  flex: none;
  align-self: stretch;
  display: flex;
  border-right: 1px solid var(--border);
}
button.fb-group-trigger {
  width: 34px;
  height: 100%;
  justify-content: center;
  padding: 0;
  border: 0;
  border-radius: 0;
  background: transparent;
  color: var(--text-secondary);
}
button.fb-group-trigger:hover:not(:disabled),
button.fb-group-trigger.active {
  border-color: transparent;
  border-radius: 0;
  background: var(--bg-hover);
  color: var(--accent);
}
.fb-input-wrap {
  position: relative;
  flex: 1;
  display: flex;
  align-self: stretch;
  min-width: 0;
}
.fb-input {
  flex: 1;
  min-width: 0;
  border: 0;
  border-radius: 0 var(--radius-md) var(--radius-md) 0;
  background: transparent;
  padding-right: 28px;
}
.fb-input:focus {
  border-color: transparent;
}
.fb-input.dirty {
  padding-left: 31px;
}
.fb-input.invalid {
  padding-left: 31px;
}
.fb-input.active {
  background: transparent;
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
.fb-regex-error {
  position: absolute;
  left: 9px;
  top: 50%;
  z-index: 1;
  transform: translateY(-50%);
  color: var(--danger);
}
.fb-loading {
  position: absolute;
  left: 9px;
  top: 50%;
  z-index: 1;
  transform: translateY(-50%);
  width: 15px;
  height: 15px;
  display: flex;
  align-items: center;
  justify-content: center;
  pointer-events: none;
}
.fb-spinner {
  width: 11px;
  height: 11px;
  border: 1.5px solid color-mix(in srgb, var(--accent) 35%, transparent);
  border-top-color: var(--accent);
  border-radius: 50%;
  animation: fb-spin 0.7s linear infinite;
}
@keyframes fb-spin {
  to {
    transform: rotate(360deg);
  }
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
  .fb-spinner {
    animation: none;
    border-top-color: var(--accent);
  }
}
</style>
