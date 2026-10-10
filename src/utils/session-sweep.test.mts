import { expect, test } from "vitest";
import { sweepCreateRequest } from "./session-sweep.ts";
import type {
  SessionInterceptorsPayload,
  SessionMetadata,
  SessionViewPayload,
} from "../api/types";

const session: SessionMetadata = {
  id: 7,
  name: "checkout-debug",
  created_at: 1785380000000,
  description: "Capture checkout failures",
};

const view: SessionViewPayload = {
  session_id: 7,
  columns: [
    { kind: "method", width: 50 },
    { kind: "script", width: 120, script_name: "correlation-id" },
  ],
  filter: {
    option: { kind: "column", column: { kind: "uri" }, regex: false },
    input: "example.com",
  },
};

const chains: SessionInterceptorsPayload = {
  session_id: 7,
  request: [
    { name: "rewrite", enabled: true, valid: true },
    { name: "broken", enabled: false, valid: false, display_name: "坏脚本" },
  ],
  response: [{ name: "tag", enabled: true, valid: true }],
};

test("sweep request copies session identity without switching the active session", () => {
  const request = sweepCreateRequest(session, view, chains);
  expect(request.name).toBe("checkout-debug");
  expect(request.description).toBe("Capture checkout failures");
  // 创建请求不携带活跃语义；激活由随后的 PUT /api/active-session 完成。
  expect("active" in request).toBe(false);
});

test("sweep request copies columns and filter unchanged", () => {
  const request = sweepCreateRequest(session, view, chains);
  expect(request.view?.columns).toEqual(view.columns);
  expect(request.view?.filter).toEqual(view.filter);
});

test("sweep request copies both interceptor chains by name and enabled flag", () => {
  const request = sweepCreateRequest(session, view, chains);
  expect(request.interceptors?.request).toEqual([
    { name: "rewrite", enabled: true },
    { name: "broken", enabled: false },
  ]);
  expect(request.interceptors?.response).toEqual([{ name: "tag", enabled: true }]);
});
