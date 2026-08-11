import assert from "node:assert/strict";
import test from "node:test";
import { reactive } from "vue";

import {
  columnGroupKey,
  copyFilterColumn,
  exactColumnRegex,
  filterAndSortGroups,
  newestId,
  oldestId,
} from "./column-groups.ts";

test("column keys distinguish built-ins and scripts", () => {
  assert.equal(columnGroupKey({ kind: "uri" }), "uri");
  assert.equal(
    columnGroupKey({ kind: "script", script_name: "host" }),
    "script:host",
  );
});

test("pagination cursors do not depend on response order", () => {
  assert.equal(newestId([9, 12, 4, 11]), 12);
  assert.equal(oldestId([9, 12, 4, 11]), 4);
  assert.equal(newestId([]), undefined);
  assert.equal(oldestId([]), undefined);
});

test("filter columns can be copied from Vue reactive state", () => {
  const state = reactive({
    column: { kind: "script" as const, script_name: "host" },
  });
  assert.deepEqual(copyFilterColumn(state.column), {
    kind: "script",
    script_name: "host",
  });
});

test("exact regex escapes metacharacters and handles empty values", () => {
  assert.equal(exactColumnRegex("a.b+[x]"), String.raw`^a\.b\+\[x\]$`);
  assert.equal(exactColumnRegex(""), "^$");
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
  assert.deepEqual(
    result.map(({ label, count }) => [label, count]),
    [
      ["post", 4],
      ["GET", 2],
      ["PATCH", 1],
    ],
  );
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
  assert.deepEqual(
    ascending.filter((entry) => entry.kind === "value").map((entry) => entry.label),
    ["a", "b"],
  );
  assert.equal(ascending.find((entry) => entry.kind === "empty")?.exactPattern, "^$");
  assert.equal(ascending.find((entry) => entry.kind === "error")?.exactPattern, null);

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
  assert.deepEqual(descending.map((entry) => entry.label), ["b", "a"]);
});
