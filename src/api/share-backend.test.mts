import { afterEach, expect, test, vi } from "vitest";
import { createShareBackend, type ShareBootstrap } from "./share-backend.ts";
import type { HttpApiChange } from "./types.ts";

afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

test("share subscriptions refresh changed script labels without exposing source or changing references", async () => {
  vi.useFakeTimers();
  vi.stubGlobal("window", {
    setInterval,
    clearInterval,
    setTimeout,
    clearTimeout,
    dispatchEvent: vi.fn(),
  });
  let displayName: string | null = "鉴权token";
  const bootstrap: ShareBootstrap = {
    session: { id: 1, name: "shared", created_at: 0, description: null },
    view: { session_id: 1, columns: [], filter: { option: null, input: "" } },
    proxy_status: { status: "stopped" },
    api_port: 18089,
  };
  const fetchMock = vi.fn(async (input: URL, init: RequestInit) => {
    const path = new URL(input).pathname;
    if (path === "/share-api/proxy/changes") {
      return new Promise<Response>((_resolve, reject) => {
        init.signal?.addEventListener("abort", () => reject(new Error("aborted")), { once: true });
      });
    }
    const data =
      path === "/share-api/session-view"
        ? bootstrap.view
        : path === "/share-api/column-scripts"
          ? [{ name: "auth-token", display_name: displayName, content: "" }]
          : [];
    return new Response(JSON.stringify({ ok: true, data }));
  });
  vi.stubGlobal("fetch", fetchMock);
  const backend = createShareBackend("http://localhost", "test-share-token", bootstrap);
  const changes: HttpApiChange[] = [];
  const stop = await backend.subscribeChanges((change) => changes.push(change));
  try {
    await vi.advanceTimersByTimeAsync(1500);
    changes.length = 0;
    await vi.advanceTimersByTimeAsync(1500);
    expect(changes).toEqual([]);
    displayName = "新的标题";
    await vi.advanceTimersByTimeAsync(1500);
    expect(changes).toEqual([{ resources: ["column_scripts"], session_id: 1 }]);
    expect(await backend.listColumnScripts()).toEqual([
      { name: "auth-token", display_name: "新的标题", content: "" },
    ]);
    changes.length = 0;
    displayName = null;
    await vi.advanceTimersByTimeAsync(1500);
    expect(changes).toEqual([{ resources: ["column_scripts"], session_id: 1 }]);
  } finally {
    stop();
  }
  const requests = fetchMock.mock.calls.length;
  await vi.advanceTimersByTimeAsync(1500);
  expect(fetchMock).toHaveBeenCalledTimes(requests);
});

test("share backend returns scoped session state and leaves write-side methods undefined", async () => {
  const backend = createShareBackend("http://localhost", "test-share-token", {
    session: { id: 1, name: "shared", created_at: 0, description: null },
    view: { session_id: 1, columns: [], filter: { option: null, input: "" } },
    proxy_status: { status: "stopped" },
    api_port: 18089,
  });
  expect(await backend.listSessions()).toEqual({
    sessions: [{ id: 1, name: "shared", created_at: 0, description: null }],
    active_session_id: 1,
  });
  expect(backend.replay).toBeUndefined();
  expect(backend.replaceSessionView).toBeUndefined();
  expect(backend.replaceSessionFilter).toBeUndefined();
  expect(backend.listHttpApprovals).toBeUndefined();
  expect(backend.listLocalIps).toBeUndefined();
});

test("share policy revisions invalidate views once and detect failed/recovered bindings", async () => {
  vi.useFakeTimers();
  const dispatchEvent = vi.fn();
  vi.stubGlobal("window", { setInterval, clearInterval, setTimeout, clearTimeout, dispatchEvent });
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
  let revision: string | null = "v1";
  vi.stubGlobal("fetch", async (input: URL, init: RequestInit) => {
    const path = new URL(input).pathname;
    if (path === "/share-api/proxy/changes")
      return new Promise<Response>((_resolve, reject) => {
        init.signal?.addEventListener("abort", () => reject(new Error("aborted")), { once: true });
      });
    if (path === "/share-api/session-view") {
      if (revision === "failed")
        return new Response(
          JSON.stringify({ ok: false, error: { code: "redaction_failed", message: "failed" } }),
          { status: 500 },
        );
      return new Response(
        JSON.stringify({ ok: true, data: { ...bootstrap.view, redaction_revision: revision } }),
      );
    }
    return new Response(JSON.stringify({ ok: true, data: [] }));
  });
  const stop = await createShareBackend("http://localhost", "test", bootstrap).subscribeChanges(
    () => {},
  );
  try {
    await vi.advanceTimersByTimeAsync(1500);
    expect(dispatchEvent).not.toHaveBeenCalled();
    revision = "v2";
    await vi.advanceTimersByTimeAsync(1500);
    expect(dispatchEvent).toHaveBeenCalledTimes(1);
    expect(dispatchEvent.mock.calls[0]?.[0].type).toBe("proxycrab-redaction-changed");
    await vi.advanceTimersByTimeAsync(1500);
    expect(dispatchEvent).toHaveBeenCalledTimes(1);
    revision = "failed";
    await vi.advanceTimersByTimeAsync(3000);
    expect(dispatchEvent).toHaveBeenCalledTimes(2);
    revision = null;
    await vi.advanceTimersByTimeAsync(1500);
    expect(dispatchEvent).toHaveBeenCalledTimes(3);
  } finally {
    stop();
  }
});

test("share interceptor reads ignore supplied session IDs and expose no editing or breakpoint methods", async () => {
  const config = {
    session_id: 1,
    request: [{ name: "auth", enabled: true, valid: true }],
    response: [],
  };
  const fetchMock = vi.fn(async () => new Response(JSON.stringify({ ok: true, data: config })));
  vi.stubGlobal("fetch", fetchMock);
  const backend = createShareBackend("http://localhost", "share secret", {
    session: { id: 1, name: "shared", created_at: 0, description: null },
    view: { session_id: 1, columns: [], filter: { option: null, input: "" } },
    proxy_status: { status: "stopped" },
    api_port: 18089,
  });
  expect(await backend.getSessionInterceptors(999)).toEqual(config);
  const url = fetchMock.mock.calls[0]![0] as URL;
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

test("share polls interceptor order, labels, enabled and valid state and stops after unsubscribe", async () => {
  vi.useFakeTimers();
  vi.stubGlobal("window", {
    setInterval,
    clearInterval,
    setTimeout,
    clearTimeout,
    dispatchEvent: vi.fn(),
  });
  const bootstrap: ShareBootstrap = {
    session: { id: 1, name: "shared", created_at: 0, description: null },
    view: { session_id: 1, columns: [], filter: { option: null, input: "" } },
    proxy_status: { status: "stopped" },
    api_port: 18089,
  };
  let config = {
    session_id: 1,
    request: [{ name: "auth", display_name: "Auth", enabled: true, valid: true }],
    response: [],
  };
  const fetchMock = vi.fn(async (input: URL, init: RequestInit) => {
    if (input.pathname === "/share-api/proxy/changes")
      return new Promise<Response>((_resolve, reject) => {
        init.signal?.addEventListener("abort", () => reject(new Error("aborted")), { once: true });
      });
    const data =
      input.pathname === "/share-api/session-view"
        ? bootstrap.view
        : input.pathname === "/share-api/session-interceptors"
          ? config
          : [];
    return new Response(JSON.stringify({ ok: true, data }));
  });
  vi.stubGlobal("fetch", fetchMock);
  const changes: HttpApiChange[] = [];
  const stop = await createShareBackend("http://localhost", "token", bootstrap).subscribeChanges(
    (change) => changes.push(change),
  );
  try {
    await vi.advanceTimersByTimeAsync(1500);
    changes.length = 0;
    await vi.advanceTimersByTimeAsync(1500);
    expect(changes).toEqual([]);
    for (const request of [
      [{ name: "auth", display_name: "Renamed", enabled: true, valid: true }],
      [{ name: "auth", display_name: "Renamed", enabled: false, valid: true }],
      [{ name: "auth", display_name: "Renamed", enabled: false, valid: false }],
      [
        { name: "new", display_name: "New", enabled: true, valid: true },
        { name: "auth", display_name: "Renamed", enabled: false, valid: false },
      ],
      [],
    ]) {
      config = { ...config, request };
      await vi.advanceTimersByTimeAsync(1500);
      expect(changes).toEqual([{ resources: ["session_interceptors"], session_id: 1 }]);
      changes.length = 0;
    }
  } finally {
    stop();
  }
  const requests = fetchMock.mock.calls.length;
  await vi.advanceTimersByTimeAsync(3000);
  expect(fetchMock).toHaveBeenCalledTimes(requests);
});

test("slow share metadata polling never overlaps or emits late changes after unsubscribe", async () => {
  vi.useFakeTimers();
  vi.stubGlobal("window", {
    setInterval,
    clearInterval,
    setTimeout,
    clearTimeout,
    dispatchEvent: vi.fn(),
  });
  const bootstrap: ShareBootstrap = {
    session: { id: 1, name: "shared", created_at: 0, description: null },
    view: { session_id: 1, columns: [], filter: { option: null, input: "" } },
    proxy_status: { status: "stopped" },
    api_port: 18089,
  };
  let resolveInterceptors!: (response: Response) => void;
  const fetchMock = vi.fn(async (input: URL, init: RequestInit) => {
    if (input.pathname === "/share-api/proxy/changes")
      return new Promise<Response>((_resolve, reject) => {
        init.signal?.addEventListener("abort", () => reject(new Error("aborted")), { once: true });
      });
    if (input.pathname === "/share-api/session-interceptors")
      return new Promise<Response>((resolve) => {
        resolveInterceptors = resolve;
      });
    return new Response(
      JSON.stringify({
        ok: true,
        data: input.pathname === "/share-api/session-view" ? bootstrap.view : [],
      }),
    );
  });
  vi.stubGlobal("fetch", fetchMock);
  const changes = vi.fn();
  const stop = await createShareBackend("http://localhost", "token", bootstrap).subscribeChanges(
    changes,
  );
  await vi.advanceTimersByTimeAsync(4500);
  expect(
    fetchMock.mock.calls.filter(([url]) => url.pathname === "/share-api/session-interceptors"),
  ).toHaveLength(1);
  stop();
  resolveInterceptors(
    new Response(JSON.stringify({ ok: true, data: { session_id: 1, request: [], response: [] } })),
  );
  await vi.advanceTimersByTimeAsync(1500);
  expect(changes).not.toHaveBeenCalled();
});

test("configuration download capability follows owner changes and is absent without authorization", async () => {
  vi.useFakeTimers();
  vi.stubGlobal("window", {
    setInterval,
    clearInterval,
    setTimeout,
    clearTimeout,
    dispatchEvent: vi.fn(),
  });
  let enabled = false;
  const bootstrap: ShareBootstrap = {
    session: { id: 1, name: "shared", created_at: 0, description: null },
    view: { session_id: 1, columns: [], filter: { option: null, input: "" } },
    proxy_status: { status: "stopped" },
    api_port: 18089,
  };
  vi.stubGlobal("fetch", async (input: URL, init: RequestInit) => {
    const path = input.pathname;
    if (path === "/share-api/proxy/changes")
      return new Promise<Response>((_, reject) =>
        init.signal?.addEventListener("abort", () => reject(new Error("aborted"))),
      );
    return new Response(
      JSON.stringify({
        ok: true,
        data:
          path === "/share-api/capabilities"
            ? { session_config_download: enabled }
            : path === "/share-api/session-view"
              ? bootstrap.view
              : [],
      }),
    );
  });
  const backend = createShareBackend("http://localhost", "token", bootstrap);
  expect(backend.downloadSessionConfig).toBeUndefined();
  expect(backend.importBundle).toBeUndefined();
  const changes: HttpApiChange[] = [];
  const stop = await backend.subscribeChanges((c) => changes.push(c));
  enabled = true;
  await vi.advanceTimersByTimeAsync(1500);
  expect(backend.downloadSessionConfig).toBeTypeOf("function");
  enabled = false;
  await vi.advanceTimersByTimeAsync(1500);
  expect(backend.downloadSessionConfig).toBeUndefined();
  expect(changes.filter((c) => c.resources.includes("session_share"))).toHaveLength(2);
  stop();
});
