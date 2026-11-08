import type { WebSocketMessage, WebSocketPage, WebSocketMessageRef } from "../api/types";
import { t } from "../i18n";

export function messageBar(message: WebSocketMessage, origin: number, scale: number) {
  return {
    left: Math.max(0, message.started_at - origin) * scale,
    width: Math.max(8, (message.completed_at - message.started_at) * scale),
  };
}

/** Commit each page before yielding, so a long capture remains interactive. */
export async function drainWebSocketPages(
  after: number,
  fetchPage: (after: number) => Promise<WebSocketPage>,
  commit: (page: WebSocketPage) => void,
  current: () => boolean,
): Promise<void> {
  while (current()) {
    const page = await fetchPage(after);
    if (!current()) return;
    if (page.has_more && page.next_after_id <= after) throw new Error(t("消息分页游标未前进"));
    commit(page);
    if (!page.has_more) return;
    after = page.next_after_id;
    await new Promise<void>((resolve) => setTimeout(resolve, 0));
  }
}

/** Navigate by capture order, including messages with identical timestamps or gaps in IDs. */
export function messageNeighbors(messages: WebSocketMessage[], selectedId: number) {
  const index = messages.findIndex((message) => message.id === selectedId);
  return {
    index,
    previous: index > 0 ? messages[index - 1] : undefined,
    next: index >= 0 ? messages[index + 1] : undefined,
  };
}

export function messageLaneTop(side: "request" | "response") {
  return side === "request" ? 27 : 59;
}

/** Coordinates are CSS pixels in the scrolled timeline, not backing-store pixels. */
export function messageAtPosition(
  messages: Record<"request" | "response", WebSocketMessage[]>,
  x: number,
  y: number,
  origin: number,
  scale: number,
  selected: { side: "request" | "response"; message: WebSocketMessage } | null = null,
) {
  const side: "request" | "response" | null =
    y >= 27 && y <= 45 ? "request" : y >= 59 && y <= 77 ? "response" : null;
  if (!side) return null;
  const contains = (message: WebSocketMessage) => {
    const bar = messageBar(message, origin, scale);
    return x >= bar.left && x <= bar.left + bar.width;
  };
  if (selected?.side === side && contains(selected.message)) return selected;
  for (let i = messages[side].length - 1; i >= 0; i--) {
    const message = messages[side][i]!;
    if (contains(message)) return { side, message };
  }
  return null;
}

/** Anchor the time under the pointer while changing the horizontal scale. */
export function zoomTimeline(
  zoom: number,
  scrollLeft: number,
  pointerX: number,
  deltaPixels: number,
  maxZoom = 64,
  minZoom = 1,
) {
  const next = Math.min(maxZoom, Math.max(minZoom, zoom * Math.exp(-deltaPixels * 0.002)));
  return {
    zoom: next,
    scrollLeft: Math.max(0, ((scrollLeft + pointerX) * next) / zoom - pointerX),
  };
}
/** Nice 1/2/5 time steps keep tick spacing stable across zoom and incoming data. */
export function timelineTicks(scale: number, left: number, viewportWidth: number) {
  if (!(scale > 0) || viewportWidth <= 0) return [];
  const maxSeconds = Math.max(0, (left + viewportWidth) / scale / 1000);
  const labelWidth = `${maxSeconds.toFixed(3)} s`.length * 6 + 16;
  const desired = Math.max(90, labelWidth) / scale;
  const base = 10 ** Math.floor(Math.log10(desired));
  const step = [1, 2, 5, 10].map((m) => m * base).find((s) => s >= desired)!;
  const first = Math.ceil(left / scale / step);
  const last = Math.floor((left + viewportWidth) / scale / step);
  return Array.from({ length: Math.max(0, last - first + 1) }, (_, index) => {
    const time = (first + index) * step;
    return { time, x: time * scale - left };
  });
}

export const TIMELINE_BASE_SCALE = 100 / 1000;
export const TIMELINE_MAX_ZOOM = 64;

export function timelineMetrics(
  viewportWidth: number,
  durationMs: number,
  fixedScale: number | null,
) {
  const duration = Math.max(1, durationMs);
  const available = Math.max(1, viewportWidth - 20);
  const maximum = TIMELINE_BASE_SCALE * TIMELINE_MAX_ZOOM;
  const fitScale = Math.min(available / duration, maximum);
  const scale = Math.min(maximum, Math.max(fitScale, fixedScale ?? 0));
  const width = scale * duration;
  return {
    fitScale,
    scale,
    width,
    fitted: width <= available + 0.5,
    zoom: scale / TIMELINE_BASE_SCALE,
  };
}
export function unseenMessageIds(
  ids: Set<number>,
  messages: WebSocketMessage[],
  origin: number,
  scale: number,
  left: number,
  viewportWidth: number,
) {
  const remaining = new Set(ids);
  for (const message of messages) {
    if (!remaining.has(message.id)) continue;
    const bar = messageBar(message, origin, scale);
    if (bar.left >= left && bar.left + bar.width <= left + viewportWidth)
      remaining.delete(message.id);
  }
  return remaining;
}

/** Origin fills the bar; an exceptional disposition turns the outline into the status color. */
export function messageColors(message: WebSocketMessage) {
  const fill = message.producer ? "--accent" : "--success";
  const border =
    message.status === "dropped"
      ? "--text-faint"
      : message.status === "failed" || message.status === "unsent"
        ? "--danger"
        : fill;
  return { border, fill };
}

export function messageStatusLabel(message: WebSocketMessage) {
  switch (message.status) {
    case "sent":
      return t("已发送");
    case "dropped":
      return t("已丢弃");
    case "unsent":
      return t("未发送");
    case "failed":
      return t("发送失败");
    default:
      return t("已采集");
  }
}

export function findRelatedMessage(
  messages: Record<"request" | "response", WebSocketMessage[]>,
  reference: WebSocketMessageRef,
) {
  const message = messages[reference.side].find((item) => item.id === reference.id);
  return message ? { side: reference.side, message } : null;
}
