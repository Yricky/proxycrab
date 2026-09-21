import { reactive } from "vue";
import { expect, test } from "vitest";

import {
  buildGroupRows,
  columnGroupKey,
  copyFilterColumn,
  exactColumnRegex,
  filterAndSortGroups,
  newestId,
  oldestId,
  prefixColumnRegex,
  splitGroupSegments,
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

test("prefix regex escapes metacharacters without an end anchor", () => {
  expect(prefixColumnRegex("/a.b/")).toBe(String.raw`^/a\.b/`);
});

test("segments keep delimiter runs as prefixes of the following segment", () => {
  expect(splitGroupSegments("http://host/path1", "/")).toEqual([
    "http:",
    "//host",
    "/path1",
  ]);
  expect(splitGroupSegments("/api/user/list", "/")).toEqual([
    "/api",
    "/user",
    "/list",
  ]);
  expect(splitGroupSegments("api/user", "/")).toEqual(["api", "/user"]);
  expect(splitGroupSegments("GET", "/")).toEqual(["GET"]);
  expect(splitGroupSegments("a::b:::c", "::")).toEqual(["a", "::b", ":::c"]);
  expect(splitGroupSegments("", "/")).toEqual([]);
  expect(splitGroupSegments("a/b", "")).toEqual(["a/b"]);
});

test("group rows stay flat without a delimiter", () => {
  const rows = buildGroupRows(
    new Map([
      ["GET", 2],
      ["POST", 1],
    ]),
    0,
    0,
    "get",
    "count",
    true,
    "",
    new Set(),
  );
  expect(rows).toHaveLength(1);
  expect(rows[0]).toMatchObject({
    kind: "value",
    depth: 0,
    label: "GET",
    pattern: "^GET$",
    hasChildren: false,
  });
});

test("tree rows aggregate counts and expand lazily", () => {
  const values = new Map([
    ["http://a/x", 1],
    ["http://a/y", 2],
    ["http://b/z", 3],
    ["other", 4],
  ]);
  const collapsed = buildGroupRows(values, 0, 0, "", "count", true, "/", new Set());
  expect(
    collapsed.map((row) => [row.kind, row.label, row.count, row.depth]),
  ).toEqual([
    ["group", "http:", 6, 0],
    ["value", "other", 4, 0],
  ]);
  expect(collapsed[0].pattern).toBe("^http:");

  const expanded = buildGroupRows(
    values,
    0,
    0,
    "",
    "count",
    true,
    "/",
    new Set(["group:http:", "group:http://a"]),
  );
  expect(
    expanded.map((row) => [row.kind, row.label, row.count, row.depth]),
  ).toEqual([
    ["group", "http:", 6, 0],
    ["group", "//b", 3, 1],
    ["group", "//a", 3, 1],
    ["value", "/y", 2, 2],
    ["value", "/x", 1, 2],
    ["value", "other", 4, 0],
  ]);
  const leaf = expanded.find((row) => row.label === "/x");
  expect(leaf?.pattern).toBe(String.raw`^http://a/x$`);
  expect(leaf?.tooltip).toBe("http://a/x");
  expect(leaf?.ancestors).toEqual(["group:http:", "group:http://a"]);
  expect(expanded.find((row) => row.label === "//a")?.pattern).toBe(
    "^http://a",
  );
});

test("a value that prefixes another value becomes a group row only", () => {
  const rows = buildGroupRows(
    new Map([
      ["/a", 1],
      ["/a/b", 2],
    ]),
    0,
    0,
    "",
    "count",
    true,
    "/",
    new Set(["group:/a"]),
  );
  expect(rows.map((row) => [row.kind, row.label, row.count])).toEqual([
    ["group", "/a", 3],
    ["value", "/b", 2],
  ]);
});

test("tree search filters by full value and force-expands branches", () => {
  const rows = buildGroupRows(
    new Map([
      ["/api/user", 1],
      ["/api/order", 2],
      ["/other", 4],
    ]),
    1,
    1,
    "user",
    "count",
    true,
    "/",
    new Set(),
  );
  // Special rows stay only when their label matches, like the flat list.
  expect(
    rows.map((row) => [row.kind, row.label, row.count, row.depth]),
  ).toEqual([
    ["group", "/api", 1, 0],
    ["value", "/user", 1, 1],
  ]);
  expect(rows[0].expanded).toBe(true);
});
