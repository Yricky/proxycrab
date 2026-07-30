import { reactive } from "vue";
import { createTauriBackend } from "../api/tauri-backend";
import type { SessionMetadata } from "../api/types";
import { reportError } from "./app";

const backend = createTauriBackend();

export const sessionsStore = reactive({
  sessions: [] as SessionMetadata[],
  activeSessionId: null as number | null,
  /** The session whose logs are shown in the main table. */
  viewingSessionId: null as number | null,
  loading: false,

  async refresh(): Promise<void> {
    try {
      this.sessions = await backend.listSessions();
      if (this.activeSessionId === null) {
        // active session is tracked via config; resolved in init()
      }
      if (
        this.viewingSessionId !== null &&
        !this.sessions.some((s) => s.id === this.viewingSessionId)
      ) {
        this.viewingSessionId = this.activeSessionId ?? this.sessions[0]?.id ?? null;
      }
    } catch (error) {
      reportError(error, "获取会话列表失败");
    }
  },

  async init(): Promise<void> {
    this.loading = true;
    try {
      const [sessions, config] = await Promise.all([
        backend.listSessions(),
        backend.getConfig(),
      ]);
      this.sessions = sessions;
      this.activeSessionId = config.active_session_id;
      this.viewingSessionId = config.active_session_id ?? sessions[0]?.id ?? null;
    } catch (error) {
      reportError(error, "初始化会话失败");
    } finally {
      this.loading = false;
    }
  },

  async create(name: string, description: string | null): Promise<SessionMetadata | null> {
    try {
      const session = await backend.createSession({
        name: name || null,
        description,
      });
      await this.refresh();
      return session;
    } catch (error) {
      reportError(error, "创建会话失败");
      return null;
    }
  },

  async rename(id: number, name: string, description?: string | null): Promise<void> {
    try {
      await backend.updateSession(id, {
        name,
        ...(description !== undefined ? { description } : {}),
      });
      await this.refresh();
    } catch (error) {
      reportError(error, "重命名会话失败");
    }
  },

  async remove(id: number): Promise<boolean> {
    try {
      await backend.deleteSession(id);
      if (this.viewingSessionId === id) this.viewingSessionId = null;
      await this.refresh();
      if (this.viewingSessionId === null) {
        this.viewingSessionId = this.activeSessionId ?? this.sessions[0]?.id ?? null;
      }
      return true;
    } catch (error) {
      reportError(error, "删除会话失败");
      return false;
    }
  },

  async activate(id: number): Promise<void> {
    try {
      await backend.activateSession(id);
      this.activeSessionId = id;
      await this.refresh();
    } catch (error) {
      reportError(error, "激活会话失败");
    }
  },

  view(id: number): void {
    this.viewingSessionId = id;
  },
});
