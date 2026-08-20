import { reactive } from "vue";
import { runtimeBackend as backend } from "../api/runtime-backend";
import type { BreakpointSummary, InterceptorKind } from "../api/types";
import { logsStore } from "./logs";
import { sessionsStore } from "./sessions";

const POLL_INTERVAL = 500;
let timer: number | undefined;
let inFlight = false;

export const breakpointsStore = reactive({
  sessionId: null as number | null,
  items: [] as BreakpointSummary[],

  count(kind: InterceptorKind, interceptorName: string): number {
    const sessionId = sessionsStore.viewingSessionId;
    if (sessionId === null) return 0;
    return this.items.filter(
      (item) =>
        item.session_id === sessionId &&
        item.phase === kind &&
        item.interceptor_name === interceptorName,
    ).length;
  },

  matching(kind: InterceptorKind, interceptorName: string): BreakpointSummary[] {
    const sessionId = sessionsStore.viewingSessionId;
    if (sessionId === null) return [];
    return this.items.filter(
      (item) =>
        item.session_id === sessionId &&
        item.phase === kind &&
        item.interceptor_name === interceptorName,
    );
  },

  async refresh(): Promise<void> {
    const sessionId = sessionsStore.viewingSessionId;
    if (sessionId === null) {
      this.sessionId = null;
      this.items = [];
      return;
    }
    // 断点只能伴随 in_progress 记录存在：无活跃记录且无已知断点时跳过轮询。
    // 已知断点存在时继续轮询，直到其被放行/超时。
    if (!logsStore.hasActive && this.items.length === 0) return;
    if (inFlight) return;
    inFlight = true;
    try {
      const items = await backend.listBreakpoints({ session_id: sessionId });
      if (sessionsStore.viewingSessionId === sessionId) {
        this.sessionId = sessionId;
        this.items = items;
      }
    } catch {
      // A session/proxy transition can make a polling request transiently fail.
    } finally {
      inFlight = false;
    }
  },

  startPolling(): void {
    if (timer !== undefined) return;
    void this.refresh();
    timer = window.setInterval(() => void this.refresh(), POLL_INTERVAL);
  },

  stopPolling(): void {
    if (timer !== undefined) window.clearInterval(timer);
    timer = undefined;
    inFlight = false;
  },
});
