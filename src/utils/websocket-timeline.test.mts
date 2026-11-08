import { describe, expect, test } from "vitest";
import {
  drainWebSocketPages,
  messageColors,
  messageStatusLabel,
  findRelatedMessage,
  messageBar,
  messageNeighbors,
  messageAtPosition,
  zoomTimeline,
  timelineTicks,
  timelineMetrics,
  unseenMessageIds,
} from "./websocket-timeline.ts";
import type { WebSocketMessage, WebSocketPage } from "../api/types.ts";

const message: WebSocketMessage = {
  id: 1,
  type: "text",
  started_at: 1100,
  completed_at: 1250,
  size: 5,
  content_available: true,
  omitted_reason: null,
};
describe("WebSocket shared time axis", () => {
  test("positions both directions against the same origin and scales duration", () => {
    expect(messageBar(message, 1000, 2)).toEqual({ left: 200, width: 300 });
    expect(messageBar({ ...message, started_at: 1200 }, 1000, 2)).toEqual({
      left: 400,
      width: 100,
    });
  });
  test("instant messages remain clickable and timestamps before origin are clamped", () => {
    expect(messageBar({ ...message, started_at: 950, completed_at: 950 }, 1000, 1)).toEqual({
      left: 0,
      width: 8,
    });
  });
});
describe("incremental message pagination", () => {
  test("drains every page and starts at the committed per-side cursor", async () => {
    const cursors: number[] = [];
    const committed: number[] = [];
    await drainWebSocketPages(
      2,
      async (after) => {
        cursors.push(after);
        return { messages: [], next_after_id: after + 1, has_more: after < 4 };
      },
      (page) => committed.push(page.next_after_id),
      () => true,
    );
    expect(cursors).toEqual([2, 3, 4]);
    expect(committed).toEqual([3, 4, 5]);
  });
  test("rejects a non-progressing cursor rather than looping forever", async () => {
    await expect(
      drainWebSocketPages(
        2,
        async () => ({ messages: [], next_after_id: 2, has_more: true }),
        () => {},
        () => true,
      ),
    ).rejects.toThrow("游标未前进");
  });
  test("discards in-flight pages after invalidation", async () => {
    let valid = true;
    const committed: WebSocketPage[] = [];
    await drainWebSocketPages(
      0,
      async () => {
        valid = false;
        return { messages: [message], next_after_id: 1, has_more: false };
      },
      (page) => committed.push(page),
      () => valid,
    );
    expect(committed).toEqual([]);
  });
});
describe("overlapping message navigation", () => {
  test("visits every same-millisecond message in both directions, without contiguous IDs", () => {
    const overlapping = [2, 5, 9].map((id) => ({
      ...message,
      id,
      completed_at: message.started_at,
    }));
    expect(overlapping.map((item) => messageBar(item, 1000, 64))).toEqual([
      { left: 6400, width: 8 },
      { left: 6400, width: 8 },
      { left: 6400, width: 8 },
    ]);
    const backward: number[] = [];
    let current: WebSocketMessage | undefined = overlapping[2];
    while (current) {
      backward.push(current.id);
      current = messageNeighbors(overlapping, current.id).previous;
    }
    expect(backward).toEqual([9, 5, 2]);
    const forward: number[] = [];
    current = overlapping[0];
    while (current) {
      forward.push(current.id);
      current = messageNeighbors(overlapping, current.id).next;
    }
    expect(forward).toEqual([2, 5, 9]);
  });
  test("an invalidated selection cannot accidentally select the first message", () => {
    expect(messageNeighbors([message], 99)).toEqual({
      index: -1,
      previous: undefined,
      next: undefined,
    });
    expect(messageNeighbors([], 1)).toEqual({ index: -1, previous: undefined, next: undefined });
  });
});

describe("canvas message hit testing", () => {
  test("uses scrolled timeline CSS coordinates and separates lanes from empty space", () => {
    const lanes = { request: [message], response: [{ ...message, id: 2 }] };
    expect(messageAtPosition(lanes, 205, 30, 1000, 2)?.side).toBe("request");
    expect(messageAtPosition(lanes, 205, 65, 1000, 2)?.message.id).toBe(2);
    expect(messageAtPosition(lanes, 205, 52, 1000, 2)).toBeNull();
    expect(messageAtPosition(lanes, 205, 10, 1000, 2)).toBeNull();
    // Pointer x=5 after scrolling 200px hits the same location regardless of DPR.
    expect(messageAtPosition(lanes, 5 + 200, 30, 1000, 2)?.message.id).toBe(1);
    expect(messageAtPosition(lanes, 501, 30, 1000, 2)).toBeNull();
  });
  test("hits the topmost overlap, while selected messages retain priority", () => {
    const first = { ...message, completed_at: message.started_at };
    const second = { ...first, id: 3 };
    const lanes = { request: [first, second], response: [] };
    expect(messageAtPosition(lanes, 107, 30, 1000, 1)?.message.id).toBe(3);
    expect(
      messageAtPosition(lanes, 107, 30, 1000, 1, { side: "request", message: first })?.message.id,
    ).toBe(1);
    expect(messageAtPosition(lanes, 109, 30, 1000, 1)).toBeNull();
  });
});

describe("wheel zoom", () => {
  test("keeps the time under the pointer fixed and clamps zoom bounds", () => {
    const next = zoomTimeline(2, 300, 120, -200);
    expect((next.scrollLeft + 120) / next.zoom).toBeCloseTo((300 + 120) / 2);
    expect(next.zoom).toBeGreaterThan(2);
    expect(zoomTimeline(1, 0, 100, 1000)).toEqual({ zoom: 1, scrollLeft: 0 });
    expect(zoomTimeline(64, 500, 100, -1000)).toEqual({ zoom: 64, scrollLeft: 500 });
  });
});

describe("automatic fit and unread message pills", () => {
  test("all-data mode remains unscrollable when new messages extend the time range", () => {
    const before = timelineMetrics(620, 1000, null);
    const after = timelineMetrics(620, 4000, null);
    expect(before.width).toBe(600);
    expect(after.width).toBe(600);
    expect(after.fitted).toBe(true);
    expect(after.scale).toBe(0.15);
  });
  test("inspecting history preserves pixels per millisecond as new messages arrive", () => {
    const before = timelineMetrics(620, 1000, 2);
    const after = timelineMetrics(620, 4000, before.scale);
    expect(after.scale).toBe(2);
    expect(after.width).toBe(8000);
    expect(after.fitted).toBe(false);
    expect(messageBar(message, 1000, before.scale)).toEqual(messageBar(message, 1000, after.scale));
  });
  test("clears fully visible unread messages, retaining offscreen and partially visible messages", () => {
    const messages = [
      message,
      { ...message, id: 2, started_at: 1400, completed_at: 1700 },
      { ...message, id: 3, started_at: 1800, completed_at: 1900 },
    ];
    expect([...unseenMessageIds(new Set([1, 2, 3]), messages, 1000, 1, 0, 600)]).toEqual([2, 3]);
    expect([...unseenMessageIds(new Set([2, 3]), messages, 1000, 1, 400, 600)]).toEqual([]);
  });
});

test("maximum zoom is absolute: 100px per second at 1x, independent of capture duration", () => {
  expect(timelineMetrics(620, 6000, 0.1).zoom).toBe(1);
  for (const duration of [1, 1000, 1000000]) {
    const metrics = timelineMetrics(620, duration, 100);
    expect(metrics.scale).toBe(6.4);
    expect(metrics.zoom).toBe(64);
    expect(100 / metrics.scale).toBe(15.625);
  }
  expect(timelineMetrics(620, 1, null).fitted).toBe(true);
});

describe("adaptive timeline ticks", () => {
  test.each([0.0001, 0.1, 1, 6.4])(
    "spaces visible ticks at scale %s with exact time positions",
    (scale) => {
      const left = 135;
      const ticks = timelineTicks(scale, left, 640);
      expect(ticks.length).toBeGreaterThanOrEqual(3);
      expect(ticks.length).toBeLessThanOrEqual(8);
      for (let i = 0; i < ticks.length; i++) {
        expect(ticks[i]!.x).toBeGreaterThanOrEqual(0);
        expect(ticks[i]!.x).toBeLessThanOrEqual(640);
        expect(ticks[i]!.x).toBeCloseTo(ticks[i]!.time * scale - left);
        if (i) expect(ticks[i]!.x - ticks[i - 1]!.x).toBeGreaterThanOrEqual(89);
      }
    },
  );
  test("ticks preserve their absolute time grid when scrolling", () => {
    const before = timelineTicks(6.4, 0, 640);
    const after = timelineTicks(6.4, 30, 640);
    const common = after.find((t) => t.time === before[1]!.time)!;
    expect(common.x).toBeCloseTo(before[1]!.x - 30);
  });
});

describe("message origin and disposition", () => {
  const producer = { execution_id: 7, name: "reply", invocation_id: 3, send_index: 1 };
  test("origin defines fill and disposition overrides only border", () => {
    for (const origin of [undefined, producer]) {
      const fill = origin ? "--accent" : "--success";
      expect(messageColors({ ...message, producer: origin })).toEqual({ border: fill, fill });
      expect(messageColors({ ...message, producer: origin, status: "sent" })).toEqual({
        border: fill,
        fill,
      });
      expect(messageColors({ ...message, producer: origin, status: "dropped" })).toEqual({
        border: "--text-faint",
        fill,
      });
      for (const status of ["unsent", "failed"] as const) {
        expect(messageColors({ ...message, producer: origin, status })).toEqual({
          border: "--danger",
          fill,
        });
      }
    }
  });
  test("legacy capture does not claim confirmed delivery", () => {
    expect(messageStatusLabel(message)).toBe("已采集");
    expect(messageStatusLabel({ ...message, status: "sent" })).toBe("已发送");
    expect(messageStatusLabel({ ...message, status: "dropped" })).toBe("已丢弃");
    expect(messageStatusLabel({ ...message, status: "unsent" })).toBe("未发送");
    expect(messageStatusLabel({ ...message, status: "failed" })).toBe("发送失败");
  });
  test("provenance navigation distinguishes equal IDs in opposite directions", () => {
    const response = { ...message, type: "binary" as const };
    const messages = { request: [message], response: [response] };
    expect(findRelatedMessage(messages, { side: "response", id: 1 })).toEqual({
      side: "response",
      message: response,
    });
    expect(findRelatedMessage(messages, { side: "request", id: 1 })).toEqual({
      side: "request",
      message,
    });
    expect(findRelatedMessage(messages, { side: "response", id: 2 })).toBeNull();
  });
});
