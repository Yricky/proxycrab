import { expect, test } from "vitest";
import { scriptTreeRows, validateScriptPath } from "./script-tree.ts";

test("directories precede files, preserve full identities and collapse independently", () => {
  const scripts = [
    { name: "z" },
    { name: "auth/token" },
    { name: "auth/nested/token" },
    { name: "auth" },
  ];
  const rows = scriptTreeRows(scripts, new Set());
  expect(rows.map((row) => [row.label, row.depth])).toEqual([
    ["auth", 0],
    ["nested", 1],
    ["token.lua", 2],
    ["token.lua", 1],
    ["auth.lua", 0],
    ["z.lua", 0],
  ]);
  expect(rows[2].script).toBe(scripts[2]);
  expect(scriptTreeRows(scripts, new Set(["auth"])).map((row) => row.label)).toEqual([
    "auth",
    "auth.lua",
    "z.lua",
  ]);
});

test("relative paths accept nested Unicode names and reject traversal and invalid segments", () => {
  for (const name of ["root", "业务/错误请求", "a/b/c", "a.b/c-d_e"])
    expect(validateScriptPath(name)).toBeNull();
  for (const name of [
    "",
    "/a",
    "a/",
    "a//b",
    "../a",
    "a/../b",
    "./a",
    "a\\b",
    "C:/a",
    "a b",
    "中".repeat(43),
  ]) {
    expect(validateScriptPath(name), name).not.toBeNull();
  }
});
