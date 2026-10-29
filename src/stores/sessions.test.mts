import { beforeEach, expect, test, vi } from "vitest";
import type { SessionsPayload } from "../api/types.ts";

const { backend, reportError } = vi.hoisted(() => ({
  backend: { listSessions: vi.fn(), listArchivedSessions: vi.fn(), listSessionShares: vi.fn() },
  reportError: vi.fn(),
}));
vi.mock("../api/runtime-backend", () => ({ runtimeBackend: backend }));
vi.mock("./app", () => ({ reportError }));

const { sessionsStore } = await import("./sessions.ts");

beforeEach(() => {
  vi.resetAllMocks();
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
