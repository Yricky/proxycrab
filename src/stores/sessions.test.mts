import { beforeEach, expect, test, vi } from "vitest";
import type { SessionsPayload } from "../api/types.ts";

const { backend, reportError } = vi.hoisted(() => ({
  backend: { listSessions: vi.fn(), listArchivedSessions: vi.fn() },
  reportError: vi.fn(),
}));
vi.mock("../api/runtime-backend", () => ({ runtimeBackend: backend }));
vi.mock("./app", () => ({ reportError }));

const { sessionsStore } = await import("./sessions.ts");

beforeEach(() => {
  vi.clearAllMocks();
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
