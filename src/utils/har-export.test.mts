import { expect, test, vi } from "vitest";
import type { Backend } from "../api/backend";
import type { LogDetail } from "../api/generated/LogDetail";
import { appendHar } from "./har-export";
function detail(id: number, changes: Partial<LogDetail> = {}): LogDetail {
  return {
    id,
    session_id: 7,
    created_at: 1700000000000,
    updated_at: 1700000000125,
    source_type: "proxy",
    source_addr: "127.0.0.1:1234",
    stage: "completed",
    outcome: "success",
    error: null,
    request: {
      method: "POST",
      uri: "https://example.com/path?a=1&a=2",
      version: "HTTP/1.1",
      tags: { team: "test" },
      headers: [
        { name: "Content-Type", value: "application/json" },
        { name: "Cookie", value: "sid=a=b; theme=dark" },
      ],
      body: { type: "json", content: { a: 1 }, size: 12 },
    },
    response: {
      status: 302,
      status_text: "Found",
      version: "HTTP/1.1",
      headers: [
        { name: "Content-Type", value: "image/png" },
        { name: "Location", value: "/next" },
        { name: "Set-Cookie", value: "id=token; Path=/; Secure; HttpOnly" },
      ],
      body: { type: "binary", size: 3 },
    },
    request_interceptors: [],
    response_interceptors: [],
    ...changes,
  };
}
async function run(records: LogDetail[], failBody = false) {
  const getLog = vi.fn(async (_sid, id) => records.find((record) => record.id === id)!);
  const fetchBody = vi.fn(async (_target, side) => {
    if (failBody) throw new Error("decode failure");
    const bytes =
      side === "request" ? new TextEncoder().encode('{ "a": 1 }\n') : new Uint8Array([0, 1, 255]);
    return { bytes, contentType: "", decodedSize: bytes.length };
  });
  const chunks: Uint8Array[] = [];
  const writer = {
    append: vi.fn(async (bytes) => {
      chunks.push(bytes);
    }),
    markAsFinish: vi.fn(),
    markAsError: vi.fn(),
  };
  const onProgress = vi.fn(),
    log = vi.fn();
  const result = await appendHar(
    { getLog, fetchBody } as unknown as Backend,
    writer,
    7,
    records.map((record) => record.id),
    new AbortController().signal,
    onProgress,
    log,
  );
  return {
    har: JSON.parse(chunks.map((bytes) => new TextDecoder().decode(bytes)).join("")),
    result,
    onProgress,
    log,
    getLog,
    fetchBody,
  };
}
test("HAR preserves list order, original JSON whitespace, binary content and derived headers", async () => {
  const { har, result } = await run([detail(9), detail(3)]);
  const entries = har.log.entries;
  expect(har.log.version).toBe("1.2");
  expect(entries.map((entry: any) => entry._proxyCrab.logId)).toEqual([9, 3]);
  expect(entries[0].request.postData.text).toBe('{ "a": 1 }\n');
  expect(entries[0].response.content).toMatchObject({ text: "AAH/", encoding: "base64", size: 3 });
  expect(entries[0].request.queryString).toEqual([
    { name: "a", value: "1" },
    { name: "a", value: "2" },
  ]);
  expect(entries[0].request.cookies[0]).toEqual({ name: "sid", value: "a=b" });
  expect(entries[0].response.cookies[0]).toEqual({
    name: "id",
    value: "token",
    path: "/",
    secure: true,
    httpOnly: true,
  });
  expect(entries[0].response.redirectURL).toBe("/next");
  expect(entries[0].time).toBe(125);
  expect(result).toMatchObject({ processed: 2, exported: 2, warnings: 0 });
});
test("failed and unfinished requests remain with status zero; websocket and synthetic CONNECT skip", async () => {
  const base = detail(1);
  const { har, result } = await run([
    detail(1, {
      outcome: "failed",
      response: null,
      error: { stage: "upstream", kind: "test", message: "offline" },
    }),
    detail(2, { outcome: "in_progress", response: null }),
    detail(3, { request: { ...base.request, version: "WebSocket/13" } }),
    detail(4, { request: { ...base.request, method: "CONNECT" }, stage: "tls_mitm" }),
    detail(5, { response: { ...base.response!, status: 101 } }),
  ]);
  expect(har.log.entries.map((e: any) => e.response.status)).toEqual([0, 0, 101]);
  expect(har.log.entries[0]._proxyCrab.error.message).toBe("offline");
  expect(result).toMatchObject({ processed: 5, exported: 3, skipped: 2 });
});
test("body read failures become explanatory text and warnings without stopping the export", async () => {
  const { har, result, log } = await run([detail(1), detail(2)], true);
  expect(har.log.entries).toHaveLength(2);
  expect(har.log.entries[0].request.postData.text).toContain("decode failure");
  expect(har.log.entries[0].response.content.text).toContain("Body 读取失败");
  expect(har.log.entries[0].response.content.encoding).toBeUndefined();
  expect(har.log.entries[0]._proxyCrab.bodyErrors.response).toContain("decode failure");
  expect(result.warnings).toBe(4);
  expect(log).toHaveBeenCalledWith("warning", expect.stringContaining("#1"));
});
test("empty selection produces valid HAR", async () => {
  expect((await run([])).har.log.entries).toEqual([]);
});
test("missing details are preserved as failed entries", async () => {
  const chunks: string[] = [];
  const log = vi.fn();
  await appendHar(
    { getLog: vi.fn().mockRejectedValue(new Error("gone")) } as unknown as Backend,
    {
      append: async (b) => {
        chunks.push(new TextDecoder().decode(b));
      },
      markAsFinish: vi.fn(),
      markAsError: vi.fn(),
    },
    7,
    [10],
    new AbortController().signal,
    vi.fn(),
    log,
  );
  expect(JSON.parse(chunks.join("")).log.entries[0]._proxyCrab).toMatchObject({
    logId: 10,
    stage: "export_error",
  });
});
test("cancellation and writer failures propagate rather than becoming body warnings", async () => {
  const controller = new AbortController();
  controller.abort();
  const writer = { append: vi.fn(), markAsFinish: vi.fn(), markAsError: vi.fn() };
  await expect(
    appendHar({} as Backend, writer, 7, [1], controller.signal, vi.fn(), vi.fn()),
  ).rejects.toMatchObject({ name: "AbortError" });
  expect(writer.append).not.toHaveBeenCalled();
  writer.append.mockRejectedValueOnce(new Error("disk full"));
  await expect(
    appendHar({} as Backend, writer, 7, [], new AbortController().signal, vi.fn(), vi.fn()),
  ).rejects.toThrow("disk full");
});
