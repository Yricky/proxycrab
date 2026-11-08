import { reactive } from "vue";
import { runtimeBackend as backend } from "../api/runtime-backend";
import type { SessionMetadata } from "../api/generated/SessionMetadata";
import type { SessionShareState } from "../api/generated/SessionShareState";
import type { SessionShareSummary } from "../api/generated/SessionShareSummary";
import { sweepCreateRequest } from "../utils/session-sweep";
import { reportError } from "./app";
import { t } from "../i18n";

let shareRevision = 0;
let shareReadSequence = 0;
const shareWrites = new Map<number, number>();

export const sessionsStore = reactive({
  sessions: [] as SessionMetadata[],
  archivedSessions: [] as SessionMetadata[],
  activeSessionId: null as number | null,
  loading: false,
  shares: {} as Record<number, SessionShareSummary>,

  async refreshShares(): Promise<void> {
    const sequence = ++shareReadSequence;
    const revision = shareRevision;
    if (!backend.listSessionShares) return;
    try {
      const { shares } = await backend.listSessionShares();
      if (sequence !== shareReadSequence) return;
      const next: Record<number, SessionShareSummary> = Object.fromEntries(
        shares.map((share) => [share.session_id, share]),
      );
      for (const [id, changedAt] of shareWrites) {
        if (changedAt <= revision) {
          shareWrites.delete(id);
        } else if (this.shares[id]) next[id] = this.shares[id];
        else delete next[id];
      }
      this.shares = next;
    } catch (error) {
      if (sequence === shareReadSequence) reportError(error, t("读取会话分享状态失败"));
    }
  },

  updateShare(state: SessionShareState): void {
    shareWrites.set(state.session_id, ++shareRevision);
    if (state.enabled)
      this.shares[state.session_id] = {
        session_id: state.session_id,
        persistent: state.persistent,
      };
    else delete this.shares[state.session_id];
  },

  async refresh(): Promise<void> {
    try {
      const [sessionList, archivedSessions] = await Promise.all([
        backend.listSessions(),
        backend.listArchivedSessions(),
        this.refreshShares(),
      ]);
      this.sessions = sessionList.sessions;
      this.archivedSessions = archivedSessions;
      this.activeSessionId = sessionList.active_session_id;
    } catch (error) {
      reportError(error, t("获取会话列表失败"));
    }
  },

  async init(): Promise<void> {
    this.loading = true;
    try {
      const [sessionList, archivedSessions] = await Promise.all([
        backend.listSessions(),
        backend.listArchivedSessions(),
        this.refreshShares(),
      ]);
      this.sessions = sessionList.sessions;
      this.archivedSessions = archivedSessions;
      this.activeSessionId = sessionList.active_session_id;
    } catch (error) {
      reportError(error, t("初始化会话失败"));
    } finally {
      this.loading = false;
    }
  },

  async syncFromBackend(): Promise<void> {
    try {
      const [sessionList, archivedSessions] = await Promise.all([
        backend.listSessions(),
        backend.listArchivedSessions(),
        this.refreshShares(),
      ]);
      this.sessions = sessionList.sessions;
      this.archivedSessions = archivedSessions;
      this.activeSessionId = sessionList.active_session_id;
    } catch (error) {
      reportError(error, t("同步会话状态失败"));
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
      reportError(error, t("创建会话失败"));
      return null;
    }
  },

  async update(id: number, name: string, description: string | null): Promise<boolean> {
    try {
      await backend.updateSession(id, {
        name,
        description,
      });
      await this.refresh();
      return true;
    } catch (error) {
      reportError(error, t("更新会话失败"));
      return false;
    }
  },

  async archive(id: number): Promise<boolean> {
    try {
      await backend.archiveSession(id);
      await this.refresh();
      return true;
    } catch (error) {
      reportError(error, t("归档会话失败"));
      return false;
    }
  },

  async restore(id: number): Promise<boolean> {
    try {
      await backend.restoreSession(id);
      await this.refresh();
      return true;
    } catch (error) {
      reportError(error, t("取消归档失败"));
      return false;
    }
  },

  async deleteArchived(id: number): Promise<boolean> {
    try {
      await backend.deleteArchivedSession(id);
      await this.refresh();
      return true;
    } catch (error) {
      reportError(error, t("删除已归档会话失败"));
      return false;
    }
  },

  /**
   * 清扫当前活跃会话：新建一个配置一致的新会话，切换为新活跃会话（只有这一步会断开
   * 原会话的连接与在途请求），再把原会话归档。返回新会话；任一步失败返回 null 并已上报错误。
   */
  async sweep(sessionId: number | null): Promise<SessionMetadata | null> {
    const id = sessionId;
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
      try {
        await backend.archiveSession(id);
      } catch (error) {
        reportError(error, t("新会话已启用，但原会话归档失败，请在侧边栏手动归档"));
        return null;
      }
      await this.refresh();
      return created;
    } catch (error) {
      reportError(error, t("清扫会话失败"));
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
      reportError(error, t("切换活跃会话失败"));
      return false;
    }
  },
});
