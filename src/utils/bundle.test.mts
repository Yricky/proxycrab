import { expect, test } from "vitest";
import { zipSync, strToU8 } from "fflate";
import { parseBundle, resourceMatches } from "./bundle";
function bundle(intents: unknown[]) {
  return zipSync({
    "intents.json": strToU8(JSON.stringify({ version: 1, intents })),
    "sessions/1/metadata.json": strToU8(JSON.stringify({ name: "Demo" })),
  });
}
test("preview keeps stable intent IDs and unsupported intent types for explicit selection", () => {
  expect(
    parseBundle(
      bundle([
        { id: "a", type: "session", path: "sessions/1", assets: ["mock/"] },
        { id: "b", type: "future", path: "x" },
      ]),
    ).intents,
  ).toEqual([
    { id: "a", type: "session", path: "sessions/1", assets: ["mock/"], name: "Demo" },
    { id: "b", type: "future", path: "x", assets: [], name: "x" },
  ]);
});
test("duplicate or missing intent IDs and invalid ZIP paths are rejected", () => {
  const intent = { id: "a", type: "session", path: "sessions/1" };
  expect(() => parseBundle(bundle([intent, intent]))).toThrow(/ID/);
  expect(() => parseBundle(bundle([{ type: "session", path: "sessions/1" }]))).toThrow(/ID/);
  expect(() => parseBundle(zipSync({ "../intents.json": strToU8("{}") }))).toThrow(/路径/);
});
test("asset selection uses exact IDs or slash-terminated prefixes and deduplicates", () => {
  expect(
    resourceMatches(
      ["mock/", "mock/a.json", "extra.txt"],
      ["mock/a.json", "mock/nested/b.json", "mock-other/a.json", "extra.txt"],
    ),
  ).toEqual(["mock/a.json", "mock/nested/b.json", "extra.txt"]);
});
