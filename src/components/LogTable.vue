<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch, watchEffect } from "vue";
import { useBackend } from "../api";
import { logsStore } from "../stores/logs";
import { sessionsStore } from "../stores/sessions";
import { proxyStore } from "../stores/proxy";
import { appStore, reportError } from "../stores/app";
import { openContextMenu, openMenuAt, confirmDialog, type MenuItem } from "../stores/dialog";
import { openLogDetail } from "../windows/launcher";
import LogDetailWindow from "../windows/LogDetailWindow.vue";
import type { Column, LogViewRow, Script } from "../api/types";
import { isInactiveInProgress } from "../utils/capture-outcome";
import { copyText as writeClipboardText } from "../utils/clipboard";
import { formatDateTimeWithZone } from "../utils/format";
import { buildSessionShareLinks } from "../utils/session-share";
import {
  anchoredScrollTop,
  clamp,
  fullyVisibleRowRange,
  reconcileUnseenIds,
} from "../utils/log-canvas";
import {
  Io5Checkmark,
  Io5Close,
  Io5Copy,
  Io5Link,
  Io5OpenOutline,
  Io5Trash,
} from "vue-icons-plus/io5";

const backend = useBackend();
const readonly = backend.capabilities.readonly;

const ROW_HEIGHT = 26;
const HEADER_HEIGHT = 28;
const ID_COLUMN_WIDTH = 58;
const ADD_COLUMN_WIDTH = 84;
const MIN_COLUMN_WIDTH = 48;
const SCROLLBAR_SIZE = 8;
const MIN_THUMB_SIZE = 28;
const RESIZE_HIT_WIDTH = 7;

const wrap = ref<HTMLElement | null>(null);
const tableWrap = ref<HTMLElement | null>(null);
const canvas = ref<HTMLCanvasElement | null>(null);
const viewportWidth = ref(0);
const viewportHeight = ref(0);
const scrollTop = ref(0);
const scrollLeft = ref(0);
const hoveredRow = ref<number | null>(null);
const hoverHit = ref<HitTarget | null>(null);
const unseenIds = ref<Set<number>>(new Set());
const following = ref(true);

/** Local width overrides (view column index → px) for live resize. */
const widthOverrides = ref<Record<number, number>>({});

/** 展示用列：key/name 由 Column 派生（key=`script:${name}` 或 kind，name=kind 或 script_name）。 */
type TableColumn = Column & { key: string; name: string };

const columns = computed<TableColumn[]>(() =>
  logsStore.columns.map((column) => {
    const key =
      column.kind === "script" ? `script:${column.script_name}` : column.kind;
    const name =
      column.kind === "script" ? column.script_name : column.kind;
    return { ...column, key, name };
  }),
);

function columnWidth(index: number): number {
  if (widthOverrides.value[index] !== undefined) return widthOverrides.value[index];
  const w = columns.value[index]?.width;
  return w && w > 0 ? w : 160;
}

const totalWidth = computed(() =>
  columns.value.reduce(
    (sum, _, i) => sum + columnWidth(i),
    ID_COLUMN_WIDTH + (readonly ? 0 : ADD_COLUMN_WIDTH),
  ),
);

const rowIds = computed(() => logsStore.sortedIds);

interface CanvasLayout {
  bodyWidth: number;
  bodyHeight: number;
  horizontal: boolean;
  vertical: boolean;
  maxScrollLeft: number;
  maxScrollTop: number;
}

const layout = computed<CanvasLayout>(() => {
  const width = viewportWidth.value;
  const height = viewportHeight.value;
  const contentHeight = rowIds.value.length * ROW_HEIGHT;
  let horizontal = false;
  let vertical = false;
  for (let i = 0; i < 2; i += 1) {
    const bodyWidth = Math.max(0, width - (vertical ? SCROLLBAR_SIZE : 0));
    const bodyHeight = Math.max(
      0,
      height - HEADER_HEIGHT - (horizontal ? SCROLLBAR_SIZE : 0),
    );
    horizontal = totalWidth.value > bodyWidth;
    vertical = contentHeight > bodyHeight;
  }
  const bodyWidth = Math.max(0, width - (vertical ? SCROLLBAR_SIZE : 0));
  const bodyHeight = Math.max(
    0,
    height - HEADER_HEIGHT - (horizontal ? SCROLLBAR_SIZE : 0),
  );
  return {
    bodyWidth,
    bodyHeight,
    horizontal,
    vertical,
    maxScrollLeft: Math.max(0, totalWidth.value - bodyWidth),
    maxScrollTop: Math.max(0, contentHeight - bodyHeight),
  };
});

const visibleIds = computed(() => {
  const start = Math.max(0, Math.floor(scrollTop.value / ROW_HEIGHT));
  const end = Math.min(
    rowIds.value.length,
    Math.ceil((scrollTop.value + layout.value.bodyHeight) / ROW_HEIGHT),
  );
  return rowIds.value.slice(start, end);
});

watch(
  visibleIds,
  (ids) => logsStore.setViewportIds(ids),
  { immediate: true },
);

function toggleSort(): void {
  logsStore.sortDesc = !logsStore.sortDesc;
}

function openRow(id: number): void {
  const sessionId = sessionsStore.viewingSessionId;
  if (sessionId === null) return;
  // 再次点击已选中行则关闭底部面板。
  if (panelLogId.value === id) {
    closePanel();
    return;
  }
  openPanel(sessionId, id);
}

// ---------- bottom detail panel ----------

const MIN_PANEL_HEIGHT = 160;
const MIN_TABLE_HEIGHT = 120;

/** 面板按 log id 跟踪，新日志插入导致行位移不影响展示内容。 */
const panelSessionId = ref<number | null>(null);
const panelLogId = ref<number | null>(null);
/** 0 表示尚未初始化，首次打开时按容器高度 40% 计算。 */
const panelHeight = ref(0);
const panelDragging = ref(false);

function closePanel(): void {
  panelSessionId.value = null;
  panelLogId.value = null;
}

function openPanel(sessionId: number, id: number): void {
  panelSessionId.value = sessionId;
  panelLogId.value = id;
  if (panelHeight.value <= 0) {
    panelHeight.value = Math.round((wrap.value?.clientHeight ?? 0) * 0.4);
  }
}

/** 分享链接定位：滚动到目标行（尽量居中）并打开底部详情面板。 */
function focusLog(id: number): boolean {
  const sessionId = sessionsStore.viewingSessionId;
  if (sessionId === null) return false;
  const index = rowIds.value.indexOf(id);
  if (index < 0) return false;
  const target = index * ROW_HEIGHT - (layout.value.bodyHeight - ROW_HEIGHT) / 2;
  setScrollOffsets(scrollLeft.value, target);
  openPanel(sessionId, id);
  return true;
}

defineExpose({ focusLog });

function openPanelInWindow(): void {
  if (panelSessionId.value === null || panelLogId.value === null) return;
  openLogDetail(panelSessionId.value, panelLogId.value);
  closePanel();
}

watch(() => sessionsStore.viewingSessionId, closePanel);

function maxPanelHeight(): number {
  const container = wrap.value?.clientHeight ?? 0;
  return Math.max(MIN_PANEL_HEIGHT, container - MIN_TABLE_HEIGHT);
}

function startPanelDrag(event: PointerEvent): void {
  event.preventDefault();
  panelDragging.value = true;
  const startY = event.clientY;
  const startHeight = panelHeight.value;
  const onMove = (e: PointerEvent) => {
    panelHeight.value = clamp(
      startHeight + (startY - e.clientY),
      MIN_PANEL_HEIGHT,
      maxPanelHeight(),
    );
  };
  const onUp = () => {
    panelDragging.value = false;
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUp);
  };
  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", onUp);
}

async function copyCell(value: string): Promise<void> {
  try {
    await writeClipboardText(value);
    appStore.toast("单元格内容已复制", "success");
  } catch (error) {
    reportError(error, "复制单元格失败");
  }
}

function showCellMenu(event: MouseEvent, value: string | null, rowId: number): void {
  const items: MenuItem[] = [];
  if (value !== null) {
    items.push({
      label: "复制",
      icon: Io5Copy,
      action: () => void copyCell(value),
    });
  }
  items.push({
    label: "复制聚焦到此行的分享链接",
    icon: Io5Link,
    action: () => void copyFocusedShareLink(rowId),
  });
  openContextMenu(event, items);
}

/** 复制带 `id` 定位参数的分享链接；未分享时先弹窗确认开启分享。 */
async function copyFocusedShareLink(id: number): Promise<void> {
  // 分享页本身即为只读分享，直接基于当前 URL 设置聚焦 id。
  if (readonly) {
    const url = new URL(window.location.href);
    url.searchParams.set("id", String(id));
    try {
      await writeClipboardText(url.toString());
      appStore.toast("分享链接已复制", "success");
    } catch (error) {
      reportError(error, "复制分享链接失败");
    }
    return;
  }
  const sessionId = sessionsStore.viewingSessionId;
  if (sessionId === null) return;
  try {
    const [current, status, addresses] = await Promise.all([
      backend.getSessionShare(sessionId),
      backend.getHttpServiceStatus(),
      backend.listLocalIps(),
    ]);
    if (!status.running) {
      appStore.toast("HTTP 服务未运行，无法生成分享链接", "error");
      return;
    }
    let share = current;
    if (!share.enabled || !share.token) {
      const confirmed = await confirmDialog({
        title: "开启 Session 分享",
        message: "开启后，任何获得链接的人都可以只读查看此会话的抓包内容。",
        confirmText: "开启并复制",
      });
      if (!confirmed) return;
      share = await backend.enableSessionShare(sessionId);
    }
    const link = share.token
      ? buildSessionShareLinks(addresses, status.port, share.token)[0]
      : undefined;
    if (!link) {
      appStore.toast("暂无可用的本机地址，无法生成分享链接", "error");
      return;
    }
    const url = new URL(link);
    url.searchParams.set("id", String(id));
    await writeClipboardText(url.toString());
    appStore.toast("分享链接已复制", "success");
  } catch (error) {
    reportError(error, "复制分享链接失败");
  }
}

// ---------- column menu ----------

interface ColumnChoice {
  key: string;
  label?: string;
  defaultWidth: number;
  create: (width: number) => Column;
}

const BUILTIN_COLUMN_CHOICES: ColumnChoice[] = [
  {
    key: "method",
    defaultWidth: 80,
    create: (width) => ({ kind: "method", width }),
  },
  {
    key: "uri",
    defaultWidth: 300,
    create: (width) => ({ kind: "uri", width }),
  },
  {
    key: "code",
    defaultWidth: 50,
    create: (width) => ({ kind: "code", width }),
  },
  {
    key: "source",
    defaultWidth: 130,
    create: (width) => ({ kind: "source", width }),
  },
  {
    key: "stage",
    defaultWidth: 70,
    create: (width) => ({ kind: "stage", width }),
  },
  {
    key: "created_at",
    defaultWidth: 200,
    create: (width) => ({ kind: "created_at", width }),
  },
  {
    key: "updated_at",
    defaultWidth: 200,
    create: (width) => ({ kind: "updated_at", width }),
  },
];

async function columnChoices(): Promise<ColumnChoice[]> {
  let scripts: Script[] = [];
  try {
    scripts = await backend.listColumnScripts();
  } catch (error) {
    reportError(error, "加载列脚本失败");
  }
  return [
    ...BUILTIN_COLUMN_CHOICES,
    ...scripts.map((script) => ({
      key: `script:${script.name}`,
      label: `脚本：${script.name}`,
      defaultWidth: 100,
      create: (width: number): Column => ({
        kind: "script",
        script_name: script.name,
        width,
      }),
    })),
  ];
}

async function updateColumns(
  update: (columns: Column[]) => Column[],
  errorMessage: string,
): Promise<void> {
  const sessionId = sessionsStore.viewingSessionId;
  if (sessionId === null) return;
  try {
    const view = await backend.getSessionView(sessionId);
    await backend.replaceSessionView(sessionId, { columns: update(view.columns) });
    widthOverrides.value = {};
    await logsStore.refreshView();
  } catch (error) {
    reportError(error, errorMessage);
  }
}

function currentChoiceKey(index: number): string {
  const column = columns.value[index];
  return column?.kind === "script" ? `script:${column.script_name}` : (column?.kind ?? "");
}

async function showColumnMenu(index: number, x: number, y: number): Promise<void> {
  const choices = await columnChoices();
  const currentKey = currentChoiceKey(index);
  const items: MenuItem[] = [
    {
      label: "移除列",
      icon: Io5Trash,
      danger: true,
      action: () => {
        void updateColumns(
          (viewColumns) => viewColumns.filter((_, itemIndex) => itemIndex !== index),
          "移除列失败",
        );
      },
    },
    ...choices.map((choice, choiceIndex) => ({
      label: choice.label ?? choice.key,
      icon: choice.key === currentKey ? Io5Checkmark : undefined,
      disabled: choice.key === currentKey,
      dividerBefore: choiceIndex === 0,
      action: () => {
        void updateColumns((viewColumns) => {
          const current = viewColumns[index];
          if (!current) return viewColumns;
          const next = [...viewColumns];
          next[index] = choice.create(current.width);
          return next;
        }, "更新列失败");
      },
    })),
  ];
  openMenuAt(x, y, items);
}

async function showAddColumnMenu(x: number, y: number): Promise<void> {
  const choices = await columnChoices();
  openMenuAt(
    x,
    y,
    choices.map((choice) => ({
      label: choice.label ?? choice.key,
      action: () => {
        void updateColumns(
          (viewColumns) => [...viewColumns, choice.create(choice.defaultWidth)],
          "添加列失败",
        );
      },
    })),
  );
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
    window.setTimeout(() => {
      ignoreNextClick = false;
    }, 0);
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

function displayCellValue(row: LogViewRow, index: number): string {
  const rawValue = row.cells[index] ?? "";
  if (rawValue === "…") return rawValue;
  const kind = columns.value[index]?.kind;
  if (kind === "created_at") return formatDateTimeWithZone(row.created_at);
  if (kind === "updated_at") return formatDateTimeWithZone(row.updated_at);
  return rawValue;
}

function outcomeDotClass(row: LogViewRow): string {
  const active = proxyStore.isNetlogActive(sessionsStore.viewingSessionId, row.id);
  if (isInactiveInProgress(row.outcome, active)) return "stale";
  if (active) return "active";
  if (row.outcome === "failed") return "failed";
  return "";
}

type HitTarget =
  | { type: "header-id" }
  | { type: "header-column"; index: number }
  | { type: "header-add" }
  | { type: "resize"; index: number }
  | { type: "row"; rowIndex: number; cellIndex: number | null }
  | { type: "pill" }
  | { type: "vertical-thumb" }
  | { type: "vertical-track" }
  | { type: "horizontal-thumb" }
  | { type: "horizontal-track" };

interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

interface DragState {
  axis: "x" | "y";
  pointerStart: number;
  scrollStart: number;
  scrollRange: number;
  thumbRange: number;
  pointerId: number;
}

interface Palette {
  app: string;
  panel: string;
  elevated: string;
  hover: string;
  selected: string;
  border: string;
  borderStrong: string;
  text: string;
  textSecondary: string;
  textFaint: string;
  accent: string;
  accentText: string;
  success: string;
  warning: string;
  danger: string;
  stripe: string;
  uiFont: string;
  monoFont: string;
}

let resizeObserver: ResizeObserver | null = null;
let themeObserver: MutationObserver | null = null;
let drawFrame: number | undefined;
let pillRect: Rect | null = null;
let dragState: DragState | null = null;
let ignoreNextClick = false;
let capturedPointerId: number | null = null;
let previousIds = new Set<number>();
let previousSortDesc = logsStore.sortDesc;

function cssValue(style: CSSStyleDeclaration, name: string): string {
  return style.getPropertyValue(name).trim();
}

function palette(): Palette {
  const style = getComputedStyle(document.documentElement);
  return {
    app: cssValue(style, "--bg-app"),
    panel: cssValue(style, "--bg-panel"),
    elevated: cssValue(style, "--bg-elevated"),
    hover: cssValue(style, "--bg-hover"),
    selected: cssValue(style, "--bg-selected"),
    border: cssValue(style, "--border"),
    borderStrong: cssValue(style, "--border-strong"),
    text: cssValue(style, "--text"),
    textSecondary: cssValue(style, "--text-secondary"),
    textFaint: cssValue(style, "--text-faint"),
    accent: cssValue(style, "--accent"),
    accentText: cssValue(style, "--accent-text"),
    success: cssValue(style, "--success"),
    warning: cssValue(style, "--warning"),
    danger: cssValue(style, "--danger"),
    stripe: cssValue(style, "--stripe"),
    uiFont: cssValue(style, "--font-ui"),
    monoFont: cssValue(style, "--font-mono"),
  };
}

function roundedRect(
  ctx: CanvasRenderingContext2D,
  rect: Rect,
  radius: number,
): void {
  const r = Math.min(radius, rect.width / 2, rect.height / 2);
  ctx.beginPath();
  ctx.moveTo(rect.x + r, rect.y);
  ctx.arcTo(rect.x + rect.width, rect.y, rect.x + rect.width, rect.y + rect.height, r);
  ctx.arcTo(
    rect.x + rect.width,
    rect.y + rect.height,
    rect.x,
    rect.y + rect.height,
    r,
  );
  ctx.arcTo(rect.x, rect.y + rect.height, rect.x, rect.y, r);
  ctx.arcTo(rect.x, rect.y, rect.x + rect.width, rect.y, r);
  ctx.closePath();
}

function pointInRect(x: number, y: number, rect: Rect | null): boolean {
  return !!rect && x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height;
}

function verticalThumb(targetLayout = layout.value): Rect | null {
  if (!targetLayout.vertical || targetLayout.bodyHeight <= 0) return null;
  const contentHeight = rowIds.value.length * ROW_HEIGHT;
  const height = Math.min(
    targetLayout.bodyHeight,
    Math.max(
      MIN_THUMB_SIZE,
      (targetLayout.bodyHeight * targetLayout.bodyHeight) / contentHeight,
    ),
  );
  const travel = Math.max(0, targetLayout.bodyHeight - height);
  return {
    x: targetLayout.bodyWidth,
    y:
      HEADER_HEIGHT +
      (targetLayout.maxScrollTop > 0
        ? (scrollTop.value / targetLayout.maxScrollTop) * travel
        : 0),
    width: SCROLLBAR_SIZE,
    height,
  };
}

function horizontalThumb(targetLayout = layout.value): Rect | null {
  if (!targetLayout.horizontal || targetLayout.bodyWidth <= 0) return null;
  const width = Math.min(
    targetLayout.bodyWidth,
    Math.max(
      MIN_THUMB_SIZE,
      (targetLayout.bodyWidth * targetLayout.bodyWidth) / totalWidth.value,
    ),
  );
  const travel = Math.max(0, targetLayout.bodyWidth - width);
  return {
    x:
      targetLayout.maxScrollLeft > 0
        ? (scrollLeft.value / targetLayout.maxScrollLeft) * travel
        : 0,
    y: HEADER_HEIGHT + targetLayout.bodyHeight,
    width,
    height: SCROLLBAR_SIZE,
  };
}

function setScrollOffsets(left: number, top: number, updateFollowing = true): void {
  const targetLayout = layout.value;
  scrollLeft.value = clamp(left, 0, targetLayout.maxScrollLeft);
  scrollTop.value = clamp(top, 0, targetLayout.maxScrollTop);
  if (updateFollowing) updateVisibilityState();
  scheduleDraw();
}

function fullyVisibleRange(ids = rowIds.value): { start: number; end: number } {
  return fullyVisibleRowRange(
    scrollTop.value,
    layout.value.bodyHeight,
    ROW_HEIGHT,
    ids.length,
  );
}

function maxIdFullyVisible(ids = rowIds.value): boolean {
  if (ids.length === 0) return true;
  const range = fullyVisibleRange(ids);
  const maxIndex = logsStore.sortDesc ? 0 : ids.length - 1;
  return maxIndex >= range.start && maxIndex < range.end;
}

function updateVisibilityState(): void {
  if (maxIdFullyVisible()) {
    following.value = true;
    if (unseenIds.value.size > 0) unseenIds.value = new Set();
    return;
  }
  following.value = false;
  if (unseenIds.value.size === 0) return;
  const range = fullyVisibleRange();
  const next = new Set(unseenIds.value);
  for (let index = range.start; index < range.end; index += 1) {
    next.delete(rowIds.value[index]);
  }
  if (next.size !== unseenIds.value.size) unseenIds.value = next;
}

function scrollToMax(): void {
  unseenIds.value = new Set();
  following.value = true;
  setScrollOffsets(
    scrollLeft.value,
    logsStore.sortDesc ? 0 : layout.value.maxScrollTop,
    false,
  );
}

function resetFollowState(): void {
  unseenIds.value = new Set();
  previousIds = new Set(rowIds.value);
  previousSortDesc = logsStore.sortDesc;
  following.value = maxIdFullyVisible();
  scheduleDraw();
}

watch(
  [
    () => sessionsStore.viewingSessionId,
    () => JSON.stringify(logsStore.appliedFilter),
  ],
  resetFollowState,
  { immediate: true, flush: "sync" },
);

watch(
  rowIds,
  (ids, oldIds) => {
    const current = new Set(ids);
    const sortChanged = previousSortDesc !== logsStore.sortDesc;
    previousSortDesc = logsStore.sortDesc;

    if (logsStore.loading || sortChanged) {
      previousIds = current;
      setScrollOffsets(scrollLeft.value, scrollTop.value);
      return;
    }

    const shouldFollow = following.value;
    const additions = ids.some((id) => !previousIds.has(id));
    const oldRange = fullyVisibleRange(oldIds);
    const fallbackIndex = clamp(
      Math.floor(scrollTop.value / ROW_HEIGHT),
      0,
      Math.max(0, oldIds.length - 1),
    );
    let anchorIndex = oldRange.start < oldRange.end ? oldRange.start : fallbackIndex;
    while (anchorIndex < oldIds.length && !current.has(oldIds[anchorIndex])) {
      anchorIndex += 1;
    }
    const anchorId = oldIds[anchorIndex];

    unseenIds.value = reconcileUnseenIds(unseenIds.value, previousIds, ids);
    previousIds = current;

    if (shouldFollow && additions) {
      scrollToMax();
      return;
    }

    const nextAnchorIndex = anchorId === undefined ? -1 : ids.indexOf(anchorId);
    if (nextAnchorIndex >= 0) {
      scrollTop.value = anchoredScrollTop(
        scrollTop.value,
        anchorIndex,
        nextAnchorIndex,
        ROW_HEIGHT,
        layout.value.maxScrollTop,
      );
    }
    setScrollOffsets(scrollLeft.value, scrollTop.value);
  },
  { flush: "sync" },
);

watch(
  () => logsStore.loading,
  (loading) => {
    if (!loading) resetFollowState();
  },
);

watch(
  layout,
  () => {
    setScrollOffsets(scrollLeft.value, scrollTop.value);
  },
  { deep: true },
);

function scheduleDraw(): void {
  if (drawFrame !== undefined) return;
  drawFrame = window.requestAnimationFrame(() => {
    drawFrame = undefined;
    drawCanvas();
  });
}

function drawText(
  ctx: CanvasRenderingContext2D,
  value: string,
  x: number,
  y: number,
  width: number,
  color: string,
  leftPadding = 8,
): void {
  void width;
  ctx.fillStyle = color;
  ctx.fillText(value, x + leftPadding, y + ROW_HEIGHT / 2);
}

function drawArrow(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  direction: "up" | "down",
  color: string,
): void {
  const sign = direction === "down" ? 1 : -1;
  ctx.strokeStyle = color;
  ctx.lineWidth = 1.4;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  ctx.beginPath();
  ctx.moveTo(x, y - 4 * sign);
  ctx.lineTo(x, y + 4 * sign);
  ctx.moveTo(x - 3, y + sign);
  ctx.lineTo(x, y + 4 * sign);
  ctx.lineTo(x + 3, y + sign);
  ctx.stroke();
}

function drawHeader(ctx: CanvasRenderingContext2D, colors: Palette): void {
  const targetLayout = layout.value;
  ctx.fillStyle = colors.panel;
  ctx.fillRect(0, 0, viewportWidth.value, HEADER_HEIGHT);
  ctx.save();
  ctx.beginPath();
  ctx.rect(0, 0, targetLayout.bodyWidth, HEADER_HEIGHT);
  ctx.clip();
  ctx.fillStyle = colors.panel;
  ctx.fillRect(0, 0, targetLayout.bodyWidth, HEADER_HEIGHT);
  ctx.font = `600 12px ${colors.uiFont}`;
  ctx.textBaseline = "middle";

  let globalX = 0;
  const drawHeaderCell = (
    width: number,
    label: string,
    hovered: boolean,
    icon?: "sort" | "menu" | "add",
    drawRightBorder = true,
    disabled = false,
  ) => {
    const x = globalX - scrollLeft.value;
    if (x + width > 0 && x < targetLayout.bodyWidth) {
      if (hovered) {
        ctx.fillStyle = colors.hover;
        ctx.fillRect(x, 0, width, HEADER_HEIGHT);
      }
      ctx.save();
      ctx.beginPath();
      ctx.rect(x, 0, width, HEADER_HEIGHT);
      ctx.clip();
      ctx.fillStyle = disabled ? colors.textFaint : colors.textSecondary;
      ctx.globalAlpha = disabled ? 0.45 : 1;
      if (icon === "add") {
        ctx.font = `12px ${colors.uiFont}`;
        ctx.fillText("+", x + 9, HEADER_HEIGHT / 2);
        ctx.font = `600 12px ${colors.uiFont}`;
        ctx.fillText(label, x + 24, HEADER_HEIGHT / 2);
      } else {
        ctx.fillText(label, x + 8, HEADER_HEIGHT / 2);
        if (icon === "sort") {
          drawArrow(
            ctx,
            x + Math.min(width - 9, 25),
            HEADER_HEIGHT / 2,
            logsStore.sortDesc ? "down" : "up",
            colors.textSecondary,
          );
        } else if (icon === "menu") {
          ctx.globalAlpha = 1;
          ctx.fillStyle = colors.panel;
          ctx.fillRect(x + width - 21, 0, 21, HEADER_HEIGHT);
          if (hovered) {
            ctx.fillStyle = colors.hover;
            ctx.fillRect(x + width - 21, 0, 21, HEADER_HEIGHT);
          }
          ctx.strokeStyle = colors.textFaint;
          ctx.lineWidth = 1;
          ctx.beginPath();
          ctx.moveTo(x + width - 13, HEADER_HEIGHT / 2 - 2);
          ctx.lineTo(x + width - 9, HEADER_HEIGHT / 2 + 2);
          ctx.lineTo(x + width - 5, HEADER_HEIGHT / 2 - 2);
          ctx.stroke();
        }
      }
      ctx.restore();
      if (drawRightBorder) {
        ctx.strokeStyle = colors.border;
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.moveTo(x + width - 0.5, 0);
        ctx.lineTo(x + width - 0.5, HEADER_HEIGHT);
        ctx.stroke();
      }
    }
    globalX += width;
  };

  drawHeaderCell(
    ID_COLUMN_WIDTH,
    "id",
    hoverHit.value?.type === "header-id",
    "sort",
    columns.value.length > 0 || !readonly,
  );
  columns.value.forEach((column, index) => {
    drawHeaderCell(
      columnWidth(index),
      column.name,
      hoverHit.value?.type === "header-column" && hoverHit.value.index === index,
      readonly ? undefined : "menu",
      !readonly || index < columns.value.length - 1,
    );
  });
  if (!readonly) {
    drawHeaderCell(
      ADD_COLUMN_WIDTH,
      "添加列",
      hoverHit.value?.type === "header-add",
      "add",
      false,
      sessionsStore.viewingSessionId === null,
    );
  }

  if (!readonly && hoverHit.value?.type === "resize") {
    let boundary = ID_COLUMN_WIDTH;
    for (let index = 0; index <= hoverHit.value.index; index += 1) {
      boundary += columnWidth(index);
    }
    const x = boundary - scrollLeft.value;
    ctx.strokeStyle = colors.accent;
    ctx.globalAlpha = 0.5;
    ctx.lineWidth = 3;
    ctx.beginPath();
    ctx.moveTo(x, 0);
    ctx.lineTo(x, HEADER_HEIGHT);
    ctx.stroke();
  }
  ctx.restore();
  ctx.strokeStyle = colors.borderStrong;
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(0, HEADER_HEIGHT - 0.5);
  ctx.lineTo(viewportWidth.value, HEADER_HEIGHT - 0.5);
  ctx.stroke();
}

function codeColor(index: number, value: string, colors: Palette): string {
  switch (cellClass(index, value)) {
    case "code-2xx":
      return colors.success;
    case "code-3xx":
      return colors.accent;
    case "code-4xx":
      return colors.warning;
    case "code-5xx":
      return colors.danger;
    default:
      return colors.text;
  }
}

function drawRows(ctx: CanvasRenderingContext2D, colors: Palette): void {
  const targetLayout = layout.value;
  ctx.save();
  ctx.beginPath();
  ctx.rect(0, HEADER_HEIGHT, targetLayout.bodyWidth, targetLayout.bodyHeight);
  ctx.clip();
  ctx.font = `12px ${colors.monoFont}`;
  ctx.textBaseline = "middle";

  const start = clamp(
    Math.floor(scrollTop.value / ROW_HEIGHT),
    0,
    rowIds.value.length,
  );
  const end = clamp(
    Math.ceil((scrollTop.value + targetLayout.bodyHeight) / ROW_HEIGHT) + 1,
    start,
    rowIds.value.length,
  );

  for (let index = start; index < end; index += 1) {
    const id = rowIds.value[index];
    const row = logsStore.row(id);
    const y = HEADER_HEIGHT + index * ROW_HEIGHT - scrollTop.value;
    const selected = id === panelLogId.value;

    if (hoveredRow.value === index) {
      ctx.fillStyle = colors.selected;
      ctx.fillRect(0, y, targetLayout.bodyWidth, ROW_HEIGHT);
    } else if (selected) {
      ctx.save();
      ctx.globalAlpha = 0.14;
      ctx.fillStyle = colors.accent;
      ctx.fillRect(0, y, targetLayout.bodyWidth, ROW_HEIGHT);
      ctx.restore();
    } else if (index % 2 === 1) {
      ctx.fillStyle = colors.stripe;
      ctx.fillRect(0, y, targetLayout.bodyWidth, ROW_HEIGHT);
    }

    let globalX = 0;
    const drawCell = (
      width: number,
      value: string,
      color: string,
      error: boolean,
      dotClass = "",
      drawRightBorder = true,
    ) => {
      const x = globalX - scrollLeft.value;
      if (x + width > 0 && x < targetLayout.bodyWidth) {
        if (error) {
          ctx.save();
          ctx.globalAlpha = 0.18;
          ctx.fillStyle = colors.danger;
          ctx.fillRect(x, y, width, ROW_HEIGHT);
          ctx.globalAlpha = 0.6;
          ctx.strokeStyle = colors.danger;
          ctx.strokeRect(x + 0.5, y + 0.5, width - 1, ROW_HEIGHT - 1);
          ctx.restore();
        }
        if (dotClass) {
          ctx.fillStyle =
            dotClass === "active"
              ? colors.accent
              : dotClass === "failed"
                ? colors.danger
                : colors.textFaint;
          ctx.beginPath();
          ctx.arc(x + 4, y + ROW_HEIGHT / 2, 2, 0, Math.PI * 2);
          ctx.fill();
        }
        ctx.save();
        ctx.beginPath();
        ctx.rect(x, y, width, ROW_HEIGHT);
        ctx.clip();
        drawText(ctx, value, x, y, width, color);
        ctx.restore();
        if (drawRightBorder) {
          ctx.strokeStyle = colors.border;
          ctx.lineWidth = 1;
          ctx.beginPath();
          ctx.moveTo(x + width - 0.5, y);
          ctx.lineTo(x + width - 0.5, y + ROW_HEIGHT);
          ctx.stroke();
        }
      }
      globalX += width;
    };

    drawCell(ID_COLUMN_WIDTH, String(id), colors.text, false, outcomeDotClass(row));
    columns.value.forEach((_, columnIndex) => {
      const value = displayCellValue(row, columnIndex);
      const isLast = readonly && columnIndex === columns.value.length - 1;
      drawCell(
        columnWidth(columnIndex),
        value,
        codeColor(columnIndex, value, colors),
        logsStore.cellError(id, columnIndex) !== undefined,
        "",
        !isLast,
      );
    });
    if (!readonly) drawCell(ADD_COLUMN_WIDTH, "", colors.text, false, "", false);

    ctx.strokeStyle = colors.border;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(0, y + ROW_HEIGHT - 0.5);
    ctx.lineTo(targetLayout.bodyWidth, y + ROW_HEIGHT - 0.5);
    ctx.stroke();
  }

  if (!logsStore.loading && rowIds.value.length === 0) {
    const message =
      sessionsStore.viewingSessionId === null
        ? "请选择或创建一个会话"
        : logsStore.filterActive
          ? "没有匹配过滤条件的记录"
          : "暂无抓包记录";
    ctx.fillStyle = colors.textFaint;
    ctx.font = `13px ${colors.uiFont}`;
    ctx.textAlign = "center";
    ctx.fillText(message, targetLayout.bodyWidth / 2, HEADER_HEIGHT + 48);
    ctx.textAlign = "start";
  }
  ctx.restore();
}

function drawScrollbars(ctx: CanvasRenderingContext2D, colors: Palette): void {
  const targetLayout = layout.value;
  const vertical = verticalThumb(targetLayout);
  const horizontal = horizontalThumb(targetLayout);

  if (targetLayout.vertical) {
    ctx.fillStyle = colors.app;
    ctx.fillRect(
      targetLayout.bodyWidth,
      HEADER_HEIGHT,
      SCROLLBAR_SIZE,
      targetLayout.bodyHeight,
    );
  }
  if (vertical) {
    roundedRect(ctx, vertical, SCROLLBAR_SIZE / 2);
    ctx.fillStyle =
      hoverHit.value?.type === "vertical-thumb" ? colors.textFaint : colors.borderStrong;
    ctx.fill();
  }
  if (targetLayout.horizontal) {
    ctx.fillStyle = colors.app;
    ctx.fillRect(
      0,
      HEADER_HEIGHT + targetLayout.bodyHeight,
      targetLayout.bodyWidth,
      SCROLLBAR_SIZE,
    );
  }
  if (horizontal) {
    roundedRect(ctx, horizontal, SCROLLBAR_SIZE / 2);
    ctx.fillStyle =
      hoverHit.value?.type === "horizontal-thumb" ? colors.textFaint : colors.borderStrong;
    ctx.fill();
  }
  if (targetLayout.horizontal && targetLayout.vertical) {
    ctx.fillStyle = colors.app;
    ctx.fillRect(
      targetLayout.bodyWidth,
      HEADER_HEIGHT + targetLayout.bodyHeight,
      SCROLLBAR_SIZE,
      SCROLLBAR_SIZE,
    );
  }
}

function drawNewLogsPill(ctx: CanvasRenderingContext2D, colors: Palette): void {
  pillRect = null;
  if (following.value || unseenIds.value.size === 0 || logsStore.loading) return;
  const targetLayout = layout.value;
  const count = String(unseenIds.value.size);
  ctx.font = `600 12px ${colors.uiFont}`;
  const width = Math.ceil(ctx.measureText(count).width) + 34;
  const height = 24;
  const y = logsStore.sortDesc
    ? HEADER_HEIGHT + 8
    : HEADER_HEIGHT + targetLayout.bodyHeight - height - 8;
  pillRect = {
    x: Math.max(8, (targetLayout.bodyWidth - width) / 2),
    y,
    width,
    height,
  };
  ctx.save();
  ctx.shadowColor = "rgba(0, 0, 0, 0.2)";
  ctx.shadowBlur = 8;
  ctx.shadowOffsetY = 2;
  roundedRect(ctx, pillRect, height / 2);
  ctx.fillStyle = colors.accent;
  ctx.fill();
  ctx.restore();

  drawArrow(
    ctx,
    pillRect.x + 13,
    pillRect.y + height / 2,
    logsStore.sortDesc ? "up" : "down",
    colors.accentText,
  );
  ctx.fillStyle = colors.accentText;
  ctx.textBaseline = "middle";
  ctx.fillText(count, pillRect.x + 24, pillRect.y + height / 2);
}

function drawCanvas(): void {
  const target = canvas.value;
  if (!target || viewportWidth.value <= 0 || viewportHeight.value <= 0) return;
  const ratio = Math.max(1, window.devicePixelRatio || 1);
  const pixelWidth = Math.round(viewportWidth.value * ratio);
  const pixelHeight = Math.round(viewportHeight.value * ratio);
  if (target.width !== pixelWidth || target.height !== pixelHeight) {
    target.width = pixelWidth;
    target.height = pixelHeight;
  }
  const ctx = target.getContext("2d");
  if (!ctx) return;
  ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
  ctx.clearRect(0, 0, viewportWidth.value, viewportHeight.value);
  const colors = palette();
  ctx.fillStyle = colors.app;
  ctx.fillRect(0, 0, viewportWidth.value, viewportHeight.value);
  drawRows(ctx, colors);
  drawHeader(ctx, colors);
  drawNewLogsPill(ctx, colors);
  drawScrollbars(ctx, colors);
}

function canvasPoint(event: MouseEvent | PointerEvent): { x: number; y: number } {
  const rect = canvas.value?.getBoundingClientRect();
  return {
    x: rect ? event.clientX - rect.left : 0,
    y: rect ? event.clientY - rect.top : 0,
  };
}

function hitTest(x: number, y: number): HitTarget | null {
  const targetLayout = layout.value;
  if (pointInRect(x, y, pillRect)) return { type: "pill" };

  const vertical = verticalThumb(targetLayout);
  if (
    targetLayout.vertical &&
    x >= targetLayout.bodyWidth &&
    y >= HEADER_HEIGHT &&
    y <= HEADER_HEIGHT + targetLayout.bodyHeight
  ) {
    return pointInRect(x, y, vertical)
      ? { type: "vertical-thumb" }
      : { type: "vertical-track" };
  }
  const horizontal = horizontalThumb(targetLayout);
  if (
    targetLayout.horizontal &&
    y >= HEADER_HEIGHT + targetLayout.bodyHeight &&
    x <= targetLayout.bodyWidth
  ) {
    return pointInRect(x, y, horizontal)
      ? { type: "horizontal-thumb" }
      : { type: "horizontal-track" };
  }

  const globalX = x + scrollLeft.value;
  if (y < HEADER_HEIGHT && x < targetLayout.bodyWidth) {
    if (!readonly) {
      let boundary = ID_COLUMN_WIDTH;
      for (let index = 0; index < columns.value.length; index += 1) {
        boundary += columnWidth(index);
        if (Math.abs(globalX - boundary) <= RESIZE_HIT_WIDTH / 2) {
          return { type: "resize", index };
        }
      }
    }
    if (globalX < ID_COLUMN_WIDTH) return { type: "header-id" };
    let columnX = ID_COLUMN_WIDTH;
    for (let index = 0; index < columns.value.length; index += 1) {
      columnX += columnWidth(index);
      if (globalX < columnX) return { type: "header-column", index };
    }
    if (!readonly && globalX < totalWidth.value) return { type: "header-add" };
    return null;
  }

  if (
    y >= HEADER_HEIGHT &&
    y < HEADER_HEIGHT + targetLayout.bodyHeight &&
    x < targetLayout.bodyWidth
  ) {
    const rowIndex = Math.floor(
      (scrollTop.value + y - HEADER_HEIGHT) / ROW_HEIGHT,
    );
    if (rowIndex < 0 || rowIndex >= rowIds.value.length) return null;
    if (globalX < ID_COLUMN_WIDTH) return { type: "row", rowIndex, cellIndex: -1 };
    let columnX = ID_COLUMN_WIDTH;
    for (let index = 0; index < columns.value.length; index += 1) {
      columnX += columnWidth(index);
      if (globalX < columnX) return { type: "row", rowIndex, cellIndex: index };
    }
    return { type: "row", rowIndex, cellIndex: null };
  }
  return null;
}

function hitCursor(hit: HitTarget | null): string {
  if (!hit) return "default";
  if (hit.type === "resize") return "col-resize";
  if (hit.type.endsWith("thumb")) return dragState ? "grabbing" : "grab";
  if (
    hit.type === "header-id" ||
    hit.type === "header-column" ||
    hit.type === "row" ||
    hit.type === "pill"
  ) {
    return "pointer";
  }
  if (hit.type === "header-add") {
    return sessionsStore.viewingSessionId === null ? "default" : "pointer";
  }
  return "default";
}

function onPointerMove(event: PointerEvent): void {
  if (dragState) {
    const point = canvasPoint(event);
    const pointer = dragState.axis === "x" ? point.x : point.y;
    const delta = pointer - dragState.pointerStart;
    const scroll =
      dragState.scrollStart +
      (dragState.thumbRange > 0
        ? (delta / dragState.thumbRange) * dragState.scrollRange
        : 0);
    setScrollOffsets(
      dragState.axis === "x" ? scroll : scrollLeft.value,
      dragState.axis === "y" ? scroll : scrollTop.value,
    );
    return;
  }
  const point = canvasPoint(event);
  const hit = hitTest(point.x, point.y);
  hoverHit.value = hit;
  hoveredRow.value = hit?.type === "row" ? hit.rowIndex : null;
  if (canvas.value) canvas.value.style.cursor = hitCursor(hit);
  scheduleDraw();
}

function onPointerLeave(): void {
  if (dragState) return;
  hoverHit.value = null;
  hoveredRow.value = null;
  scheduleDraw();
}

function beginScrollbarDrag(
  axis: "x" | "y",
  event: PointerEvent,
  thumb: Rect,
): void {
  const targetLayout = layout.value;
  const point = canvasPoint(event);
  dragState = {
    axis,
    pointerStart: axis === "x" ? point.x : point.y,
    scrollStart: axis === "x" ? scrollLeft.value : scrollTop.value,
    scrollRange: axis === "x" ? targetLayout.maxScrollLeft : targetLayout.maxScrollTop,
    thumbRange:
      axis === "x"
        ? targetLayout.bodyWidth - thumb.width
        : targetLayout.bodyHeight - thumb.height,
    pointerId: event.pointerId,
  };
  capturedPointerId = event.pointerId;
  canvas.value?.setPointerCapture(event.pointerId);
}

function jumpScrollbar(axis: "x" | "y", event: PointerEvent, thumb: Rect): void {
  const targetLayout = layout.value;
  const point = canvasPoint(event);
  if (axis === "x") {
    const travel = targetLayout.bodyWidth - thumb.width;
    const ratio = travel > 0 ? (point.x - thumb.width / 2) / travel : 0;
    setScrollOffsets(targetLayout.maxScrollLeft * ratio, scrollTop.value);
  } else {
    const travel = targetLayout.bodyHeight - thumb.height;
    const ratio =
      travel > 0
        ? (point.y - HEADER_HEIGHT - thumb.height / 2) / travel
        : 0;
    setScrollOffsets(scrollLeft.value, targetLayout.maxScrollTop * ratio);
  }
}

function onPointerDown(event: PointerEvent): void {
  const point = canvasPoint(event);
  const hit = hitTest(point.x, point.y);
  if (!hit) return;
  if (hit.type === "resize") {
    ignoreNextClick = true;
    startResize(hit.index, event);
    return;
  }
  const vertical = verticalThumb();
  const horizontal = horizontalThumb();
  if (hit.type === "vertical-thumb" && vertical) {
    ignoreNextClick = true;
    beginScrollbarDrag("y", event, vertical);
  } else if (hit.type === "horizontal-thumb" && horizontal) {
    ignoreNextClick = true;
    beginScrollbarDrag("x", event, horizontal);
  } else if (hit.type === "vertical-track" && vertical) {
    ignoreNextClick = true;
    capturedPointerId = event.pointerId;
    canvas.value?.setPointerCapture(event.pointerId);
    jumpScrollbar("y", event, vertical);
  } else if (hit.type === "horizontal-track" && horizontal) {
    ignoreNextClick = true;
    capturedPointerId = event.pointerId;
    canvas.value?.setPointerCapture(event.pointerId);
    jumpScrollbar("x", event, horizontal);
  }
}

function onPointerUp(event: PointerEvent): void {
  if (capturedPointerId !== null && canvas.value?.hasPointerCapture(capturedPointerId)) {
    canvas.value.releasePointerCapture(capturedPointerId);
  }
  dragState = null;
  capturedPointerId = null;
  if (ignoreNextClick) {
    window.setTimeout(() => {
      ignoreNextClick = false;
    }, 0);
  }
  onPointerMove(event);
}

function onCanvasClick(event: MouseEvent): void {
  if (ignoreNextClick) {
    ignoreNextClick = false;
    return;
  }
  const point = canvasPoint(event);
  const hit = hitTest(point.x, point.y);
  if (!hit) return;
  if (hit.type === "pill") {
    scrollToMax();
  } else if (hit.type === "header-id") {
    toggleSort();
  } else if (hit.type === "header-column" && !readonly) {
    const rect = canvas.value?.getBoundingClientRect();
    let x = ID_COLUMN_WIDTH;
    for (let index = 0; index < hit.index; index += 1) x += columnWidth(index);
    void showColumnMenu(
      hit.index,
      (rect?.left ?? 0) + Math.max(0, x - scrollLeft.value),
      (rect?.top ?? 0) + HEADER_HEIGHT + 4,
    );
  } else if (hit.type === "header-add" && sessionsStore.viewingSessionId !== null) {
    const rect = canvas.value?.getBoundingClientRect();
    void showAddColumnMenu(
      (rect?.left ?? 0) + Math.max(0, totalWidth.value - ADD_COLUMN_WIDTH - scrollLeft.value),
      (rect?.top ?? 0) + HEADER_HEIGHT + 4,
    );
  } else if (hit.type === "row") {
    openRow(rowIds.value[hit.rowIndex]);
  }
}

function onCanvasContextMenu(event: MouseEvent): void {
  const point = canvasPoint(event);
  const hit = hitTest(point.x, point.y);
  if (hit?.type !== "row") return;
  const rowId = rowIds.value[hit.rowIndex];
  if (hit.cellIndex === null) {
    showCellMenu(event, null, rowId);
    return;
  }
  const row = logsStore.row(rowId);
  const value = hit.cellIndex === -1 ? String(row.id) : displayCellValue(row, hit.cellIndex);
  showCellMenu(event, value, rowId);
}

function normalizeWheelDelta(value: number, mode: number, pageSize: number): number {
  if (mode === WheelEvent.DOM_DELTA_LINE) return value * ROW_HEIGHT;
  if (mode === WheelEvent.DOM_DELTA_PAGE) return value * pageSize;
  return value;
}

function onWheel(event: WheelEvent): void {
  const targetLayout = layout.value;
  let x = normalizeWheelDelta(event.deltaX, event.deltaMode, targetLayout.bodyWidth);
  let y = normalizeWheelDelta(event.deltaY, event.deltaMode, targetLayout.bodyHeight);
  if (event.shiftKey && x === 0) {
    x = y;
    y = 0;
  }
  if (targetLayout.maxScrollLeft > 0 || targetLayout.maxScrollTop > 0) {
    event.preventDefault();
    setScrollOffsets(scrollLeft.value + x, scrollTop.value + y);
  }
}

watchEffect(() => {
  const targetLayout = layout.value;
  const start = clamp(
    Math.floor(scrollTop.value / ROW_HEIGHT),
    0,
    rowIds.value.length,
  );
  const end = clamp(
    Math.ceil((scrollTop.value + targetLayout.bodyHeight) / ROW_HEIGHT) + 1,
    start,
    rowIds.value.length,
  );
  for (let index = start; index < end; index += 1) {
    const id = rowIds.value[index];
    const row = logsStore.rowsById.get(id);
    if (row) {
      void row.updated_at;
      for (const cell of row.cells) void cell;
    }
    for (let columnIndex = 0; columnIndex < columns.value.length; columnIndex += 1) {
      void logsStore.cellError(id, columnIndex);
    }
    void proxyStore.isNetlogActive(sessionsStore.viewingSessionId, id);
  }
  void appStore.theme;
  void logsStore.loading;
  void unseenIds.value.size;
  void hoveredRow.value;
  void hoverHit.value;
  void panelLogId.value;
  scheduleDraw();
});

onMounted(() => {
  const updateSize = () => {
    const rect = tableWrap.value?.getBoundingClientRect();
    if (!rect) return;
    viewportWidth.value = rect.width;
    viewportHeight.value = rect.height;
    setScrollOffsets(scrollLeft.value, scrollTop.value);
    if (panelHeight.value > maxPanelHeight()) {
      panelHeight.value = maxPanelHeight();
    }
    // ResizeObserver 在绘制前触发，这里同步重绘，避免 canvas 的 CSS 尺寸已变
    // 而位图仍待下一帧 rAF 更新，导致浏览器拉伸旧位图产生形变。
    if (drawFrame !== undefined) {
      window.cancelAnimationFrame(drawFrame);
      drawFrame = undefined;
    }
    drawCanvas();
  };
  if (tableWrap.value) {
    resizeObserver = new ResizeObserver(updateSize);
    resizeObserver.observe(tableWrap.value);
    updateSize();
  }
  themeObserver = new MutationObserver(scheduleDraw);
  themeObserver.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["data-theme"],
  });
});

onBeforeUnmount(() => {
  resizeObserver?.disconnect();
  themeObserver?.disconnect();
  if (drawFrame !== undefined) window.cancelAnimationFrame(drawFrame);
});
</script>

<template>
  <div ref="wrap" class="lt-wrap">
    <div ref="tableWrap" class="lt-table">
      <canvas
        ref="canvas"
        class="lt-canvas"
        @pointermove="onPointerMove"
        @pointerleave="onPointerLeave"
        @pointerdown="onPointerDown"
        @pointerup="onPointerUp"
        @pointercancel="onPointerUp"
        @click="onCanvasClick"
        @contextmenu="onCanvasContextMenu"
        @wheel="onWheel"
      />
      <div v-if="logsStore.loading" class="lt-loading" role="status" aria-label="正在加载日志">
        <span class="lt-spinner" aria-hidden="true" />
      </div>
    </div>
    <template v-if="panelSessionId !== null && panelLogId !== null">
      <div class="lt-panel" :style="{ height: `${panelHeight}px` }">
        <div
          class="lt-splitter"
          :class="{ dragging: panelDragging }"
          title="拖动调整详情面板高度"
          @pointerdown="startPanelDrag"
        />
        <header class="lt-panel-bar">
          <span class="lt-panel-title mono">#{{ panelLogId }}</span>
          <span class="lt-panel-label">详情</span>
          <span class="lt-panel-spacer" />
          <button class="btn icon" title="在窗口中打开" @click="openPanelInWindow">
            <Io5OpenOutline :size="14" />
          </button>
          <button class="btn icon" title="关闭" @click="closePanel">
            <Io5Close :size="14" />
          </button>
        </header>
        <LogDetailWindow
          :key="panelLogId"
          :session-id="panelSessionId"
          :log-id="panelLogId"
          class="lt-panel-body"
        />
      </div>
    </template>
  </div>
</template>

<style scoped>
.lt-wrap {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.lt-table {
  flex: 1;
  min-height: 0;
  position: relative;
  overflow: hidden;
}
.lt-splitter {
  position: absolute;
  top: -3px;
  left: 0;
  right: 0;
  height: 5px;
  z-index: 10;
  cursor: row-resize;
  touch-action: none;
}
.lt-splitter::after {
  content: "";
  position: absolute;
  top: 2px;
  left: 0;
  width: 100%;
  height: 2px;
  background: transparent;
  transition: background 0.12s;
}
.lt-splitter:hover::after,
.lt-splitter.dragging::after {
  background: var(--accent);
}
.lt-panel {
  flex: none;
  min-height: 0;
  position: relative;
  display: flex;
  flex-direction: column;
  border-top: 1px solid var(--border);
  background: var(--bg-panel);
}
.lt-panel-bar {
  flex: none;
  display: flex;
  align-items: center;
  gap: var(--space-2);
  height: 32px;
  padding: 0 var(--space-2);
  border-bottom: 1px solid var(--border);
}
.lt-panel-title {
  font-weight: 600;
}
.lt-panel-label {
  color: var(--text-secondary);
  font-size: 12px;
}
.lt-panel-spacer {
  flex: 1;
}
.lt-panel-body {
  flex: 1;
  min-height: 0;
}
.lt-canvas {
  display: block;
  width: 100%;
  height: 100%;
  touch-action: none;
}
.lt-loading {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  pointer-events: none;
  background: color-mix(in srgb, var(--bg-app) 55%, transparent);
  z-index: 6;
}
.lt-spinner {
  width: 28px;
  height: 28px;
  border: 3px solid color-mix(in srgb, var(--accent) 30%, transparent);
  border-top-color: var(--accent);
  border-radius: 50%;
  animation: lt-spin 0.8s linear infinite;
}
@keyframes lt-spin {
  to {
    transform: rotate(360deg);
  }
}
@media (prefers-reduced-motion: reduce) {
  .lt-spinner {
    animation: none;
  }
}
</style>
