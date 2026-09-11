<script setup lang="ts">
import { nextTick, onMounted, ref, watch } from "vue";
import { Io5ReorderTwo } from "vue-icons-plus/io5";
import { normalizeKvRows, type KvRow } from "../utils/replay";

const rows = defineModel<KvRow[]>({ required: true });

const nonEmpty = (list: KvRow[]) =>
  list.filter((row) => row.name !== "" || row.value !== "");

// 展示态始终以恰好一个末尾空行结尾；与外部 v-model 同步时去掉空行。
const display = ref<KvRow[]>(normalizeKvRows(rows.value));
watch(rows, (value) => {
  if (
    JSON.stringify(nonEmpty(value)) !== JSON.stringify(nonEmpty(display.value))
  ) {
    display.value = normalizeKvRows(value);
  }
});

function commit(): void {
  display.value = normalizeKvRows(display.value);
  rows.value = nonEmpty(display.value);
}

// ---------- 值列换行 textarea 自动增高 ----------
const tableRef = ref<HTMLElement | null>(null);

function autosize(el: HTMLTextAreaElement): void {
  el.style.height = "0";
  el.style.height = `${el.scrollHeight}px`;
}

function autosizeAll(): void {
  void nextTick(() => {
    tableRef.value
      ?.querySelectorAll("textarea.kv-value-input")
      .forEach((el) => autosize(el as HTMLTextAreaElement));
  });
}

onMounted(autosizeAll);
watch(display, autosizeAll, { deep: true });

function onValueInput(row: KvRow, event: Event): void {
  // header value 不允许换行；粘贴带入的换行直接去除
  if (row.value.includes("\n")) row.value = row.value.replace(/\r?\n/g, "");
  autosize(event.target as HTMLTextAreaElement);
  commit();
}

// ---------- 拖拽排序（pointer 事件实现，不依赖浏览器 DnD） ----------
const dragIndex = ref<number | null>(null);

function draggable(index: number): boolean {
  const row = display.value[index];
  return row.name !== "" || row.value !== "";
}

function onHandlePointerDown(index: number, event: PointerEvent): void {
  event.preventDefault();
  dragIndex.value = index;
  const onMove = (move: PointerEvent) => {
    const target = document
      .elementFromPoint(move.clientX, move.clientY)
      ?.closest("tr[data-kv-index]");
    if (!(target instanceof HTMLElement) || !tableRef.value?.contains(target))
      return;
    const to = Number(target.dataset.kvIndex);
    const from = dragIndex.value;
    if (from === null || Number.isNaN(to) || to === from) return;
    const next = [...display.value];
    const [moved] = next.splice(from, 1);
    next.splice(to, 0, moved);
    display.value = next;
    dragIndex.value = to;
  };
  const onUp = () => {
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUp);
    dragIndex.value = null;
    commit();
  };
  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", onUp);
}
</script>

<template>
  <table ref="tableRef" class="kv-edit-table">
    <tbody>
      <tr
        v-for="(row, index) in display"
        :key="index"
        :data-kv-index="index"
        :class="{ dragging: dragIndex === index }"
      >
        <td class="kv-drag-col">
          <span
            v-if="draggable(index)"
            class="drag-handle text-faint"
            title="拖动排序"
            @pointerdown="onHandlePointerDown(index, $event)"
          >
            <Io5ReorderTwo :size="13" />
          </span>
        </td>
        <td class="mono kv-name">
          <input
            v-model="row.name"
            class="kv-edit kv-name-input"
            placeholder="header"
            :size="Math.max(1, row.name.length)"
            spellcheck="false"
            @input="commit"
          />
        </td>
        <td class="mono kv-value">
          <textarea
            v-model="row.value"
            class="kv-edit kv-value-input"
            placeholder="value"
            rows="1"
            spellcheck="false"
            @input="onValueInput(row, $event)"
            @keydown.enter.prevent
          />
        </td>
      </tr>
    </tbody>
  </table>
</template>

<style scoped>
/* 与详情页 kv-table 完全一致，仅内容可编辑 */
.kv-edit-table {
  width: 100%;
  border-collapse: collapse;
}

.kv-edit-table tr + tr {
  border-top: 1px solid var(--border);
}

.kv-edit-table tr:hover {
  background: var(--bg-hover);
}

.kv-edit-table td {
  padding: 4px 10px;
  vertical-align: top;
  font-size: 11px;
}

.kv-drag-col {
  width: 18px;
  text-align: center;
  padding-left: 2px;
  padding-right: 2px;
}

.drag-handle {
  display: inline-flex;
  cursor: grab;
  touch-action: none;
  user-select: none;
}

.kv-name {
  color: var(--text);
  white-space: nowrap;
  width: 1%;
  padding-right: 16px;
}

.kv-value {
  word-break: break-all;
}

.kv-edit {
  display: block;
  border: none;
  background: transparent;
  outline: none;
  font: inherit;
  color: inherit;
  padding: 0;
  resize: none;
  border-radius: 3px;
}

.kv-edit::placeholder {
  color: var(--text-faint);
}

.kv-edit:focus {
  background: var(--bg-active);
}

.kv-name-input {
  width: auto;
  max-width: 360px;
}

.kv-value-input {
  width: 100%;
  color: var(--success);
  overflow: hidden;
  white-space: pre-wrap;
  word-break: break-all;
}

tr.dragging {
  background: var(--bg-selected);
}
</style>
