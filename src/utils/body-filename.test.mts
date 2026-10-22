import { expect, test } from "vitest";
import { bodyFilename } from "./body-filename.ts";

const target = { kind: "log", id: 42, sessionId: 3 } as const;

test("download names prefer UTF-8 disposition and preserve quoted semicolons", () => {
  expect(bodyFilename(target, "response", [{ name: "Content-Disposition", value:
    "attachment; filename=fallback.txt; filename*=UTF-8''%E6%96%87%E4%BB%B6.json" }])).toBe("文件.json");
  expect(bodyFilename(target, "request", [{ name: "content-disposition", value:
    'attachment; filename="one;two.txt"; filename*=UTF-8\'\'%zz' }])).toBe("one;two.txt");
});

test("download names remove path components and fall back for unusable names", () => {
  expect(bodyFilename(target, "response", [{ name: "content-disposition", value:
    "attachment; filename*=UTF-8''..%2F..%2Freport%3F.txt" }])).toBe("report_.txt");
  expect(bodyFilename(target, "response", [{ name: "content-disposition", value:
    'attachment; filename=".."' }])).toBe("log-42-response.bin");
});

test("fallback names distinguish snapshot, breakpoint and side with MIME suffixes", () => {
  const headers = [{ name: "Content-Type", value: "application/problem+json; charset=utf-8" }];
  expect(bodyFilename(target, "request", headers)).toBe("log-42-request.json");
  expect(bodyFilename({ ...target, kind: "interceptor", executionId: 17 }, "response", headers))
    .toBe("log-42-execution-17-response.json");
  expect(bodyFilename({ kind: "breakpoint", id: 9 }, "request", []))
    .toBe("breakpoint-9-request.bin");
});
