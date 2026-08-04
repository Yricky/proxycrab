import { reactive } from "vue";
import { createTauriBackend } from "../api/tauri-backend";
import type { SessionMetadata } from "../api/types";
import { reportError } from "./app";

const backend = createTauriBackend();

export const sessionsStore = reactive({
  sessions: [] as SessionMetadata[],
  /** The session whose logs are shown in the main table. */
  viewingSessionId: null as number | null,
  activeSessionId: null as number | null,
  loading: false,

  async refresh(): Promise<void> {
    try {
      const [sessions, active] = await Promise.all([
        backend.listSessions(),
        backend.getActiveSession(),
      ]);
      this.sessions = sessions;
      this.activeSessionId = active.session_id;
      if (
        this.viewingSessionId !== null &&
        !this.sessions.some((s) => s.id === this.viewingSessionId)
      ) {
        this.viewingSessionId = this.sessions[0]?.id ?? null;
      }
    } catch (error) {
      reportError(error, "获取会话列表失败");
    }
  },

  async init(): Promise<void> {
    this.loading = true;
    try {
      const [sessions, active] = await Promise.all([
        backend.listSessions(),
        backend.getActiveSession(),
      ]);
      this.sessions = sessions;
      this.activeSessionId = active.session_id;
      // 默认查看活跃会话；活跃会话不存在时回退到第一个会话
      const activeSession = sessions.find((s) => s.id === active.session_id);
      this.viewingSessionId = activeSession?.id ?? sessions[0]?.id ?? null;
    } catch (error) {
      reportError(error, "初始化会话失败");
    } finally {
      this.loading = false;
    }
  },

  async syncFromBackend(): Promise<void> {
    try {
      const [sessions, active] = await Promise.all([
        backend.listSessions(),
        backend.getActiveSession(),
      ]);
      this.sessions = sessions;
      this.activeSessionId = active.session_id;
      if (
        this.viewingSessionId === null ||
        !sessions.some((session) => session.id === this.viewingSessionId)
      ) {
        this.viewingSessionId = sessions[0]?.id ?? null;
      }
    } catch (error) {
      reportError(error, "同步会话状态失败");
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

  async update(
    id: number,
    name: string,
    description: string | null,
  ): Promise<boolean> {
    try {
      await backend.updateSession(id, {
        name,
        description,
      });
      await this.refresh();
      return true;
    } catch (error) {
      reportError(error, "更新会话失败");
      return false;
    }
  },

  async remove(id: number): Promise<boolean> {
    try {
      await backend.deleteSession(id);
      if (this.viewingSessionId === id) this.viewingSessionId = null;
      await this.refresh();
      if (this.viewingSessionId === null) {
        this.viewingSessionId = this.sessions[0]?.id ?? null;
      }
      return true;
    } catch (error) {
      reportError(error, "删除会话失败");
      return false;
    }
  },

  async replaceActive(sessionId: number | null): Promise<boolean> {
    try {
      const active = await backend.replaceActiveSession({ session_id: sessionId });
      this.activeSessionId = active.session_id;
      return true;
    } catch (error) {
      reportError(error, "切换活跃会话失败");
      return false;
    }
  },

  view(id: number): void {
    this.viewingSessionId = id;
  },
});
