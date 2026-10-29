import { expect, test } from "vitest";
import { validateScriptPath } from "./script-tree.ts";

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
