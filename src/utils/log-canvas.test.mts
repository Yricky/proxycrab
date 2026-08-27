import assert from "node:assert/strict";
import test from "node:test";

import {
  anchoredScrollTop,
  clamp,
  fullyVisibleRowRange,
  reconcileUnseenIds,
} from "./log-canvas.ts";

test("clamp keeps scroll offsets inside their range", () => {
  assert.equal(clamp(-4, 0, 100), 0);
  assert.equal(clamp(40, 0, 100), 40);
  assert.equal(clamp(140, 0, 100), 100);
});

test("fully visible rows exclude partial rows at both edges", () => {
  assert.deepEqual(fullyVisibleRowRange(0, 52, 26, 10), { start: 0, end: 2 });
  assert.deepEqual(fullyVisibleRowRange(1, 52, 26, 10), { start: 1, end: 2 });
  assert.deepEqual(fullyVisibleRowRange(26, 51, 26, 10), { start: 1, end: 2 });
  assert.deepEqual(fullyVisibleRowRange(250, 52, 26, 10), { start: 10, end: 10 });
});

test("anchored scrolling keeps the same row at the same viewport position", () => {
  assert.equal(anchoredScrollTop(260, 10, 13, 26, 1_000), 338);
  assert.equal(anchoredScrollTop(20, 4, 1, 26, 1_000), 0);
  assert.equal(anchoredScrollTop(980, 10, 20, 26, 1_000), 1_000);
});

test("unseen IDs retain additions, discard removals, and count re-entry", () => {
  const first = reconcileUnseenIds(new Set([5]), new Set([5, 4]), [6, 5]);
  assert.deepEqual([...first], [5, 6]);

  const removed = reconcileUnseenIds(first, new Set([6, 5]), [6]);
  assert.deepEqual([...removed], [6]);

  const reentered = reconcileUnseenIds(removed, new Set([6]), [6, 5]);
  assert.deepEqual([...reentered], [6, 5]);
});
