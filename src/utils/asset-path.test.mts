import { expect, test } from "vitest";
import { validateAssetPath } from "./asset-path.ts";
test("asset upload names follow the store contract without rewriting names", () => {
  for (const path of ["images/icon.png", "a-b/c_1.json", "empty"])
    expect(validateAssetPath(path)).toBeNull();
  for (const path of [
    "",
    "A.png",
    "中文.png",
    "a b.png",
    "../a",
    "a/../b",
    "a//b",
    "/a",
    "a/",
    "a/.metadata/b",
    "a".repeat(101),
    "a/".repeat(128),
  ])
    expect(validateAssetPath(path), path).not.toBeNull();
});
