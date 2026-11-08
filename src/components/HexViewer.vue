<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { copyText } from "../utils/clipboard";
import {
  formatHex,
  formatAscii,
  ASCII_BYTES,
  hexByteAt,
  hexViewport,
  hexScrollThumb,
  HEX_BYTES,
  HEX_HEADER_HEIGHT,
  HEX_SCROLLBAR_SIZE,
  HEX_ROW_HEIGHT,
  type HexViewerCopyFormat,
  type HexRightPane,
  type HexSelection,
} from "../utils/hex-viewer";

const props = defineProps<{ bytes: Uint8Array }>();
const rightPane = defineModel<HexRightPane>("rightPane", { default: "ascii" });
const ascii = computed(() => rightPane.value === "ascii");
const emit = defineEmits<{
  "selection-change": [selection: HexSelection | null];
  "layout-change": [layout: { bytesPerRow: number; groups: number }];
  contextmenu: [event: MouseEvent];
  copied: [byteCount: number];
}>();
const viewport = ref<HTMLElement>();
const canvas = ref<HTMLCanvasElement>();
const width = ref(600);
const height = ref(0);
const charWidth = ref(7.2);
const scrollLeft = ref(0);
const scrollTop = ref(0);
const scene = computed(() =>
  hexViewport(width.value, height.value, charWidth.value, ascii.value, props.bytes.length),
);
const layout = computed(() => scene.value.layout);
const verticalThumb = computed(() =>
  hexScrollThumb(scene.value.bodyHeight, scene.value.contentHeight, scrollTop.value),
);
const horizontalThumb = computed(() =>
  hexScrollThumb(scene.value.bodyWidth, layout.value.width, scrollLeft.value),
);
const hoverAxis = ref<"x" | "y" | null>(null);
const scrolling = ref(false);
const menuOpen = ref(false);
const menuHovered = ref(false);
const menuIndex = ref(-1);
const paneOptions = [
  { value: "ascii", label: "ASCII" },
  { value: "none", label: "关闭" },
] as const;
const paneMenu = computed(() => ({
  x: Math.max(2, width.value - 122),
  y: Math.max(0, Math.min(HEX_HEADER_HEIGHT + 3, height.value - (paneOptions.length * 24 + 8) - 2)),
  width: Math.max(0, Math.min(120, width.value - 4)),
  height: paneOptions.length * 24 + 8,
}));
const anchor = ref<number | null>(null);
const focus = ref<number | null>(null);
const selection = computed<HexSelection | null>(() =>
  anchor.value === null || focus.value === null
    ? null
    : { start: Math.min(anchor.value, focus.value), end: Math.max(anchor.value, focus.value) + 1 },
);
const offset = (value: number) => value.toString(16).padStart(8, "0");
let paintFrame = 0;
let dragFrame = 0;
let dragPointer: { x: number; y: number } | null = null;
let dragPane: "hex" | "ascii" = "hex";
let barDrag: {
  axis: "x" | "y";
  pointer: number;
  scroll: number;
  travel: number;
  maxScroll: number;
} | null = null;
let resizeObserver: ResizeObserver | undefined;
let themeObserver: MutationObserver | undefined;

function openRightPaneMenu(): void {
  menuOpen.value = true;
  menuIndex.value = paneOptions.findIndex((option) => option.value === rightPane.value);
}

function menuItemAt(x: number, y: number): number {
  const menu = paneMenu.value;
  if (
    x < menu.x + 4 ||
    x >= menu.x + menu.width - 4 ||
    y < menu.y + 4 ||
    y >= menu.y + menu.height - 4
  )
    return -1;
  return Math.floor((y - menu.y - 4) / 24);
}

function clearHover(): void {
  hoverAxis.value = null;
  menuHovered.value = false;
  menuIndex.value = -1;
}

function schedulePaint(): void {
  if (!paintFrame) paintFrame = requestAnimationFrame(paint);
}

function measure(): void {
  const view = viewport.value;
  const context = canvas.value?.getContext("2d");
  if (!view || !context) return;
  context.font = `12px ${getComputedStyle(view).fontFamily}`;
  charWidth.value = context.measureText("0").width;
  width.value = view.clientWidth;
  height.value = view.clientHeight;
  schedulePaint();
}

function paint(): void {
  paintFrame = 0;
  const view = viewport.value;
  const element = canvas.value;
  const context = element?.getContext("2d");
  if (!view || !element || !context || !height.value) return;
  const metrics = layout.value;
  const dpr = window.devicePixelRatio || 1;
  const pixelWidth = Math.round(width.value * dpr);
  const canvasHeight = Math.round(height.value * dpr);
  if (element.width !== pixelWidth || element.height !== canvasHeight) {
    element.width = pixelWidth;
    element.height = canvasHeight;
  }
  context.setTransform(dpr, 0, 0, dpr, 0, 0);
  const style = getComputedStyle(view);
  const color = (name: string) => style.getPropertyValue(name).trim();
  const text = color("--text");
  const secondary = color("--text-secondary");
  const faint = color("--text-faint");
  const accent = color("--accent");
  const selected = color("--bg-selected-strong");
  const linked = color("--bg-selected");
  const border = color("--border");
  context.font = `12px ${style.fontFamily}`;
  context.textBaseline = "middle";
  context.fillStyle = color("--bg-panel");
  context.fillRect(0, 0, width.value, height.value);

  context.save();
  context.beginPath();
  context.rect(0, HEX_HEADER_HEIGHT, scene.value.bodyWidth, scene.value.bodyHeight);
  context.clip();
  context.translate(-scrollLeft.value, 0);
  const firstRow = Math.floor(scrollTop.value / HEX_ROW_HEIGHT);
  const lastRow = Math.min(
    Math.ceil(props.bytes.length / metrics.bytesPerRow),
    Math.ceil((scrollTop.value + scene.value.bodyHeight) / HEX_ROW_HEIGHT),
  );
  for (let row = firstRow; row < lastRow; row++) {
    const start = row * metrics.bytesPerRow;
    const end = Math.min(start + metrics.bytesPerRow, props.bytes.length);
    const top = HEX_HEADER_HEIGHT + row * HEX_ROW_HEIGHT - scrollTop.value;
    const baseline = top + HEX_ROW_HEIGHT / 2;
    const active = selection.value && selection.value.start < end && selection.value.end > start;
    context.fillStyle = active ? accent : faint;
    context.fillText(offset(start), 6, baseline);
    if (active && selection.value) {
      const first = Math.max(start, selection.value.start) - start;
      const last = Math.min(end, selection.value.end) - start - 1;
      const left =
        metrics.hexStart +
        Math.floor(first / 4) * metrics.groupStride +
        (first % 4) * metrics.charWidth * 2;
      const right =
        metrics.hexStart +
        Math.floor(last / 4) * metrics.groupStride +
        ((last % 4) + 1) * metrics.charWidth * 2;
      context.fillStyle = selected;
      context.fillRect(left, top, right - left, HEX_ROW_HEIGHT);
      if (ascii.value) {
        context.fillStyle = linked;
        context.fillRect(
          metrics.asciiStart + first * metrics.charWidth,
          top,
          (last - first + 1) * metrics.charWidth,
          HEX_ROW_HEIGHT,
        );
      }
    }
    for (let index = start; index < end; index++) {
      const column = index - start;
      const x =
        metrics.hexStart +
        Math.floor(column / 4) * metrics.groupStride +
        (column % 4) * metrics.charWidth * 2;
      const asciiX = metrics.asciiStart + column * metrics.charWidth;
      const isSelected =
        selection.value && index >= selection.value.start && index < selection.value.end;
      const byte = props.bytes[index];
      context.fillStyle = text;
      context.fillText(HEX_BYTES[byte], x, baseline);
      if (ascii.value) {
        const printable = byte >= 0x20 && byte <= 0x7e;
        context.fillStyle = isSelected ? text : printable ? secondary : faint;
        context.fillText(ASCII_BYTES[byte], asciiX, baseline);
      }
    }
  }
  if (ascii.value) {
    context.fillStyle = border;
    context.fillRect(metrics.asciiStart - 7, HEX_HEADER_HEIGHT, 1, scene.value.bodyHeight);
  }
  if (!props.bytes.length) {
    context.fillStyle = faint;
    context.fillText("无二进制数据", metrics.hexStart, HEX_HEADER_HEIGHT + HEX_ROW_HEIGHT);
  }
  context.restore();

  context.fillStyle = color("--bg-app");
  context.fillRect(0, 0, width.value, HEX_HEADER_HEIGHT);
  context.fillStyle = border;
  context.fillRect(0, HEX_HEADER_HEIGHT - 1, width.value, 1);
  context.save();
  context.beginPath();
  context.rect(0, 0, scene.value.bodyWidth, HEX_HEADER_HEIGHT - 1);
  context.clip();
  context.translate(-scrollLeft.value, 0);
  context.font = `10px ${style.fontFamily}`;
  context.fillStyle = faint;
  context.fillText("OFFSET", 6, HEX_HEADER_HEIGHT / 2);
  context.fillText("HEX", metrics.hexStart, HEX_HEADER_HEIGHT / 2);
  if (ascii.value) context.fillText("ASCII", metrics.asciiStart, HEX_HEADER_HEIGHT / 2);
  context.restore();

  for (const axis of ["y", "x"] as const) {
    const thumb = axis === "y" ? verticalThumb.value : horizontalThumb.value;
    if (!thumb) continue;
    const vertical = axis === "y";
    const x = vertical ? scene.value.bodyWidth : 0;
    const y = vertical ? HEX_HEADER_HEIGHT : HEX_HEADER_HEIGHT + scene.value.bodyHeight;
    context.fillStyle = color("--bg-app");
    context.fillRect(
      x,
      y,
      vertical ? HEX_SCROLLBAR_SIZE : scene.value.bodyWidth,
      vertical ? scene.value.bodyHeight : HEX_SCROLLBAR_SIZE,
    );
    context.fillStyle = hoverAxis.value === axis ? faint : color("--border-strong");
    context.beginPath();
    context.roundRect(
      x + (vertical ? 2 : thumb.start + 2),
      y + (vertical ? thumb.start + 2 : 2),
      vertical ? 4 : Math.max(0, thumb.size - 4),
      vertical ? Math.max(0, thumb.size - 4) : 4,
      2,
    );
    context.fill();
  }
  context.fillStyle = menuHovered.value || menuOpen.value ? color("--bg-hover") : color("--bg-app");
  context.fillRect(width.value - 20, 0, 20, HEX_HEADER_HEIGHT - 1);
  context.strokeStyle = menuHovered.value || menuOpen.value ? text : faint;
  context.lineWidth = 1;
  context.beginPath();
  context.moveTo(width.value - 14, 8);
  context.lineTo(width.value - 10, 12);
  context.lineTo(width.value - 6, 8);
  context.stroke();
  if (menuOpen.value) {
    const menu = paneMenu.value;
    context.save();
    context.shadowColor = "rgba(0, 0, 0, 0.2)";
    context.shadowBlur = 10;
    context.shadowOffsetY = 3;
    context.fillStyle = color("--bg-elevated");
    context.beginPath();
    context.roundRect(menu.x, menu.y, menu.width, menu.height, 4);
    context.fill();
    context.restore();
    context.strokeStyle = color("--border-strong");
    context.stroke();
    context.font = `11px ${style.fontFamily}`;
    for (let index = 0; index < paneOptions.length; index++) {
      const top = menu.y + 4 + index * 24;
      if (menuIndex.value === index) {
        context.fillStyle = color("--bg-hover");
        context.fillRect(menu.x + 4, top, menu.width - 8, 24);
      }
      context.fillStyle = text;
      context.fillText(paneOptions[index].label, menu.x + 28, top + 12);
      if (rightPane.value === paneOptions[index].value) {
        context.strokeStyle = text;
        context.beginPath();
        context.moveTo(menu.x + 10, top + 12);
        context.lineTo(menu.x + 13, top + 15);
        context.lineTo(menu.x + 19, top + 9);
        context.stroke();
      }
    }
  }
}

function scrollTo(left: number, top: number): void {
  scrollLeft.value = Math.max(0, Math.min(scene.value.maxScrollLeft, left));
  scrollTop.value = Math.max(0, Math.min(scene.value.maxScrollTop, top));
  updateDrag();
  schedulePaint();
}

function updateDrag(): void {
  if (!dragPointer || !canvas.value) return;
  const rect = canvas.value.getBoundingClientRect();
  focus.value = hexByteAt(
    layout.value,
    Math.max(0, Math.min(scene.value.bodyWidth - 1, dragPointer.x - rect.left)) + scrollLeft.value,
    Math.max(
      HEX_HEADER_HEIGHT,
      Math.min(HEX_HEADER_HEIGHT + scene.value.bodyHeight - 1, dragPointer.y - rect.top),
    ),
    scrollTop.value,
    props.bytes.length,
    dragPane,
  );
}

function autoScroll(): void {
  dragFrame = 0;
  if (!dragPointer || !canvas.value) return;
  const rect = canvas.value.getBoundingClientRect();
  const x = dragPointer.x - rect.left;
  const y = dragPointer.y - rect.top;
  const dx = x < 0 ? x : x > scene.value.bodyWidth ? x - scene.value.bodyWidth : 0;
  const bottom = HEX_HEADER_HEIGHT + scene.value.bodyHeight;
  const dy = y < HEX_HEADER_HEIGHT ? y - HEX_HEADER_HEIGHT : y > bottom ? y - bottom : 0;
  const previousLeft = scrollLeft.value;
  const previousTop = scrollTop.value;
  scrollTo(
    previousLeft + Math.max(-33, Math.min(33, dx)),
    previousTop + Math.max(-33, Math.min(33, dy)),
  );
  if (scrollLeft.value !== previousLeft || scrollTop.value !== previousTop)
    dragFrame = requestAnimationFrame(autoScroll);
}

function scrollbarAt(x: number, y: number): "x" | "y" | null {
  if (
    verticalThumb.value &&
    x >= scene.value.bodyWidth &&
    y >= HEX_HEADER_HEIGHT &&
    y < HEX_HEADER_HEIGHT + scene.value.bodyHeight
  )
    return "y";
  if (
    horizontalThumb.value &&
    y >= HEX_HEADER_HEIGHT + scene.value.bodyHeight &&
    x < scene.value.bodyWidth
  )
    return "x";
  return null;
}

function startSelection(event: PointerEvent): void {
  if (event.button !== 0 || !canvas.value) return;
  const rect = canvas.value.getBoundingClientRect();
  const x = event.clientX - rect.left;
  const y = event.clientY - rect.top;
  if (menuOpen.value) {
    const index = menuItemAt(x, y);
    if (index >= 0) rightPane.value = paneOptions[index].value;
    menuOpen.value = false;
    event.preventDefault();
    return;
  }
  if (y < HEX_HEADER_HEIGHT && x >= width.value - 20) {
    canvas.value.focus({ preventScroll: true });
    openRightPaneMenu();
    event.preventDefault();
    return;
  }
  const axis = scrollbarAt(x, y);
  if (axis) {
    const thumb = (axis === "y" ? verticalThumb.value : horizontalThumb.value)!;
    const position = axis === "y" ? y - HEX_HEADER_HEIGHT : x;
    if (position < thumb.start || position > thumb.start + thumb.size) {
      const value = thumb.travel
        ? ((position - thumb.size / 2) / thumb.travel) * thumb.maxScroll
        : 0;
      scrollTo(axis === "x" ? value : scrollLeft.value, axis === "y" ? value : scrollTop.value);
    }
    barDrag = {
      axis,
      pointer: axis === "y" ? event.clientY : event.clientX,
      scroll: axis === "y" ? scrollTop.value : scrollLeft.value,
      travel: thumb.travel,
      maxScroll: thumb.maxScroll,
    };
    scrolling.value = true;
    hoverAxis.value = axis;
  } else {
    if (
      x >= scene.value.bodyWidth ||
      y >= HEX_HEADER_HEIGHT + scene.value.bodyHeight ||
      y < HEX_HEADER_HEIGHT
    )
      return;
    const contentX = x + scrollLeft.value;
    if (contentX >= layout.value.hexStart && contentX < layout.value.hexEnd) dragPane = "hex";
    else if (
      ascii.value &&
      contentX >= layout.value.asciiStart &&
      contentX < layout.value.asciiStart + layout.value.bytesPerRow * charWidth.value
    )
      dragPane = "ascii";
    else return;
    const index = hexByteAt(
      layout.value,
      x + scrollLeft.value,
      y,
      scrollTop.value,
      props.bytes.length,
      dragPane,
    );
    if (index === null) return;
    if (!event.shiftKey || anchor.value === null) anchor.value = index;
    focus.value = index;
    dragPointer = { x: event.clientX, y: event.clientY };
  }
  event.preventDefault();
  canvas.value.focus({ preventScroll: true });
  canvas.value.setPointerCapture(event.pointerId);
}

function moveSelection(event: PointerEvent): void {
  if (barDrag) {
    const delta = (barDrag.axis === "y" ? event.clientY : event.clientX) - barDrag.pointer;
    const value =
      barDrag.scroll + (barDrag.travel ? (delta / barDrag.travel) * barDrag.maxScroll : 0);
    scrollTo(
      barDrag.axis === "x" ? value : scrollLeft.value,
      barDrag.axis === "y" ? value : scrollTop.value,
    );
  } else if (dragPointer) {
    dragPointer = { x: event.clientX, y: event.clientY };
    updateDrag();
    if (!dragFrame) dragFrame = requestAnimationFrame(autoScroll);
  } else if (canvas.value) {
    const rect = canvas.value.getBoundingClientRect();
    const x = event.clientX - rect.left;
    const y = event.clientY - rect.top;
    menuHovered.value = y < HEX_HEADER_HEIGHT && x >= width.value - 20;
    menuIndex.value = menuOpen.value ? menuItemAt(x, y) : -1;
    hoverAxis.value = menuOpen.value ? null : scrollbarAt(x, y);
  }
}

function stopSelection(): void {
  dragPointer = null;
  barDrag = null;
  scrolling.value = false;
  cancelAnimationFrame(dragFrame);
  dragFrame = 0;
}

function onWheel(event: WheelEvent): void {
  if (event.ctrlKey || event.metaKey) return;
  event.preventDefault();
  event.stopPropagation();
  menuOpen.value = false;
  const scale =
    event.deltaMode === 1 ? HEX_ROW_HEIGHT : event.deltaMode === 2 ? scene.value.bodyHeight : 1;
  const horizontal = event.shiftKey && !event.deltaX;
  scrollTo(
    scrollLeft.value + (horizontal ? event.deltaY : event.deltaX) * scale,
    scrollTop.value + (horizontal ? 0 : event.deltaY) * scale,
  );
}

async function copy(format: HexViewerCopyFormat = "group"): Promise<void> {
  if (!selection.value) return;
  const { start, end } = selection.value;
  const bytes = props.bytes.subarray(start, end);
  await copyText(format === "ascii" ? formatAscii(bytes) : formatHex(bytes, format));
  canvas.value?.focus({ preventScroll: true });
  emit("copied", end - start);
}

defineExpose({ copy });

function onCopy(event: ClipboardEvent): void {
  if (!selection.value || !event.clipboardData) return;
  event.preventDefault();
  const { start, end } = selection.value;
  event.clipboardData.setData("text/plain", formatHex(props.bytes.subarray(start, end)));
  emit("copied", end - start);
}

function onContextMenu(event: MouseEvent): void {
  menuOpen.value = false;
  emit("contextmenu", event);
}

function onKey(event: KeyboardEvent): void {
  if (menuOpen.value) {
    if (!["Escape", "ArrowDown", "ArrowUp", "Enter", " "].includes(event.key)) {
      if (event.key === "Tab") menuOpen.value = false;
      return;
    }
    event.preventDefault();
    if (event.key === "Escape") menuOpen.value = false;
    else if (event.key === "ArrowDown" || event.key === "ArrowUp")
      menuIndex.value =
        (menuIndex.value + (event.key === "ArrowDown" ? 1 : paneOptions.length - 1)) %
        paneOptions.length;
    else if (event.key === "Enter" || event.key === " ") {
      if (menuIndex.value >= 0) rightPane.value = paneOptions[menuIndex.value].value;
      menuOpen.value = false;
    }
    return;
  }
  if (event.key === "Enter" || event.key === " ") {
    event.preventDefault();
    openRightPaneMenu();
    return;
  }
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "a") {
    event.preventDefault();
    if (props.bytes.length) {
      anchor.value = 0;
      focus.value = props.bytes.length - 1;
    }
  } else if (event.key === "Escape") {
    anchor.value = focus.value = null;
  } else {
    const page = Math.max(HEX_ROW_HEIGHT, scene.value.bodyHeight - HEX_ROW_HEIGHT);
    const top =
      event.key === "ArrowUp"
        ? scrollTop.value - HEX_ROW_HEIGHT
        : event.key === "ArrowDown"
          ? scrollTop.value + HEX_ROW_HEIGHT
          : event.key === "PageUp"
            ? scrollTop.value - page
            : event.key === "PageDown"
              ? scrollTop.value + page
              : event.key === "Home"
                ? 0
                : event.key === "End"
                  ? scene.value.maxScrollTop
                  : null;
    const left =
      event.key === "ArrowLeft"
        ? scrollLeft.value - charWidth.value * 4
        : event.key === "ArrowRight"
          ? scrollLeft.value + charWidth.value * 4
          : null;
    if (top !== null || left !== null) {
      event.preventDefault();
      scrollTo(left ?? scrollLeft.value, top ?? scrollTop.value);
    }
  }
}

watch(
  [selection, hoverAxis, menuOpen, menuHovered, menuIndex, scrollLeft, scrollTop],
  schedulePaint,
  { flush: "post" },
);
watch(scene, () => scrollTo(scrollLeft.value, scrollTop.value));
watch(selection, (value) => emit("selection-change", value));
watch(
  () => layout.value.bytesPerRow,
  (bytesPerRow) => {
    emit("layout-change", { bytesPerRow, groups: layout.value.groups });
  },
  { immediate: true },
);
watch(ascii, () => {
  stopSelection();
  measure();
});
watch(
  () => props.bytes,
  async () => {
    stopSelection();
    anchor.value = focus.value = null;
    scrollLeft.value = scrollTop.value = 0;
    await nextTick();
    measure();
  },
);
onMounted(() => {
  resizeObserver = new ResizeObserver(measure);
  resizeObserver.observe(viewport.value!);
  themeObserver = new MutationObserver(() => {
    measure();
  });
  themeObserver.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["data-theme", "class", "style"],
  });
  window.addEventListener("resize", measure);
  measure();
});
onBeforeUnmount(() => {
  stopSelection();
  cancelAnimationFrame(paintFrame);
  resizeObserver?.disconnect();
  themeObserver?.disconnect();
  window.removeEventListener("resize", measure);
});
</script>

<template>
  <div class="hex-viewer" @copy="onCopy">
    <div ref="viewport" class="hex-viewport">
      <canvas
        ref="canvas"
        class="hex-canvas"
        :style="{
          width: `${width}px`,
          height: `${height}px`,
          cursor: scrolling
            ? 'grabbing'
            : menuHovered || (menuOpen && menuIndex >= 0)
              ? 'pointer'
              : hoverAxis
                ? 'grab'
                : 'text',
        }"
        tabindex="0"
        aria-label="二进制 Hex 与 ASCII 视图，拖动选择字节，Command 或 Control 加 C 复制，加 A 全选，Enter 打开右侧栏菜单"
        aria-haspopup="menu"
        :aria-expanded="menuOpen"
        @pointerdown="startSelection"
        @pointermove="moveSelection"
        @pointerup="stopSelection"
        @pointercancel="stopSelection"
        @lostpointercapture="stopSelection"
        @contextmenu.prevent="onContextMenu"
        @keydown="onKey"
        @wheel="onWheel"
        @pointerleave="clearHover"
        @blur="menuOpen = false"
      />
    </div>
  </div>
</template>

<style scoped>
.hex-viewer {
  position: relative;
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  background: var(--bg-panel);
}
.hex-viewport {
  flex: 1;
  min-height: 0;
  overflow: hidden;
  font-family: var(--font-mono);
}
.hex-canvas {
  display: block;
  outline: none;
  cursor: text;
  touch-action: none;
}
.hex-canvas:focus-visible {
  outline: 1px solid var(--accent);
  outline-offset: -1px;
}
</style>
