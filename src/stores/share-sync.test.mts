import { afterEach, expect, test, vi } from "vitest";
import { createShareBackend, type ShareBootstrap } from "../api/share-backend";
import { installBackend } from "../api/runtime-backend";
import { proxyStore } from "./proxy";
import { sessionsStore } from "./sessions";
import { createLogsStore, type LogsStore } from "./logs";
import { reportError } from "./app";

vi.mock("./app", () => ({ reportError: vi.fn() }));
let logs: LogsStore | undefined;
afterEach(() => {
  logs?.dispose();
  logs = undefined;
  proxyStore.dispose();
  sessionsStore.dispose();
  vi.unstubAllGlobals();
  vi.useRealTimers();
  vi.clearAllMocks();
});

test("shared view owners handle initial all and reconnect using only supported reads on one SSE connection", async () => {
  vi.useFakeTimers();
  vi.stubGlobal("window", Object.assign(new EventTarget(), { setTimeout, clearTimeout }));
  const bootstrap: ShareBootstrap = {
    session: { id: 7, name: "shared", created_at: 0, description: null },
    view: { session_id: 7, columns: [], filter: { option: null, input: "" } },
    proxy_status: { status: "stopped" },
    api_port: 18089,
  };
  const controllers: ReadableStreamDefaultController<Uint8Array>[] = [];
  const cancel = vi.fn();
  const fetchMock = vi.fn(async (input: URL | string) => {
    const url = new URL(input);
    if (url.pathname === "/share-api/changes") {
      return new Response(
        new ReadableStream<Uint8Array>({
          start: (controller) => {
            controllers.push(controller);
          },
          cancel,
        }),
        {
          headers: { "Content-Type": "text/event-stream" },
        },
      );
    }
    const data = {
      "/share-api/proxy/status": bootstrap.proxy_status,
      "/share-api/session-view": bootstrap.view,
      "/share-api/logs/ids": { matched_ids: [], in_progress_ids: [] },
    }[url.pathname];
    if (!data) throw new Error(`Unexpected shared request: ${url.pathname}`);
    return new Response(JSON.stringify({ ok: true, data }));
  });
  vi.stubGlobal("fetch", fetchMock);
  installBackend(createShareBackend("http://localhost", "share", bootstrap));
  logs = createLogsStore();
  await proxyStore.init();
  await sessionsStore.init();
  logs.view(7);
  await vi.advanceTimersByTimeAsync(0);
  const all =
    'data: {"resources":[{"kind":"all"}],"session_id":7,"session_config_download":false,"redaction_revision":null}\n\n';
  controllers[0]!.enqueue(new TextEncoder().encode(all));
  await vi.advanceTimersByTimeAsync(40);
  expect(sessionsStore.sessions).toEqual([bootstrap.session]);
  expect(proxyStore.status).toEqual(bootstrap.proxy_status);
  expect(
    fetchMock.mock.calls.filter(([url]) => new URL(url).pathname === "/share-api/changes"),
  ).toHaveLength(1);
  controllers[0]!.close();
  await vi.advanceTimersByTimeAsync(1000);
  controllers[1]!.enqueue(new TextEncoder().encode(all));
  await vi.advanceTimersByTimeAsync(40);
  expect(reportError).not.toHaveBeenCalled();
  expect(
    fetchMock.mock.calls.every(([url]) => new URL(url).pathname.startsWith("/share-api/")),
  ).toBe(true);
  logs.dispose();
  logs = undefined;
  proxyStore.dispose();
  expect(cancel).not.toHaveBeenCalled();
  sessionsStore.dispose();
  expect(cancel).toHaveBeenCalledTimes(1);
  const requests = fetchMock.mock.calls.length;
  await vi.advanceTimersByTimeAsync(2000);
  expect(fetchMock).toHaveBeenCalledTimes(requests);
});
