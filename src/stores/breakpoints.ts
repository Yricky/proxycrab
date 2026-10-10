import { reactive } from "vue";
import { runtimeBackend as backend } from "../api/runtime-backend";
import type { BreakpointSummary, InterceptorKind } from "../api/types";

const POLL_INTERVAL = 500;
let timer: number | undefined;
let inFlight = false;

/** 断点总是针对"当前正在查看的会话"：由持有日志视图的组件在启动轮询时绑定。 */
export interface BreakpointScope {
  sessionId: () => number | null;
  hasActive: () => boolean;
}

let scope: BreakpointScope | null = null;

export const breakpointsStore = reactive({
  sessionId: null as number | null,
  items: [] as BreakpointSummary[],

  count(kind: InterceptorKind, interceptorName: string): number {
    const sessionId = scope?.sessionId() ?? null;
    if (sessionId === null) return 0;
    return this.items.filter(
      (item) =>
        item.session_id === sessionId &&
        item.phase === kind &&
        item.interceptor_name === interceptorName,
    ).length;
  },

  matching(kind: InterceptorKind, interceptorName: string): BreakpointSummary[] {
    const sessionId = scope?.sessionId() ?? null;
    if (sessionId === null) return [];
    return this.items.filter(
      (item) =>
        item.session_id === sessionId &&
        item.phase === kind &&
        item.interceptor_name === interceptorName,
    );
  },

  async refresh(): Promise<void> {
    const sessionId = scope?.sessionId() ?? null;
    if (sessionId === null) {
      this.sessionId = null;
      this.items = [];
      return;
    }
    // 断点只能伴随 in_progress 记录存在：无活跃记录且无已知断点时跳过轮询。
    // 已知断点存在时继续轮询，直到其被放行/超时。
    if (!(scope?.hasActive() ?? false) && this.items.length === 0) return;
    if (inFlight) return;
    inFlight = true;
    try {
      const items = await backend.listBreakpoints({ session_id: sessionId });
      if ((scope?.sessionId() ?? null) === sessionId) {
        this.sessionId = sessionId;
        this.items = items;
      }
    } catch {
      // A session/proxy transition can make a polling request transiently fail.
    } finally {
      inFlight = false;
    }
  },

  startPolling(next: BreakpointScope): void {
    scope = next;
    if (timer !== undefined) return;
    void this.refresh();
    timer = window.setInterval(() => void this.refresh(), POLL_INTERVAL);
  },

  stopPolling(): void {
    if (timer !== undefined) window.clearInterval(timer);
    timer = undefined;
    inFlight = false;
    scope = null;
  },
});
