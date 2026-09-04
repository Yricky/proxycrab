import { reactive } from "vue";
import { expect, test } from "vitest";

import {
  columnGroupKey,
  copyFilterColumn,
  exactColumnRegex,
  filterAndSortGroups,
  newestId,
  oldestId,
} from "./column-groups.ts";

test("column keys distinguish built-ins and scripts", () => {
  expect(columnGroupKey({ kind: "uri" })).toBe("uri");
  expect(columnGroupKey({ kind: "script", script_name: "host" })).toBe(
    "script:host",
  );
});

test("pagination cursors do not depend on response order", () => {
  expect(newestId([9, 12, 4, 11])).toBe(12);
  expect(oldestId([9, 12, 4, 11])).toBe(4);
  expect(newestId([])).toBeUndefined();
  expect(oldestId([])).toBeUndefined();
});

test("filter columns can be copied from Vue reactive state", () => {
  const state = reactive({
    column: { kind: "script" as const, script_name: "host" },
  });
  expect(copyFilterColumn(state.column)).toEqual({
    kind: "script",
    script_name: "host",
  });
});

test("exact regex escapes metacharacters and handles empty values", () => {
  expect(exactColumnRegex("a.b+[x]")).toBe(String.raw`^a\.b\+\[x\]$`);
  expect(exactColumnRegex("")).toBe("^$");
});

test("groups filter case-insensitively and sort by count", () => {
  const result = filterAndSortGroups(
    new Map([
      ["GET", 2],
      ["post", 4],
      ["PATCH", 1],
    ]),
    3,
    2,
    "t",
    "count",
    true,
  );
  expect(result.map(({ label, count }) => [label, count])).toEqual([
    ["post", 4],
    ["GET", 2],
    ["PATCH", 1],
  ]);
});

test("groups include special rows and sort lexically in either direction", () => {
  const ascending = filterAndSortGroups(
    new Map([
      ["b", 1],
      ["a", 1],
    ]),
    1,
    1,
    "",
    "value",
    false,
  );
  expect(
    ascending
      .filter((entry) => entry.kind === "value")
      .map((entry) => entry.label),
  ).toEqual(["a", "b"]);
  expect(
    ascending.find((entry) => entry.kind === "empty")?.exactPattern,
  ).toBe("^$");
  expect(
    ascending.find((entry) => entry.kind === "error")?.exactPattern,
  ).toBeNull();

  const descending = filterAndSortGroups(
    new Map([
      ["b", 1],
      ["a", 1],
    ]),
    0,
    0,
    "",
    "value",
    true,
  );
  expect(descending.map((entry) => entry.label)).toEqual(["b", "a"]);
});
