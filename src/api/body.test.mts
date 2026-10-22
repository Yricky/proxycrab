import { afterEach, expect, test, vi } from "vitest";
import { BodyFetchError, downloadBodyFromBase, fetchBodyFromBase, UI_BODY_MAX_SIZE } from "./body.ts";
import { saveBlobInWebview } from "./save-file.ts";

vi.mock("./save-file.ts", () => ({ saveBlobInWebview: vi.fn(async () => true) }));
afterEach(() => { vi.unstubAllGlobals(); vi.clearAllMocks(); });

test("downloads use the complete binary Blob and retain Bearer authentication beyond preview limits", async () => {
  const blob = new Blob([new Uint8Array([0, 255, 13, 10])]);
  const response = { ok: true, blob: vi.fn(async () => blob), arrayBuffer: vi.fn() };
  const fetchMock = vi.fn(async () => response);
  vi.stubGlobal("fetch", fetchMock);
  const target = { kind: "log", id: 42, sessionId: 3 } as const;
  expect(await downloadBodyFromBase("http://localhost", target, "response", "body.bin", "Bearer test"))
    .toBe(true);
  const [url, init] = fetchMock.mock.calls[0] as unknown as [URL, RequestInit];
  expect(url.pathname).toBe("/api/logs/42/body");
  expect(url.searchParams.get("session_id")).toBe("3");
  expect(url.searchParams.get("side")).toBe("response");
  expect(Number(url.searchParams.get("max_size"))).toBeGreaterThan(UI_BODY_MAX_SIZE);
  expect(init.headers).toEqual({ Authorization: "Bearer test" });
  expect(saveBlobInWebview).toHaveBeenCalledWith(blob, "body.bin");
  expect(response.arrayBuffer).not.toHaveBeenCalled();
  await expect(fetchBodyFromBase("http://localhost", target, "response", UI_BODY_MAX_SIZE + 1))
    .rejects.toBeInstanceOf(BodyFetchError);
  expect(fetchMock).toHaveBeenCalledTimes(1);
});

test("share snapshot downloads retain execution selection and query-token authentication", async () => {
  const fetchMock = vi.fn(async () => new Response(' { "a": 1 }\n'));
  vi.stubGlobal("fetch", fetchMock);
  await downloadBodyFromBase("http://localhost", {
    kind: "interceptor", id: 42, sessionId: 3, executionId: 17,
  }, "request", "body.json", undefined, "/share-api", "share secret");
  const [url, init] = fetchMock.mock.calls[0] as unknown as [URL, RequestInit];
  expect(url.pathname).toBe("/share-api/logs/42/body");
  expect(url.searchParams.get("execution_id")).toBe("17");
  expect(url.searchParams.getAll("token")).toEqual(["share secret"]);
  expect(init.referrerPolicy).toBe("no-referrer");
  const blob = vi.mocked(saveBlobInWebview).mock.calls[0][0];
  expect(await blob.text()).toBe(' { "a": 1 }\n');
});

test("breakpoint download errors retain the API error and do not save an error document", async () => {
  const fetchMock = vi.fn(async () => new Response(JSON.stringify({
    error: { code: "body_not_found", message: "Body 尚未捕获" },
  }), { status: 404 }));
  vi.stubGlobal("fetch", fetchMock);
  await expect(downloadBodyFromBase("http://localhost", { kind: "breakpoint", id: 7 },
    "request", "body.bin")).rejects.toMatchObject({ code: "body_not_found", message: "Body 尚未捕获" });
  const [url] = fetchMock.mock.calls[0] as unknown as [URL];
  expect(url.pathname).toBe("/api/breakpoints/7/body");
  expect(url.searchParams.has("session_id")).toBe(false);
  expect(saveBlobInWebview).not.toHaveBeenCalled();
});
