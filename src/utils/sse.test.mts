import { expect, test } from "vitest";
import { splitSseBlocks } from "./sse";

test.each(["\n", "\r\n", "\r"])("splits events at blank lines using %j", (newline) => {
  const first = ["id: same", "event: update", "data: 一", "data: 二", "", ""].join(newline);
  const second = ["id: same", "data: 三", "", ""].join(newline);
  expect(splitSseBlocks(first + second)).toEqual([
    { raw: first, complete: true },
    { raw: second, complete: true },
  ]);
});

test("preserves mixed line endings, comments, whitespace-only lines and unknown fields", () => {
  const heartbeat = ": ping\r\n\r\n";
  const event = "data: a\r \ncustom: b\r\n\n";
  expect(splitSseBlocks(heartbeat + event)).toEqual([
    { raw: heartbeat, complete: true },
    { raw: event, complete: true },
  ]);
});

test("keeps unfinished input visible until its terminating blank line arrives", () => {
  const event = "data: first\n\n";
  for (const tail of ["data: sec", "data: second\n"]) {
    expect(splitSseBlocks(event + tail)).toEqual([
      { raw: event, complete: true },
      { raw: tail, complete: false },
    ]);
  }
  expect(splitSseBlocks("data: second\n\n")).toEqual([{ raw: "data: second\n\n", complete: true }]);
});

test("ignores empty blocks and an initial BOM while retaining raw event text", () => {
  expect(splitSseBlocks("\n\r\n\r")).toEqual([]);
  expect(splitSseBlocks("\uFEFF")).toEqual([]);
  expect(splitSseBlocks("\uFEFFdata:\n\n")).toEqual([{ raw: "\uFEFFdata:\n\n", complete: true }]);
});
