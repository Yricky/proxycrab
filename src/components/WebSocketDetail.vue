<script setup lang="ts">
import { saveBytesFile } from "../api/save-file";
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef, watch } from "vue";
import {
  Io5ChevronBack,
  Io5ChevronForward,
  Io5Copy,
  Io5DownloadOutline,
  Io5CodeSlash,
} from "vue-icons-plus/io5";
import { useBackend } from "../api";
import type { LogDetail } from "../api/generated/LogDetail";
import type { WsInterceptor } from "../api/generated/WsInterceptor";
import type { WebSocketMessage, WebSocketMessageRef } from "../api/types";
import type { BodySide, LoadedBody } from "../api/body";
import { appStore, reportError } from "../stores/app";
import { t } from "../i18n";
import { openReadonlyViewer } from "../windows/launcher";
import { copyText } from "../utils/clipboard";
import { formatHex } from "../utils/hex-viewer";
import { useHexViewerMenu } from "../utils/hex-viewer-menu";
import HexViewer from "./HexViewer.vue";
import MonacoEditor from "./MonacoEditor.vue";
import {
  drainWebSocketPages,
  messageColors,
  messageStatusLabel,
  findRelatedMessage,
  messageBar,
  messageNeighbors,
  messageAtPosition,
  messageLaneTop,
  timelineTicks,
  zoomTimeline,
  timelineMetrics,
  unseenMessageIds,
  TIMELINE_BASE_SCALE,
  TIMELINE_MAX_ZOOM,
} from "../utils/websocket-timeline";

const props = defineProps<{ detail: LogDetail; active: boolean }>();
const backend = useBackend();
const {
  selection: hexSelection,
  rightPane,
  contextMenu: hexContextMenu,
  copied: hexCopied,
} = useHexViewerMenu();
const sides: BodySide[] = ["request", "response"];
const messages = shallowRef<Record<BodySide, WebSocketMessage[]>>({ request: [], response: [] });
const cursors: Record<BodySide, number> = { request: 0, response: 0 };
const selected = ref<{ side: BodySide; message: WebSocketMessage } | null>(null);
const neighbors = computed(() =>
  selected.value
    ? messageNeighbors(messages.value[selected.value.side], selected.value.message.id)
    : null,
);
const body = shallowRef<LoadedBody | null>(null);
const bodyLoading = ref(false);
const bodyError = ref("");
const listError = ref("");
const refreshing = ref(false);
const formatted = ref(true);
// null means fit all data; a fixed scale preserves the inspected time window.
const fixedScale = ref<number | null>(null);
const unseen = shallowRef<Record<BodySide, Set<number>>>({
  request: new Set(),
  response: new Set(),
});
const initialized: Record<BodySide, boolean> = { request: false, response: false };
let pillRects: Partial<Record<BodySide, { x: number; y: number; width: number; height: number }>> =
  {};
const viewport = ref<HTMLElement>();
const canvas = ref<HTMLCanvasElement>();
const hoverTitle = ref("");
const CANVAS_HEIGHT = 84;
let paintFrame = 0;
let themeObserver: MutationObserver | undefined;
const viewportWidth = ref(600);
const scrollLeft = ref(0);
let generation = 0;
let selectionGeneration = 0;
let disposed = false;
let refreshAgain = false;
let timer: number | undefined;
let observer: ResizeObserver | undefined;
const info = computed(() => props.detail.websocket);
const live = computed(
  () => props.active && props.detail.outcome === "in_progress" && !info.value?.closed_at,
);
const state = computed(() =>
  info.value?.closed_at != null
    ? t("已关闭")
    : live.value
      ? t("连接中")
      : props.detail.outcome === "in_progress"
        ? t("采集已停止")
        : t("已结束"),
);
const origin = computed(() => info.value?.opened_at ?? props.detail.created_at);
const end = computed(() => {
  let value = origin.value + 1;
  for (const side of sides)
    for (const message of messages.value[side]) value = Math.max(value, message.completed_at);
  return Math.max(value, info.value?.closed_at ?? origin.value);
});
const metrics = computed(() =>
  timelineMetrics(viewportWidth.value, end.value - origin.value, fixedScale.value),
);
const fitScale = computed(() => metrics.value.fitScale);
const scale = computed(() => metrics.value.scale);
const width = computed(() => metrics.value.width);
const fitted = computed(() => metrics.value.fitted);
const zoom = computed(() => metrics.value.zoom);
watch(
  fitScale,
  () => {
    if (fixedScale.value !== null && fixedScale.value <= fitScale.value) fixedScale.value = null;
  },
  { flush: "sync" },
);
const ticks = computed(() => timelineTicks(scale.value, scrollLeft.value, viewportWidth.value));
const visibleMessages = computed(
  () =>
    Object.fromEntries(
      sides.map((side) => [
        side,
        messages.value[side].filter((message) => {
          const bar = messageBar(message, origin.value, scale.value);
          return (
            bar.left + bar.width >= scrollLeft.value - 10 &&
            bar.left <= scrollLeft.value + viewportWidth.value
          );
        }),
      ]),
    ) as Record<BodySide, WebSocketMessage[]>,
);
const PREVIEW_LIMIT = 64 * 1024;
const preview = computed(() => {
  const bytes = body.value?.bytes.subarray(0, PREVIEW_LIMIT);
  const text = bytes ? new TextDecoder().decode(bytes) : "";
  if ((body.value?.bytes.length ?? 0) <= PREVIEW_LIMIT) {
    try {
      const json = JSON.parse(text);
      return { text: formatted.value ? JSON.stringify(json, null, 2) : text, language: "json" };
    } catch {
      /* Plain text. */
    }
  }
  return { text, language: "plaintext" };
});
const truncated = computed(() => (body.value?.bytes.length ?? 0) > PREVIEW_LIMIT);
function paint() {
  paintFrame = 0;
  const element = canvas.value;
  if (!element || !viewport.value) return;
  const ctx = element.getContext("2d");
  if (!ctx) return;
  const dpr = window.devicePixelRatio || 1;
  const w = viewportWidth.value;
  element.width = Math.round(w * dpr);
  element.height = Math.round(CANVAS_HEIGHT * dpr);
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  const style = getComputedStyle(element);
  const color = (name: string) => style.getPropertyValue(name).trim();
  ctx.fillStyle = color("--bg-panel");
  ctx.fillRect(0, 0, w, CANVAS_HEIGHT);
  ctx.strokeStyle = color("--border");
  ctx.lineWidth = 1;
  for (const y of [20, 52, 83]) {
    ctx.beginPath();
    ctx.moveTo(0, y + 0.5);
    ctx.lineTo(w, y + 0.5);
    ctx.stroke();
  }
  ctx.font = `10px ${color("--font-mono") || "monospace"}`;
  ctx.textBaseline = "middle";
  ctx.fillStyle = color("--text-faint");
  for (const tick of ticks.value) {
    const label = timeLabel(tick.time);
    const labelWidth = ctx.measureText(label).width;
    if (tick.x + labelWidth + 5 <= w) ctx.fillText(label, tick.x + 3, 9);
    ctx.strokeStyle = color("--text-faint");
    ctx.beginPath();
    ctx.moveTo(tick.x + 0.5, 16);
    ctx.lineTo(tick.x + 0.5, 20);
    ctx.stroke();
  }
  const drawMessage = (side: BodySide, message: WebSocketMessage) => {
    const bar = messageBar(message, origin.value, scale.value);
    const x = bar.left - scrollLeft.value;
    const y = messageLaneTop(side);
    const colors = messageColors(message);
    const chosen = selected.value?.side === side && selected.value.message.id === message.id;
    ctx.fillStyle = color(colors.fill);
    ctx.globalAlpha = 0.3;
    ctx.fillRect(x, y, bar.width, 18);
    ctx.globalAlpha = 1;
    ctx.strokeStyle = color(colors.border);
    ctx.lineWidth = chosen ? 2 : 1;
    ctx.strokeRect(x + 0.5, y + 0.5, Math.max(0, bar.width - 1), 17);
    ctx.save();
    ctx.beginPath();
    ctx.rect(x + 1, y, Math.max(0, bar.width - 2), 18);
    ctx.clip();
    ctx.fillStyle = color("--text");
    ctx.fillText(`${message.id}`, x + 3, y + 9);
    ctx.restore();
  };
  for (const side of sides) {
    for (const message of visibleMessages.value[side]) drawMessage(side, message);
  }
  // Keep the selected message visible over coincident messages, matching hit testing.
  if (selected.value) drawMessage(selected.value.side, selected.value.message);
  pillRects = {};
  if (!fitted.value)
    for (const side of sides) {
      const count = unseen.value[side].size;
      if (!count) continue;
      const label = `${count} →`;
      const pillWidth = ctx.measureText(label).width + 18;
      const rect = {
        x: Math.max(0, w - pillWidth - 6),
        y: messageLaneTop(side) - 1,
        width: pillWidth,
        height: 20,
      };
      pillRects[side] = rect;
      ctx.beginPath();
      ctx.roundRect(rect.x, rect.y, rect.width, rect.height, 10);
      ctx.fillStyle = color("--accent");
      ctx.fill();
      ctx.fillStyle = color("--accent-text");
      ctx.fillText(label, rect.x + 9, rect.y + 10);
    }
}
function schedulePaint() {
  if (!disposed && !paintFrame) paintFrame = window.requestAnimationFrame(paint);
}
function hitMessage(event: MouseEvent) {
  const rect = canvas.value!.getBoundingClientRect();
  return messageAtPosition(
    visibleMessages.value,
    event.clientX - rect.left + scrollLeft.value,
    event.clientY - rect.top,
    origin.value,
    scale.value,
    selected.value,
  );
}
function hitPill(event: MouseEvent): BodySide | undefined {
  const rect = canvas.value!.getBoundingClientRect();
  const x = event.clientX - rect.left,
    y = event.clientY - rect.top;
  return sides.find((side) => {
    const p = pillRects[side];
    return p && x >= p.x && x <= p.x + p.width && y >= p.y && y <= p.y + p.height;
  });
}
function revealLatest(side: BodySide) {
  const latest = messages.value[side][messages.value[side].length - 1];
  if (!latest || !viewport.value) return;
  const bar = messageBar(latest, origin.value, scale.value);
  unseen.value = { ...unseen.value, [side]: new Set<number>() };
  viewport.value.scrollLeft = Math.max(0, bar.left + bar.width - viewportWidth.value + 20);
  onScroll();
}
function clickCanvas(event: MouseEvent) {
  const side = hitPill(event);
  if (side) {
    revealLatest(side);
    return;
  }
  const hit = hitMessage(event);
  if (hit) {
    void select(hit.side, hit.message);
  }
}
function hoverCanvas(event: MouseEvent) {
  const side = hitPill(event);
  if (side) {
    hoverTitle.value = `${side === "request" ? "C→S" : "S→C"} ${t("{n} 条新消息，点击查看最新", unseen.value[side].size)}`;
    return;
  }
  const hit = hitMessage(event);
  hoverTitle.value = hit
    ? `${hit.side === "request" ? "C→S" : "S→C"} #${hit.message.id} ${hit.message.type} · ${hit.message.size} B · ${hit.message.producer ? t("拦截器发送") : t("原始消息")} · ${messageStatusLabel(hit.message)} · ${timeLabel(hit.message.started_at - origin.value)} → ${timeLabel(hit.message.completed_at - origin.value)}`
    : "";
}
async function wheelCanvas(event: WheelEvent) {
  if (!event.ctrlKey && !event.metaKey) return;
  event.preventDefault();
  event.stopPropagation();
  const x = event.clientX - canvas.value!.getBoundingClientRect().left;
  const delta = event.deltaY || event.deltaX;
  const pixels =
    delta * (event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? viewportWidth.value : 1);
  const next = zoomTimeline(
    zoom.value,
    scrollLeft.value,
    x,
    pixels,
    TIMELINE_MAX_ZOOM,
    fitScale.value / TIMELINE_BASE_SCALE,
  );
  const nextScale = next.zoom * TIMELINE_BASE_SCALE;
  fixedScale.value = nextScale <= fitScale.value ? null : nextScale;
  hoverTitle.value = "";
  await nextTick();
  if (viewport.value) {
    viewport.value.scrollLeft = next.scrollLeft;
    onScroll();
  }
}
function keyCanvas(event: KeyboardEvent) {
  if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)) return;
  event.preventDefault();
  if (event.key === "ArrowUp" || event.key === "ArrowDown" || !selected.value) {
    const side = event.key === "ArrowDown" ? "response" : "request";
    const message = messages.value[side][0];
    if (message) void select(side, message);
  } else
    navigateMessage(event.key === "ArrowLeft" ? neighbors.value?.previous : neighbors.value?.next);
}
watch([visibleMessages, ticks, selected, viewportWidth, scrollLeft], schedulePaint, {
  flush: "post",
});
function timeLabel(time: number) {
  return `${(time / 1000).toFixed(3)} s`;
}
function markVisible() {
  const next = { ...unseen.value };
  let changed = false;
  for (const side of sides) {
    const remaining = fitted.value
      ? new Set<number>()
      : unseenMessageIds(
          next[side],
          messages.value[side],
          origin.value,
          scale.value,
          scrollLeft.value,
          viewportWidth.value,
        );
    if (remaining.size !== next[side].size) {
      next[side] = remaining;
      changed = true;
    }
  }
  if (changed) unseen.value = next;
}
function onScroll() {
  scrollLeft.value = viewport.value?.scrollLeft ?? 0;
  hoverTitle.value = "";
  markVisible();
}
watch(
  [scale, viewportWidth, messages],
  async () => {
    await nextTick();
    if (fitted.value) {
      fixedScale.value = null;
      if (viewport.value) viewport.value.scrollLeft = 0;
    }
    onScroll();
  },
  { flush: "post" },
);
watch(unseen, schedulePaint);
async function refresh() {
  if (refreshing.value) {
    refreshAgain = true;
    return;
  }
  if (!backend.getWebSocketMessages || disposed) return;
  refreshing.value = true;
  const token = generation;
  const { session_id, id } = props.detail;
  listError.value = "";
  try {
    const results = await Promise.allSettled(
      sides.map((side) =>
        drainWebSocketPages(
          cursors[side],
          (after) => backend.getWebSocketMessages!(session_id, id, side, after, 200),
          (page) => {
            if (initialized[side] && !fitted.value && page.messages.length) {
              unseen.value = {
                ...unseen.value,
                [side]: new Set([...unseen.value[side], ...page.messages.map((m) => m.id)]),
              };
            }
            messages.value = {
              ...messages.value,
              [side]: [...messages.value[side], ...page.messages],
            };
            cursors[side] = page.next_after_id;
            if (!page.has_more) initialized[side] = true;
          },
          () => !disposed && token === generation,
        ),
      ),
    );
    const failure = results.find((result) => result.status === "rejected");
    if (failure?.status === "rejected") throw failure.reason;
  } catch (error) {
    if (!disposed && token === generation) listError.value = String(error);
  } finally {
    refreshing.value = false;
    if (refreshAgain && !disposed) {
      refreshAgain = false;
      void refresh();
    }
  }
}
async function select(side: BodySide, message: WebSocketMessage) {
  selected.value = { side, message };
  body.value = null;
  bodyError.value = "";
  const token = ++selectionGeneration;
  bodyLoading.value = false;
  if (!message.content_available || !backend.fetchWebSocketMessageBody) return;
  bodyLoading.value = true;
  try {
    const result = await backend.fetchWebSocketMessageBody(
      props.detail.session_id,
      props.detail.id,
      side,
      message.id,
    );
    if (token === selectionGeneration && !disposed) body.value = result;
  } catch (error) {
    if (token === selectionGeneration && !disposed) bodyError.value = String(error);
  } finally {
    if (token === selectionGeneration) bodyLoading.value = false;
  }
}
function navigateMessage(message: WebSocketMessage | undefined, side = selected.value?.side) {
  if (!side || !message) return;
  void select(side, message);
  if (viewport.value) {
    const bar = messageBar(message, origin.value, scale.value);
    if (
      bar.left < viewport.value.scrollLeft ||
      bar.left + bar.width > viewport.value.scrollLeft + viewportWidth.value
    ) {
      viewport.value.scrollLeft = Math.max(0, bar.left - viewportWidth.value / 2);
      onScroll();
    }
  }
}
function referenceLabel(reference: WebSocketMessageRef) {
  return `${reference.side === "request" ? "C→S" : "S→C"} #${reference.id}`;
}
function navigateRelated(reference: WebSocketMessageRef) {
  const related = findRelatedMessage(messages.value, reference);
  if (!related) return;
  navigateMessage(related.message, related.side);
}
async function openSource(interceptor: WsInterceptor) {
  if (!info.value || !backend.getInterceptorContent) return;
  const { session_id } = props.detail;
  const id = info.value.handshake_log_id;
  try {
    const { content } = await backend.getInterceptorContent(
      session_id,
      id,
      interceptor.execution_id,
    );
    openReadonlyViewer(
      `script-snapshot-${session_id}-${id}-${interceptor.execution_id}`,
      t("{name} — WS 来源脚本", { name: interceptor.name }),
      content,
      "lua",
    );
  } catch (error) {
    reportError(error, t("加载 WS 来源脚本失败"));
  }
}
async function copy() {
  if (!body.value) return;
  try {
    await copyText(
      selected.value?.message.type === "binary"
        ? formatHex(body.value.bytes)
        : new TextDecoder().decode(body.value.bytes),
    );
    appStore.toast(t("消息已复制"), "success");
  } catch (error) {
    reportError(error, t("复制消息失败"));
  }
}
async function download() {
  if (!body.value || !selected.value) return;
  try {
    await saveBytesFile(
      backend,
      `websocket-${props.detail.id}-${selected.value.side}-${selected.value.message.id}.${selected.value.message.type === "text" ? "txt" : "bin"}`,
      new Uint8Array(body.value.bytes),
    );
  } catch (error) {
    reportError(error, t("下载消息失败"));
  }
}
function redactionChanged() {
  generation += 1;
  selectionGeneration += 1;
  body.value = null;
  selected.value = null;
  messages.value = { request: [], response: [] };
  cursors.request = cursors.response = 0;
  unseen.value = { request: new Set(), response: new Set() };
  initialized.request = initialized.response = false;
  void refresh();
}
watch(
  [() => props.active, () => info.value?.closed_at, () => props.detail.outcome],
  () => void refresh(),
);
onMounted(() => {
  observer = new ResizeObserver(() => {
    viewportWidth.value = viewport.value?.clientWidth ?? 600;
    schedulePaint();
  });
  if (viewport.value) observer.observe(viewport.value);
  themeObserver = new MutationObserver(schedulePaint);
  themeObserver.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["data-theme", "style", "class"],
  });
  window.addEventListener("resize", schedulePaint);
  schedulePaint();
  window.addEventListener("proxycrab-redaction-changed", redactionChanged);
  void refresh();
  timer = window.setInterval(() => {
    if (live.value) void refresh();
  }, 2000);
});
onBeforeUnmount(() => {
  disposed = true;
  generation += 1;
  selectionGeneration += 1;
  observer?.disconnect();
  themeObserver?.disconnect();
  window.cancelAnimationFrame(paintFrame);
  window.removeEventListener("resize", schedulePaint);
  window.clearInterval(timer);
  window.removeEventListener("proxycrab-redaction-changed", redactionChanged);
});
</script>

<template>
  <section class="ws-detail">
    <div class="ws-toolbar ws-connection-toolbar">
      <div class="connection-info">
        <strong>WebSocket</strong><span :class="{ live }">{{ state }}</span>
        <span>{{ t("子协议：{value}", { value: info?.subprotocol || t("无") }) }}</span
        ><span>{{ t("扩展：{value}", { value: info?.extensions || t("无") }) }}</span>
        <span v-if="info?.closed_at != null"
          >关闭：{{ new Date(info.closed_at).toLocaleString() }} · {{ info.close_code ?? "—" }}
          {{ info.close_reason }}</span
        >
      </div>
      <span v-if="refreshing">{{ t("加载中…") }}</span>
    </div>
    <div v-if="info?.interceptors?.length && backend.getInterceptorContent" class="ws-toolbar">
      <span>{{ t("连接脚本快照") }}</span>
      <button
        v-for="interceptor in info.interceptors"
        :key="interceptor.execution_id"
        class="btn source-link"
        @click="openSource(interceptor)"
      >
        {{ interceptor.name }} #{{ interceptor.execution_id }}
      </button>
    </div>
    <div class="ws-toolbar ws-legend" :aria-label="t('消息颜色图例')">
      <span class="original">{{ t("原始消息") }}</span
      ><span class="generated">{{ t("拦截器发送") }}</span>
      <span class="dropped">{{ t("灰框：已丢弃") }}</span
      ><span class="failed">{{ t("红框：未发送／失败") }}</span>
    </div>
    <div v-if="listError" class="ws-error">{{ listError }}</div>
    <div class="waterfall">
      <div class="lane-labels">
        <div class="time-label">{{ t("时间") }}</div>
        <div v-for="side in sides" :key="side" class="lane-label">
          <span>{{ side === "request" ? "C→S" : "S→C" }}</span>
          <small :aria-label="`${side === 'request' ? 'C→S' : 'S→C'} ${t('消息数量')}`">{{
            t("{n} 条", messages[side].length)
          }}</small>
        </div>
      </div>
      <div
        ref="viewport"
        class="timeline-scroll"
        tabindex="0"
        :aria-label="t('WebSocket 消息时间轴')"
        @scroll="onScroll"
      >
        <div class="timeline" :style="{ width: `${width + 20}px` }">
          <canvas
            ref="canvas"
            class="timeline-canvas"
            :style="{
              width: `${viewportWidth}px`,
              height: `${CANVAS_HEIGHT}px`,
              cursor: hoverTitle ? 'pointer' : 'default',
            }"
            :title="hoverTitle"
            tabindex="0"
            role="img"
            :aria-label="
              t('双向消息瀑布图；Command 或 Ctrl 加滚轮缩放；上下键选择方向，左右键切换消息')
            "
            @click="clickCanvas"
            @mousemove="hoverCanvas"
            @mouseleave="hoverTitle = ''"
            @keydown="keyCanvas"
            @wheel="wheelCanvas"
          />
        </div>
      </div>
    </div>
    <div v-if="selected" class="message-detail">
      <div class="ws-toolbar">
        <strong
          >{{ selected.side === "request" ? "C→S" : "S→C" }} #{{ selected.message.id }}</strong
        >
        <button
          class="btn icon"
          :aria-label="t('上一条消息')"
          :disabled="!neighbors?.previous"
          :title="t('当前方向的上一条消息（包含重叠消息）')"
          @click="navigateMessage(neighbors?.previous)"
        >
          <Io5ChevronBack :size="13" />
        </button>
        <span>{{ (neighbors?.index ?? -1) + 1 }} / {{ messages[selected.side].length }}</span>
        <button
          class="btn icon"
          :aria-label="t('下一条消息')"
          :disabled="!neighbors?.next"
          :title="t('当前方向的下一条消息（包含重叠消息）')"
          @click="navigateMessage(neighbors?.next)"
        >
          <Io5ChevronForward :size="13" />
        </button>
        <span>{{ selected.message.type }} · {{ selected.message.size }} B</span>

        <span
          >{{ timeLabel(selected.message.started_at - origin) }} →
          {{ timeLabel(selected.message.completed_at - origin) }}</span
        >
        <span>{{
          t("耗时 {n} ms", selected.message.completed_at - selected.message.started_at)
        }}</span>
        <strong>{{ messageStatusLabel(selected.message) }}</strong>
        <span>{{ selected.message.producer ? t("拦截器发送") : t("原始消息") }}</span>
        <span v-if="selected.message.sequence != null">{{
          t("处理序号 {n}", selected.message.sequence)
        }}</span>
        <template v-for="relation in ['root', 'parent'] as const" :key="relation">
          <template v-if="selected.message[relation]">
            <span>{{ relation === "root" ? t("源头") : t("直接触发") }}</span>
            <button
              v-if="backend.getWebSocketMessages"
              class="btn source-link"
              :disabled="!findRelatedMessage(messages, selected.message[relation]!)"
              @click="navigateRelated(selected.message[relation]!)"
            >
              {{ referenceLabel(selected.message[relation]!) }}
            </button>
          </template>
        </template>
        <template v-if="selected.message.producer">
          <span>{{ t("发送者") }}</span>
          <button
            v-if="backend.getInterceptorContent"
            class="btn source-link"
            @click="openSource(selected.message.producer)"
          >
            {{ selected.message.producer.name }} #{{ selected.message.producer.execution_id }}
          </button>
          <span>{{
            t("调用 #{id} · 第 {n} 次发送", {
              id: selected.message.producer.invocation_id,
              n: selected.message.producer.send_index,
            })
          }}</span>
        </template>
        <template v-if="selected.message.dropped_by">
          <span>{{ t("丢弃者") }}</span>
          <button
            v-if="backend.getInterceptorContent"
            class="btn source-link"
            @click="openSource(selected.message.dropped_by)"
          >
            {{ selected.message.dropped_by.name }} #{{ selected.message.dropped_by.execution_id }}
          </button>
          <span>{{ t("调用 #{id}", { id: selected.message.dropped_by.invocation_id }) }}</span>
        </template>
        <button
          v-if="selected.message.type === 'text'"
          class="btn icon"
          :class="{ on: formatted }"
          :aria-pressed="formatted"
          :title="t('格式化 JSON')"
          :aria-label="t('格式化 JSON')"
          @click="formatted = !formatted"
        >
          <Io5CodeSlash :size="13" />
        </button>
        <button
          class="btn icon"
          :disabled="!body"
          :title="selected.message.type === 'binary' ? t('复制 Hex') : t('复制消息')"
          :aria-label="t('复制消息')"
          @click="copy"
        >
          <Io5Copy :size="13" />
        </button>
        <button
          class="btn icon"
          :disabled="!body"
          :title="t('下载消息')"
          :aria-label="t('下载消息')"
          @click="download"
        >
          <Io5DownloadOutline :size="13" />
        </button>
      </div>
      <div v-if="selected.message.error" class="ws-error">{{ selected.message.error }}</div>
      <div
        v-for="failure in selected.message.errors"
        :key="failure.invocation_id"
        class="ws-toolbar ws-error"
      >
        <span>{{ t("回调失败") }}</span>
        <button
          v-if="backend.getInterceptorContent"
          class="btn source-link"
          @click="openSource(failure)"
        >
          {{ failure.name }} #{{ failure.execution_id }}
        </button>
        <span>{{
          t("调用 #{id}：{error}", { id: failure.invocation_id, error: failure.error })
        }}</span>
      </div>
      <div v-if="bodyLoading" class="empty-hint">{{ t("加载消息正文…") }}</div>
      <div v-else-if="bodyError" class="ws-error">{{ bodyError }}</div>
      <div v-else-if="!selected.message.content_available" class="empty-hint">
        {{ t("正文不可用：{reason}") }}{{ selected.message.omitted_reason || t("未保留") }}
      </div>
      <HexViewer
        v-else-if="body && selected.message.type === 'binary'"
        ref="hexViewer"
        v-model:right-pane="rightPane"
        :bytes="body.bytes"
        @selection-change="hexSelection = $event"
        @contextmenu="hexContextMenu"
        @copied="hexCopied"
      />
      <template v-else-if="body">
        <div v-if="truncated" class="preview-note">
          {{ t("仅预览前 64 KiB；复制和下载包含完整消息（{n} B）。", { n: body.bytes.length }) }}
        </div>
        <div class="message-editor">
          <MonacoEditor :model-value="preview.text" :language="preview.language" readonly />
        </div>
      </template>
    </div>
    <div v-else class="empty-hint">
      {{
        messages.request.length + messages.response.length
          ? t("选择时间轴上的消息查看内容")
          : t("暂无完整消息")
      }}
    </div>
  </section>
</template>

<style scoped>
.ws-detail {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: 0;
  overflow: hidden;
  font-size: 11px;
}
.ws-toolbar {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
  padding: 0px 8px;
  border-bottom: 1px solid var(--border);
}
.source-link {
  padding: 1px 5px;
  font-size: inherit;
  color: var(--accent);
}
.ws-legend {
  gap: 12px;
}
.ws-legend .original {
  color: var(--success);
}
.ws-legend .generated {
  color: var(--accent);
}
.ws-legend .dropped {
  color: var(--text-faint);
}
.ws-legend .failed {
  color: var(--danger);
}
.ws-connection-toolbar {
  flex-wrap: nowrap;
}
.connection-info {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  overflow: hidden;
  white-space: nowrap;
}
.connection-info > * {
  flex: none;
}
.connection-info > span:last-child {
  flex: 0 1 auto;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ws-toolbar {
  color: var(--text-secondary);
}
.connection-info strong {
  color: var(--text);
}
.connection-info .live {
  color: var(--success);
}
.ws-connection-toolbar > .btn {
  flex: none;
}
.ws-toolbar .btn.icon {
  width: 22px;
  height: 22px;
  padding: 3px;
}
.ws-toolbar .btn.icon.on {
  color: var(--accent);
  background: var(--bg-selected);
}
.waterfall {
  display: flex;
  height: 85px;
  box-sizing: border-box;
  flex: none;
  border-bottom: 1px solid var(--border);
  background: var(--bg-panel);
}
.lane-labels {
  width: 48px;
  flex: none;
  border-right: 1px solid var(--border);
  display: flex;
  flex-direction: column;
}
.time-label {
  height: 20px;
  line-height: 20px;
  padding: 0 5px;
  flex: none;
}
.lane-label {
  height: 32px;
  padding: 6px 5px 0;
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  flex: none;
}
.lane-label > span {
  line-height: 14px;
}
.lane-label small {
  font-size: 9px;
  line-height: 9px;
  color: var(--text-faint);
  white-space: nowrap;
}
.timeline-scroll {
  flex: 1;
  min-width: 0;
  overflow-x: auto;
  overflow-y: hidden;
  scrollbar-width: none;
}
.timeline-scroll::-webkit-scrollbar {
  display: none;
}
.timeline {
  position: relative;
  min-width: 100%;
}
.timeline-canvas {
  display: block;
  position: sticky;
  left: 0;
}
.timeline-canvas:focus-visible {
  outline: 1px solid var(--accent);
  outline-offset: -1px;
}
.message-detail {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.message-editor {
  flex: 1;
  display: flex;
  min-width: 0;
  min-height: 0;
}
.preview-note,
.ws-error,
.empty-hint {
  padding: 10px;
  color: var(--text-faint);
}
.ws-error {
  color: var(--danger);
}
.preview-note {
  color: var(--warning);
}
</style>
