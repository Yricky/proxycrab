<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useBackend } from "../api";
import { logsStore } from "../stores/logs";
import { sessionsStore } from "../stores/sessions";
import { reportError } from "../stores/app";
import { openLogDetail } from "../windows/launcher";
import { Io5ArrowDown, Io5ArrowUp } from "vue-icons-plus/io5";

const backend = useBackend();

const ROW_HEIGHT = 26;
const BUFFER = 8;
const ID_COLUMN_WIDTH = 64;
const MIN_COLUMN_WIDTH = 48;

const scroller = ref<HTMLElement | null>(null);
const scrollTop = ref(0);
const viewportHeight = ref(0);

/** Local width overrides (view column index → px) for live resize. */
const widthOverrides = ref<Record<number, number>>({});

const columns = computed(() => logsStore.columns);

function columnWidth(index: number): number {
  if (widthOverrides.value[index] !== undefined) return widthOverrides.value[index];
  const w = columns.value[index]?.width;
  return w && w > 0 ? w : 160;
}

const gridTemplate = computed(
  () =>
    [`${ID_COLUMN_WIDTH}px`, ...columns.value.map((_, i) => `${columnWidth(i)}px`)].join(
      " ",
    ),
);

const totalWidth = computed(() =>
  columns.value.reduce((sum, _, i) => sum + columnWidth(i), ID_COLUMN_WIDTH),
);

const rows = computed(() => logsStore.displayRows);

const startIndex = computed(() =>
  Math.max(0, Math.floor(scrollTop.value / ROW_HEIGHT) - BUFFER),
);
const endIndex = computed(() =>
  Math.min(
    rows.value.length,
    Math.ceil((scrollTop.value + viewportHeight.value) / ROW_HEIGHT) + BUFFER,
  ),
);
const visibleRows = computed(() =>
  rows.value.slice(startIndex.value, endIndex.value).map((row, i) => ({
    row,
    index: startIndex.value + i,
  })),
);

function onScroll(): void {
  const el = scroller.value;
  if (!el) return;
  scrollTop.value = el.scrollTop;
  viewportHeight.value = el.clientHeight;
  const threshold = el.clientHeight * 2;
  const atOldRecordEdge = logsStore.sortDesc
    ? el.scrollHeight - el.scrollTop - el.clientHeight < threshold
    : el.scrollTop < threshold;
  if (atOldRecordEdge) void logsStore.loadOlder();
}

let resizeObserver: ResizeObserver | null = null;
watch(scroller, (el, prev) => {
  resizeObserver?.disconnect();
  if (el) {
    resizeObserver = new ResizeObserver(() => {
      viewportHeight.value = el.clientHeight;
    });
    resizeObserver.observe(el);
    viewportHeight.value = el.clientHeight;
  }
  void prev;
});

function toggleSort(): void {
  logsStore.sortDesc = !logsStore.sortDesc;
}

function openRow(id: number): void {
  const sessionId = sessionsStore.viewingSessionId;
  if (sessionId !== null) openLogDetail(sessionId, id);
}

// ---------- column resize ----------

function startResize(index: number, event: PointerEvent): void {
  event.preventDefault();
  event.stopPropagation();
  const startX = event.clientX;
  const startWidth = columnWidth(index);
  let width = startWidth;
  const onMove = (e: PointerEvent) => {
    width = Math.max(startWidth + e.clientX - startX, MIN_COLUMN_WIDTH);
    widthOverrides.value = { ...widthOverrides.value, [index]: width };
  };
  const onUp = () => {
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUp);
    void persistWidth(index, width);
  };
  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", onUp);
}

async function persistWidth(payloadIndex: number, width: number): Promise<void> {
  const sessionId = sessionsStore.viewingSessionId;
  if (sessionId === null) return;
  const column = columns.value[payloadIndex];
  if (!column) return;
  try {
    const view = await backend.getSessionView(sessionId);
    const current = view.columns[payloadIndex];
    if (!current) return;
    const next = [...view.columns];
    next[payloadIndex] = { ...current, width };
    await backend.replaceSessionView(sessionId, { columns: next });
  } catch (error) {
    reportError(error, "保存列宽失败");
  }
}

function cellClass(index: number, value: string): string {
  const kind = columns.value[index]?.kind;
  if (kind === "code") {
    if (value.startsWith("2")) return "code-2xx";
    if (value.startsWith("3")) return "code-3xx";
    if (value.startsWith("4")) return "code-4xx";
    if (value.startsWith("5")) return "code-5xx";
  }
  return "";
}
</script>

<template>
  <div ref="scroller" class="lt-scroll" @scroll.passive="onScroll">
    <div class="lt-content" :style="{ width: totalWidth + 'px' }">
      <div class="lt-header" :style="{ gridTemplateColumns: gridTemplate }">
        <div class="lt-cell lt-header-cell sortable" @click="toggleSort()">
          <span class="lt-header-name">
            id
            <Io5ArrowDown v-if="logsStore.sortDesc" :size="11" />
            <Io5ArrowUp v-else :size="11" />
          </span>
        </div>
        <div
          v-for="(column, i) in columns"
          :key="column.key + i"
          class="lt-cell lt-header-cell"
        >
          <span class="lt-header-name">
            {{ column.name }}
          </span>
          <span class="lt-resize" @pointerdown="startResize(i, $event)" @click.stop />
        </div>
      </div>
      <div class="lt-body" :style="{ height: rows.length * ROW_HEIGHT + 'px' }">
        <div
          class="lt-window"
          :style="{ transform: `translateY(${startIndex * ROW_HEIGHT}px)` }"
        >
          <div
            v-for="entry in visibleRows"
            :key="entry.row.id"
            class="lt-row"
            :class="{ stripe: entry.index % 2 === 1 }"
            :style="{ gridTemplateColumns: gridTemplate, height: ROW_HEIGHT + 'px' }"
            @click="openRow(entry.row.id)"
          >
            <div class="lt-cell mono" :title="String(entry.row.id)">
              {{ entry.row.id }}
            </div>
            <div
              v-for="(cell, i) in entry.row.cells"
              :key="i"
              class="lt-cell mono"
              :class="[
                cellClass(i, cell),
                { 'cell-error': logsStore.cellError(entry.row.id, i) },
              ]"
              :title="logsStore.cellError(entry.row.id, i) ?? cell"
            >
              {{ cell }}
            </div>
          </div>
        </div>
      </div>
      <div v-if="rows.length === 0 && !logsStore.loading" class="lt-empty">
        <template v-if="sessionsStore.viewingSessionId === null">请选择或创建一个会话</template>
        <template v-else-if="logsStore.filterActive">没有匹配过滤条件的记录</template>
        <template v-else>暂无抓包记录</template>
      </div>
    </div>
  </div>
</template>

<style scoped>
.lt-scroll {
  flex: 1;
  min-height: 0;
  overflow: auto;
  position: relative;
}
.lt-content {
  min-width: 100%;
}
.lt-header {
  display: grid;
  position: sticky;
  top: 0;
  z-index: 5;
  background: var(--bg-panel);
  border-bottom: 1px solid var(--border-strong);
}
.lt-header-cell {
  font-weight: 600;
  color: var(--text-secondary);
  font-size: 12px;
  height: 28px;
  display: flex;
  align-items: center;
  position: relative;
  user-select: none;
}
.lt-header-cell.sortable {
  cursor: pointer;
}
.lt-header-name {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.lt-resize {
  position: absolute;
  right: -3px;
  top: 0;
  bottom: 0;
  width: 7px;
  cursor: col-resize;
  z-index: 2;
}
.lt-resize:hover {
  background: var(--accent);
  opacity: 0.5;
}
.lt-body {
  position: relative;
}
.lt-window {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
}
.lt-row {
  display: grid;
  cursor: pointer;
  border-bottom: 1px solid var(--border);
}
.lt-row.stripe {
  background: var(--stripe);
}
.lt-row:hover {
  background: var(--bg-selected);
}
.lt-cell.cell-error {
  background: color-mix(in srgb, var(--danger) 18%, transparent);
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--danger) 60%, transparent);
}
.lt-cell {
  padding: 0 8px;
  display: flex;
  align-items: center;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  border-right: 1px solid var(--border);
  min-width: 0;
}
.lt-cell:last-child {
  border-right: none;
}
.lt-empty {
  position: sticky;
  left: 0;
  width: 100vw;
  max-width: 100%;
  text-align: center;
  color: var(--text-faint);
  padding: 40px 0;
}
.code-2xx {
  color: var(--success);
}
.code-3xx {
  color: var(--accent);
}
.code-4xx {
  color: var(--warning);
}
.code-5xx {
  color: var(--danger);
}
</style>
