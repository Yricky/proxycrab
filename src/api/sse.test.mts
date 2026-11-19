import { afterEach, expect, test, vi } from "vitest";
import { subscribeSse } from "./sse";
import { createHttpBackend } from "./http-backend";

afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

function connection() {
  let controller!: ReadableStreamDefaultController<Uint8Array>;
  const cancel = vi.fn();
  const response = new Response(
    new ReadableStream<Uint8Array>({
      start(value) {
        controller = value;
      },
      cancel,
    }),
    { headers: { "Content-Type": "text/event-stream" } },
  );
  return {
    response,
    cancel,
    send: (text: string | Uint8Array) =>
      controller.enqueue(typeof text === "string" ? new TextEncoder().encode(text) : text),
    close: () => controller.close(),
  };
}

test("SSE parses split UTF-8 frames, CRLF, comments and multiline data", async () => {
  vi.useFakeTimers();
  const stream = connection();
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => stream.response),
  );
  const handler = vi.fn();
  const stop = subscribeSse(new URL("http://localhost/changes"), {}, handler);
  stream.send(': keepalive\r\n\r\ndata: {"name":\r\n');
  const chinese = new TextEncoder().encode('data: "中文"}\r');
  const split = chinese.indexOf(0xe4) + 1;
  stream.send(chinese.slice(0, split));
  stream.send(chinese.slice(split));
  stream.send('\n\r\ndata: {"second":true}\n\n');
  await vi.advanceTimersByTimeAsync(0);
  expect(handler.mock.calls).toEqual([[{ name: "中文" }], [{ second: true }]]);
  stop();
  expect(stream.cancel).toHaveBeenCalledTimes(1);
});

test("HTTP subscriptions retain Bearer authentication and receive full invalidation on reconnect", async () => {
  vi.useFakeTimers();
  const first = connection();
  const second = connection();
  const fetchMock = vi
    .fn()
    .mockResolvedValueOnce(first.response)
    .mockResolvedValueOnce(second.response);
  vi.stubGlobal("fetch", fetchMock);
  const backend = createHttpBackend("http://localhost", "ui secret", {
    target: "cli",
    workspace: { current_path: "/tmp", configured_path: "/tmp" },
    http_service: { running: true, host: "0.0.0.0", port: 18089, error: null },
  });
  const handler = vi.fn();
  const stop = await backend.subscribeChanges(handler);
  first.send('data: {"resources":[{"kind":"all"}]}\n\n');
  first.close();
  await vi.advanceTimersByTimeAsync(1000);
  second.send('data: {"resources":[{"kind":"all"}]}\n\n');
  await vi.advanceTimersByTimeAsync(0);
  expect(handler).toHaveBeenCalledTimes(2);
  expect(fetchMock.mock.calls[0][0].pathname).toBe("/ui-api/changes");
  expect(fetchMock.mock.calls[0][1].headers.Authorization).toBe("Bearer ui secret");
  stop();
  await vi.advanceTimersByTimeAsync(3000);
  expect(fetchMock).toHaveBeenCalledTimes(2);
  expect(second.cancel).toHaveBeenCalledTimes(1);
});

test("unsubscribe cancels pending retries and suppresses responses that arrive late", async () => {
  vi.useFakeTimers();
  let resolve!: (value: Response) => void;
  const fetchMock = vi.fn(
    () =>
      new Promise<Response>((done) => {
        resolve = done;
      }),
  );
  vi.stubGlobal("fetch", fetchMock);
  const handler = vi.fn();
  const stop = subscribeSse(new URL("http://localhost/changes"), {}, handler);
  stop();
  const stream = connection();
  resolve(stream.response);
  stream.send('data: {"late":true}\n\n');
  await vi.advanceTimersByTimeAsync(3000);
  expect(handler).not.toHaveBeenCalled();
  expect(fetchMock).toHaveBeenCalledTimes(1);
  expect(fetchMock.mock.calls[0][1].signal.aborted).toBe(true);

  const retryFetch = vi.fn().mockRejectedValue(new Error("offline"));
  vi.stubGlobal("fetch", retryFetch);
  const stopRetry = subscribeSse(new URL("http://localhost/changes"), {}, handler);
  await vi.advanceTimersByTimeAsync(0);
  stopRetry();
  await vi.advanceTimersByTimeAsync(3000);
  expect(retryFetch).toHaveBeenCalledTimes(1);
});

test("SSE reports JSON authentication failures without retrying", async () => {
  vi.useFakeTimers();
  const fetchMock = vi.fn(
    async () =>
      new Response(
        JSON.stringify({
          ok: false,
          error: { code: "invalid_share_token", message: "invalid" },
        }),
        { status: 401 },
      ),
  );
  vi.stubGlobal("fetch", fetchMock);
  const error = vi.fn(() => true);
  const stop = subscribeSse(new URL("http://localhost/changes"), {}, vi.fn(), error);
  await vi.advanceTimersByTimeAsync(3000);
  expect(error.mock.calls[0][0].code).toBe("invalid_share_token");
  expect(fetchMock).toHaveBeenCalledTimes(1);
  stop();
});
