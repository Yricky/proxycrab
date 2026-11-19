import { afterEach, expect, test, vi } from "vitest";
import { createShareBackend, SHARE_INVALID_EVENT, type ShareBootstrap } from "./share-backend";
import type { HttpApiChange } from "./generated/HttpApiChange";

const bootstrap: ShareBootstrap = {
  session: { id: 1, name: "shared", created_at: 0, description: null },
  view: {
    session_id: 1,
    columns: [],
    filter: { option: null, input: "" },
    redaction_revision: "v1",
  },
  proxy_status: { status: "stopped" },
  api_port: 18089,
};

afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

function subscription() {
  vi.useFakeTimers();
  const dispatchEvent = vi.fn();
  vi.stubGlobal("window", { dispatchEvent });
  let controller!: ReadableStreamDefaultController<Uint8Array>;
  const fetchMock = vi.fn(
    async () =>
      new Response(
        new ReadableStream<Uint8Array>({
          start(value) {
            controller = value;
          },
        }),
        { headers: { "Content-Type": "text/event-stream" } },
      ),
  );
  vi.stubGlobal("fetch", fetchMock);
  const send = async (value: unknown, event?: string) => {
    controller.enqueue(
      new TextEncoder().encode(
        `${event ? `event: ${event}\n` : ""}data: ${JSON.stringify(value)}\n\n`,
      ),
    );
    await vi.advanceTimersByTimeAsync(0);
  };
  const change = (resources: HttpApiChange["resources"], metadata = {}) => ({
    resources,
    session_id: 1,
    session_config_download: false,
    redaction_revision: "v1",
    ...metadata,
  });
  return { dispatchEvent, fetchMock, send, change };
}

test("share SSE forwards scoped invalidations without polling metadata or exposing write capabilities", async () => {
  const { send, change, fetchMock } = subscription();
  const backend = createShareBackend("http://localhost", "share secret", bootstrap);
  const received: HttpApiChange[] = [];
  const stop = await backend.subscribeChanges((value) => received.push(value));
  await send(change([{ kind: "all" }]));
  await send(change([{ kind: "column_scripts", names: ["auth-token"] }]));
  await send(change([{ kind: "session_interceptors" }]));
  await vi.advanceTimersByTimeAsync(4500);
  expect(received).toEqual([
    { resources: [{ kind: "all" }], session_id: 1 },
    { resources: [{ kind: "column_scripts", names: ["auth-token"] }], session_id: 1 },
    { resources: [{ kind: "session_interceptors" }], session_id: 1 },
  ]);
  expect(fetchMock).toHaveBeenCalledTimes(1);
  const [url, init] = fetchMock.mock.calls[0] as unknown as [URL, RequestInit];
  expect(url.pathname).toBe("/share-api/changes");
  expect(url.searchParams.getAll("token")).toEqual(["share secret"]);
  expect(init.referrerPolicy).toBe("no-referrer");
  expect(backend.replay).toBeUndefined();
  expect(backend.replaceSessionFilter).toBeUndefined();
  stop();
});

test("share SSE updates download capability before notifying consumers", async () => {
  const { send, change } = subscription();
  const backend = createShareBackend("http://localhost", "token", bootstrap);
  const capabilities: boolean[] = [];
  const changes: HttpApiChange[] = [];
  const stop = await backend.subscribeChanges((value) => {
    capabilities.push(Boolean(backend.downloadSessionConfig));
    changes.push(value);
  });
  await send(change([], { session_config_download: true }));
  await send(change([], { session_config_download: false }));
  expect(capabilities).toEqual([true, false]);
  expect(changes).toEqual([
    { resources: [{ kind: "session_share" }], session_id: 1 },
    { resources: [{ kind: "session_share" }], session_id: 1 },
  ]);
  stop();
});

test("share SSE invalidates redaction once per revision and recovers after binding failures", async () => {
  const { send, change, dispatchEvent } = subscription();
  const backend = createShareBackend("http://localhost", "token", bootstrap);
  const stop = await backend.subscribeChanges(() => {});
  await send(change([{ kind: "all" }]));
  expect(dispatchEvent).not.toHaveBeenCalled();
  await send(change([], { redaction_revision: "v2" }));
  await send(change([], { redaction_revision: "v2" }));
  expect(dispatchEvent).toHaveBeenCalledTimes(1);
  await send({ code: "redaction_failed", message: "failed" }, "error");
  await send({ code: "redaction_failed", message: "failed" }, "error");
  expect(dispatchEvent).toHaveBeenCalledTimes(2);
  await send(change([{ kind: "all" }], { redaction_revision: null }));
  expect(dispatchEvent).toHaveBeenCalledTimes(3);
  expect(
    dispatchEvent.mock.calls.every(([event]) => event.type === "proxycrab-redaction-changed"),
  ).toBe(true);
  stop();
});

test("revoked share streams announce terminal errors and do not reconnect", async () => {
  const { send, dispatchEvent, fetchMock } = subscription();
  const backend = createShareBackend("http://localhost", "token", bootstrap);
  const changes = vi.fn();
  const stop = await backend.subscribeChanges(changes);
  await send({ code: "invalid_share_token", message: "revoked" }, "error");
  expect(dispatchEvent.mock.calls[0]?.[0].type).toBe(SHARE_INVALID_EVENT);
  await vi.advanceTimersByTimeAsync(3000);
  expect(fetchMock).toHaveBeenCalledTimes(1);
  expect(changes).not.toHaveBeenCalled();
  stop();
});

test("share backend returns scoped session state and leaves write-side methods undefined", async () => {
  const backend = createShareBackend("http://localhost", "token", bootstrap);
  expect(await backend.listSessions()).toEqual({
    sessions: [bootstrap.session],
    active_session_id: 1,
  });
  expect(backend.replay).toBeUndefined();
  expect(backend.replaceSessionView).toBeUndefined();
  expect(backend.replaceSessionFilter).toBeUndefined();
  expect(backend.listHttpApprovals).toBeUndefined();
  expect(backend.listLocalIps).toBeUndefined();
});

test("share interceptor reads ignore supplied session IDs and expose no editing or breakpoint methods", async () => {
  const config = {
    session_id: 1,
    request: [{ name: "auth", enabled: true, valid: true }],
    response: [],
  };
  const fetchMock = vi.fn(
    async (_input: URL) => new Response(JSON.stringify({ ok: true, data: config })),
  );
  vi.stubGlobal("fetch", fetchMock);
  const backend = createShareBackend("http://localhost", "share secret", bootstrap);
  expect(await backend.getSessionInterceptors(999)).toEqual(config);
  const url = fetchMock.mock.calls[0]![0];
  expect(url.pathname).toBe("/share-api/session-interceptors");
  expect(url.searchParams.getAll("token")).toEqual(["share secret"]);
  expect(url.searchParams.has("session_id")).toBe(false);
  for (const method of [
    "replaceSessionInterceptors",
    "listInterceptors",
    "getInterceptor",
    "createInterceptor",
    "updateInterceptor",
    "deleteInterceptor",
    "listBreakpoints",
    "getBreakpoint",
  ] as const) {
    expect(backend[method]).toBeUndefined();
  }
});
