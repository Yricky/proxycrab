import { expect, test } from "vitest";
import { fileTreeRows, revealFilePath } from "./file-tree.ts";

test("directories precede files and files can share directory names without losing identity", () => {
  const entries = ["z", "auth/token", "auth/nested/token", "auth"].map((path) => ({
    path,
    label: `${path.split("/").pop()}.lua`,
    data: { name: path },
  }));
  const rows = fileTreeRows(entries, new Set());
  expect(rows.map((row) => [row.label, row.depth])).toEqual([
    ["auth", 0],
    ["nested", 1],
    ["token.lua", 2],
    ["token.lua", 1],
    ["auth.lua", 0],
    ["z.lua", 0],
  ]);
  expect(rows[2].entry).toBe(entries[2]);
  expect(fileTreeRows(entries, new Set(["auth"])).map((row) => row.label)).toEqual([
    "auth",
    "auth.lua",
    "z.lua",
  ]);
});

test("assets retain extensions and metadata; revealing a selection opens only its ancestors", () => {
  const entries = ["images/mobile/icon.png", "images/logo.svg", "other/data.json"].map((path) => ({
    path,
    data: { size: 42 },
  }));
  const collapsed = new Set(["images", "images/mobile", "other"]);
  revealFilePath(collapsed, "images/mobile/icon.png");
  expect([...collapsed]).toEqual(["other"]);
  const rows = fileTreeRows(entries, collapsed);
  expect(rows.map((row) => row.label)).toEqual([
    "images",
    "mobile",
    "icon.png",
    "logo.svg",
    "other",
  ]);
  expect(rows[2].entry?.data.size).toBe(42);
  expect(fileTreeRows([], collapsed)).toEqual([]);
});
