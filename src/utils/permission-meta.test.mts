import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { ACTION_META, impactGroupOf } from "./permission-meta.ts";

test("every current management action has explicit display metadata", () => {
  const source = readFileSync(
    new URL("../../crates/proxy-crab-mgr/src/permission.rs", import.meta.url),
    "utf8",
  );
  const ids = [...source.matchAll(/action!\(\s*"([A-Z]+)",\s*"([^"]+)"/g)].map(
    ([, method, route]) => `${method} ${route}`,
  );
  expect(Object.keys(ACTION_META).sort()).toEqual(ids.sort());
});

test("every configured action has a Chinese label instead of a route placeholder", () => {
  for (const [id, meta] of Object.entries(ACTION_META)) {
    expect(meta.label, id).toMatch(/\p{Script=Han}/u);
  }
});

test("impact groups are explicit, with CA in read and unknown actions in other", () => {
  expect(impactGroupOf("GET /api/ca")).toBe("read");
  expect(impactGroupOf("GET /api/redaction-scripts/{name}")).toBe("danger");
  expect(impactGroupOf("POST /api/logs/ids")).toBe("write");
  expect(impactGroupOf("DELETE /api/filter-scripts/{name}")).toBe("danger");
  expect(impactGroupOf("GET /api/logs/new")).toBe("other");
  expect(impactGroupOf("POST /api/unknown")).toBe("other");
});
