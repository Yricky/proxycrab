import { reactive } from "vue";
import { runtimeBackend as backend } from "../api/runtime-backend";
import type { SessionMetadata } from "../api/types";
import { sweepCreateRequest } from "../utils/session-sweep";
import { reportError } from "./app";

export const sessionsStore = reactive({
  sessions: [] as SessionMetadata[],
  archivedSessions: [] as SessionMetadata[],
  /** The session whose logs are shown in the main table. */
  viewingSessionId: null as number | null,
  activeSessionId: null as number | null,
  loading: false,

  async refresh(): Promise<void> {
    try {
      const [sessions, archivedSessions, active] = await Promise.all([
        backend.listSessions(),
        backend.listArchivedSessions(),
        backend.getActiveSession(),
      ]);
      this.sessions = sessions;
      this.archivedSessions = archivedSessions;
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
      const [sessions, archivedSessions, active] = await Promise.all([
        backend.listSessions(),
        backend.listArchivedSessions(),
        backend.getActiveSession(),
      ]);
      this.sessions = sessions;
      this.archivedSessions = archivedSessions;
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
      const [sessions, archivedSessions, active] = await Promise.all([
        backend.listSessions(),
        backend.listArchivedSessions(),
        backend.getActiveSession(),
      ]);
      this.sessions = sessions;
      this.archivedSessions = archivedSessions;
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
      const session = await backend.createSession({ name: name || null, description });
      // 创建不含活跃语义：首个会话需要额外切换一次活跃身份。
      if (this.sessions.length === 0) await this.replaceActive(session.id);
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

  async archive(id: number): Promise<boolean> {
    try {
      await backend.archiveSession(id);
      if (this.viewingSessionId === id) this.viewingSessionId = null;
      await this.refresh();
      if (this.viewingSessionId === null) {
        this.viewingSessionId = this.sessions[0]?.id ?? null;
      }
      return true;
    } catch (error) {
      reportError(error, "归档会话失败");
      return false;
    }
  },

  async restore(id: number): Promise<boolean> {
    try {
      await backend.restoreSession(id);
      await this.refresh();
      return true;
    } catch (error) {
      reportError(error, "取消归档失败");
      return false;
    }
  },

  async deleteArchived(id: number): Promise<boolean> {
    try {
      await backend.deleteArchivedSession(id);
      await this.refresh();
      return true;
    } catch (error) {
      reportError(error, "删除已归档会话失败");
      return false;
    }
  },

  /**
   * 清扫当前活跃会话：新建一个配置一致的新会话，切换为新活跃会话（只有这一步会断开
   * 原会话的连接与在途请求），再把原会话归档。返回新会话；任一步失败返回 null 并已上报错误。
   */
  async sweep(): Promise<SessionMetadata | null> {
    const id = this.viewingSessionId;
    if (id === null || this.activeSessionId !== id) return null;
    const current = this.sessions.find((session) => session.id === id);
    if (current === undefined) return null;
    try {
      const [view, chains] = await Promise.all([
        backend.getSessionView(id),
        backend.getSessionInterceptors(id),
      ]);
      const created = await backend.createSession(sweepCreateRequest(current, view, chains));
      await this.refresh();
      if (!(await this.replaceActive(created.id))) return null;
      this.view(created.id);
      try {
        await backend.archiveSession(id);
      } catch (error) {
        reportError(error, "新会话已启用，但原会话归档失败，请在侧边栏手动归档");
        return null;
      }
      await this.refresh();
      return created;
    } catch (error) {
      reportError(error, "清扫会话失败");
      await this.refresh();
      return null;
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
