import { expect, test } from "vitest";

import {
  anchoredScrollTop,
  clamp,
  fullyVisibleRowRange,
  reconcileUnseenIds,
} from "./log-canvas.ts";

test("clamp keeps scroll offsets inside their range", () => {
  expect(clamp(-4, 0, 100)).toBe(0);
  expect(clamp(40, 0, 100)).toBe(40);
  expect(clamp(140, 0, 100)).toBe(100);
});

test("fully visible rows exclude partial rows at both edges", () => {
  expect(fullyVisibleRowRange(0, 52, 26, 10)).toEqual({ start: 0, end: 2 });
  expect(fullyVisibleRowRange(1, 52, 26, 10)).toEqual({ start: 1, end: 2 });
  expect(fullyVisibleRowRange(26, 51, 26, 10)).toEqual({ start: 1, end: 2 });
  expect(fullyVisibleRowRange(250, 52, 26, 10)).toEqual({
    start: 10,
    end: 10,
  });
});

test("anchored scrolling keeps the same row at the same viewport position", () => {
  expect(anchoredScrollTop(260, 10, 13, 26, 1_000)).toBe(338);
  expect(anchoredScrollTop(20, 4, 1, 26, 1_000)).toBe(0);
  expect(anchoredScrollTop(980, 10, 20, 26, 1_000)).toBe(1_000);
});

test("unseen IDs retain additions, discard removals, and count re-entry", () => {
  const first = reconcileUnseenIds(new Set([5]), new Set([5, 4]), [6, 5]);
  expect([...first]).toEqual([5, 6]);

  const removed = reconcileUnseenIds(first, new Set([6, 5]), [6]);
  expect([...removed]).toEqual([6]);

  const reentered = reconcileUnseenIds(removed, new Set([6]), [6, 5]);
  expect([...reentered]).toEqual([6, 5]);
});
