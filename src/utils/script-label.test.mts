import { expect, test } from "vitest";
import { scriptLabel, scriptLabelFor, scriptTitle } from "./script-label.ts";

test("display names do not change script lookup identity and duplicates remain distinguishable", () => {
  const scripts = [
    { name: "auth-token", display_name: "鉴权token" },
    { name: "other-token", display_name: "鉴权token" },
  ];
  expect(scripts.map(scriptLabel)).toEqual(["鉴权token", "鉴权token"]);
  expect(scripts.map(scriptTitle)).toEqual(["鉴权token (auth-token)", "鉴权token (other-token)"]);
  expect(scriptLabelFor(scripts, "auth-token")).toBe("鉴权token");
  expect(scriptLabelFor(scripts, "missing")).toBe("missing");
  expect(scriptLabel({ name: "old-script" })).toBe("old-script");
  expect(scriptTitle({ name: "old-script", display_name: null })).toBe("old-script");
});
