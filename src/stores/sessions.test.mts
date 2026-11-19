import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { SessionsPayload } from "../api/types.ts";

const { backend, reportError } = vi.hoisted(() => ({
  backend: {
    subscribeChanges: vi.fn(),
    listSessions: vi.fn(),
    listArchivedSessions: vi.fn(),
    listSessionShares: vi.fn(),
    getSessionView: vi.fn(),
    getSessionInterceptors: vi.fn(),
    createSession: vi.fn(),
    archiveSession: vi.fn(),
    replaceActiveSession: vi.fn(),
  },
  reportError: vi.fn(),
}));
vi.mock("../api/runtime-backend", () => ({ runtimeBackend: backend }));
vi.mock("./app", () => ({ reportError }));

afterEach(() => sessionsStore.dispose());

const { sessionsStore } = await import("./sessions.ts");

beforeEach(() => {
  sessionsStore.dispose();
  vi.resetAllMocks();
  backend.subscribeChanges.mockResolvedValue(() => {});
  backend.listSessionShares.mockResolvedValue({ shares: [] });
  sessionsStore.shares = {};
  sessionsStore.sessions = [];
  sessionsStore.archivedSessions = [];
  sessionsStore.activeSessionId = null;
  sessionsStore.loading = false;
});

test.each(["refresh", "init", "syncFromBackend"] as const)(
  "%s reads sessions and active state together, including clearing stale state",
  async (method) => {
    const session = { id: 3, name: "capture", created_at: 10, description: null };
    const archived = { id: 4, name: "archived", created_at: 20, description: null };
    for (const payload of [
      { sessions: [session], active_session_id: 3 },
      { sessions: [session], active_session_id: null },
      { sessions: [], active_session_id: null },
    ] satisfies SessionsPayload[]) {
      backend.listSessions.mockResolvedValue(payload);
      backend.listArchivedSessions.mockResolvedValue([archived]);
      await sessionsStore[method]();
      expect(sessionsStore.sessions).toEqual(payload.sessions);
      expect(sessionsStore.activeSessionId).toBe(payload.active_session_id);
      expect(sessionsStore.archivedSessions).toEqual([archived]);
      expect(sessionsStore.loading).toBe(false);
    }
    expect(backend.listSessions).toHaveBeenCalledTimes(3);
    expect(reportError).not.toHaveBeenCalled();
  },
);

test("one batch replaces share summaries, including removed shares", async () => {
  backend.listSessionShares.mockResolvedValueOnce({
    shares: [
      { session_id: 3, persistent: true },
      { session_id: 8, persistent: false },
    ],
  });
  await sessionsStore.refreshShares();
  expect(sessionsStore.shares[3]).toEqual({ session_id: 3, persistent: true });
  expect(backend.listSessionShares).toHaveBeenCalledTimes(1);
  backend.listSessionShares.mockResolvedValueOnce({ shares: [] });
  await sessionsStore.refreshShares();
  expect(sessionsStore.shares).toEqual({});
});

test("failed reads preserve the last known state instead of clearing all shares", async () => {
  sessionsStore.updateShare({
    session_id: 3,
    enabled: true,
    persistent: true,
    token: "do-not-cache",
  });
  backend.listSessionShares.mockRejectedValueOnce({ code: "permission_denied" });
  await sessionsStore.refreshShares();
  expect(sessionsStore.shares).toEqual({ 3: { session_id: 3, persistent: true } });
  expect(reportError).toHaveBeenCalledTimes(1);
});

test("a delayed batch cannot undo a successful local disable", async () => {
  let resolve!: (payload: { shares: { session_id: number; persistent: boolean }[] }) => void;
  backend.listSessionShares.mockImplementationOnce(
    () =>
      new Promise((done) => {
        resolve = done;
      }),
  );
  const pending = sessionsStore.refreshShares();
  sessionsStore.updateShare({ session_id: 3, enabled: false, persistent: false });
  resolve({
    shares: [
      { session_id: 3, persistent: true },
      { session_id: 8, persistent: false },
    ],
  });
  await pending;
  expect(sessionsStore.shares[3]).toBeUndefined();
  expect(sessionsStore.shares[8]).toEqual({ session_id: 8, persistent: false });
});

test.each([3, 8, null])(
  "sweep preserves active identity when the original active ID is %s",
  async (initialActive) => {
    const original = { id: 3, name: "capture", created_at: 10, description: "keep me" };
    const created = { ...original, id: 9, created_at: 20 };
    let live = [original];
    let activeId = initialActive;
    sessionsStore.sessions = live;
    sessionsStore.activeSessionId = activeId;
    backend.listSessions.mockImplementation(async () => ({
      sessions: live,
      active_session_id: activeId,
    }));
    backend.listArchivedSessions.mockResolvedValue([]);
    backend.getSessionView.mockResolvedValue({ columns: [], filter: { option: null, input: "" } });
    backend.getSessionInterceptors.mockResolvedValue({ request: [], response: [] });
    backend.createSession.mockImplementation(async () => {
      live = [...live, created];
      return created;
    });
    backend.replaceActiveSession.mockImplementation(async ({ session_id }) => {
      activeId = session_id;
      return { session_id };
    });
    backend.archiveSession.mockImplementation(async () => {
      live = live.filter((s) => s.id !== original.id);
    });

    expect(await sessionsStore.sweep(original.id)).toEqual(created);
    expect(backend.createSession).toHaveBeenCalledWith(
      expect.objectContaining({ name: original.name, description: original.description }),
    );
    expect(backend.archiveSession).toHaveBeenCalledWith(original.id);
    expect(sessionsStore.sessions).toEqual([created]);
    expect(sessionsStore.activeSessionId).toBe(
      initialActive === original.id ? created.id : initialActive,
    );
    expect(backend.replaceActiveSession).toHaveBeenCalledTimes(
      initialActive === original.id ? 1 : 0,
    );
    if (initialActive === original.id) {
      expect(backend.replaceActiveSession.mock.invocationCallOrder[0]).toBeLessThan(
        backend.archiveSession.mock.invocationCallOrder[0],
      );
    }
    expect(reportError).not.toHaveBeenCalled();
  },
);

test("failed inactive sweep leaves the active Session unchanged and reports the created copy", async () => {
  const original = { id: 3, name: "capture", created_at: 10, description: null };
  const created = { ...original, id: 9 };
  sessionsStore.sessions = [original];
  sessionsStore.activeSessionId = 8;
  backend.getSessionView.mockResolvedValue({ columns: [], filter: { option: null, input: "" } });
  backend.getSessionInterceptors.mockResolvedValue({ request: [], response: [] });
  backend.createSession.mockResolvedValue(created);
  backend.listSessions.mockResolvedValue({ sessions: [original, created], active_session_id: 8 });
  backend.listArchivedSessions.mockResolvedValue([]);
  backend.archiveSession.mockRejectedValue(new Error("disk error"));
  expect(await sessionsStore.sweep(original.id)).toBeNull();
  expect(backend.replaceActiveSession).not.toHaveBeenCalled();
  expect(sessionsStore.activeSessionId).toBe(8);
  expect(reportError).toHaveBeenCalledWith(
    expect.any(Error),
    "新会话已创建，但原会话归档失败，请在侧边栏手动归档",
  );
});
