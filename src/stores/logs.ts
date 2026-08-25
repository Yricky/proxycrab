import { reactive, watch } from "vue";
import { runtimeBackend as backend } from "../api/runtime-backend";
import type {
  Column,
  LogViewRow,
  LogViewsPayload,
  SessionFilter,
} from "../api/types";
import { reportError } from "./app";
import { proxyStore } from "./proxy";
import { sessionsStore } from "./sessions";
import { isStaleInProgress } from "../utils/capture-outcome";

const VIEW_BATCH_SIZE = 200;
const ID_PAGE_SIZE = 10_000;
/** 有活跃（in_progress）记录时的快轮询间隔。 */
const POLL_INTERVAL = 1000;
/** 无活跃记录时的低频 ID 探测间隔（新记录只能靠轮询发现，不能完全停）。 */
const IDLE_POLL_INTERVAL = 2000;
const FILTER_POLL_INTERVAL = 2000;

let pollTimer: number | undefined;
let pollInFlight = false;
let olderInFlight = false;

function cellErrorKey(id: number, columnIndex: number): string {
  return `${id}:${columnIndex}`;
}

export function emptySessionFilter(): SessionFilter {
  return { option: null, input: "" };
}

export function cloneSessionFilter(filter: SessionFilter): SessionFilter {
  return JSON.parse(JSON.stringify(filter)) as SessionFilter;
}

export const logsStore = reactive({
  columns: [] as Column[],
  /** IDs are retained newest first regardless of display direction. */
  ids: [] as number[],
  rowsById: new Map<number, LogViewRow>(),
  cellErrors: new Map<string, string>(),
  sortDesc: true,
  appliedFilter: emptySessionFilter() as SessionFilter,
  loading: false,
  olderExhausted: false,

  get filterActive(): boolean {
    return this.appliedFilter.option !== null && this.appliedFilter.input.length > 0;
  },

  /** 是否存在尚未完成的 in_progress 记录（已完成记录不会再变化，无需刷新）。 */
  get hasActive(): boolean {
    for (const row of this.rowsById.values()) {
      if (
        row.outcome === "in_progress" &&
        !isStaleInProgress(row.outcome, row.created_at, proxyStore.status)
      ) {
        return true;
      }
    }
    return false;
  },

  get displayRows(): LogViewRow[] {
    const ids = this.sortDesc ? this.ids : [...this.ids].reverse();
    return ids.map(
      (id) =>
        this.rowsById.get(id) ?? {
          id,
          created_at: 0,
          updated_at: 0,
          outcome: "success",
          cells: this.columns.map(() => ""),
        },
    );
  },

  syncAppliedFilter(filter: SessionFilter): void {
    if (JSON.stringify(this.appliedFilter) !== JSON.stringify(filter)) {
      this.appliedFilter = cloneSessionFilter(filter);
    }
  },

  resetData(): void {
    this.columns = [];
    this.ids = [];
    this.rowsById = new Map();
    this.cellErrors = new Map();
    this.sortDesc = true;
    this.olderExhausted = false;
  },

  reset(): void {
    this.resetData();
    this.appliedFilter = emptySessionFilter();
  },

  mergeIds(incoming: number[]): number[] {
    if (incoming.length === 0) return [];
    const known = new Set(this.ids);
    const added = incoming.filter((id) => !known.has(id));
    this.ids = [...new Set([...this.ids, ...incoming])].sort((left, right) => right - left);
    return added;
  },

  replaceNewestIdPage(incoming: number[]): number[] {
    const known = new Set(this.ids);
    const oldestIncoming = incoming.reduce(
      (oldest, id) => Math.min(oldest, id),
      Number.POSITIVE_INFINITY,
    );
    const retainedOlder =
      incoming.length === ID_PAGE_SIZE
        ? this.ids.filter((id) => id < oldestIncoming)
        : [];
    const next = [...new Set([...incoming, ...retainedOlder])].sort(
      (left, right) => right - left,
    );
    const nextSet = new Set(next);
    for (const id of this.ids) {
      if (!nextSet.has(id)) this.removeLog(id);
    }
    this.ids = next;
    return incoming.filter((id) => !known.has(id));
  },

  removeLog(id: number): void {
    this.ids = this.ids.filter((item) => item !== id);
    this.rowsById.delete(id);
    for (const key of this.cellErrors.keys()) {
      if (key.startsWith(`${id}:`)) this.cellErrors.delete(key);
    }
  },

  mergeViews(payload: LogViewsPayload): void {
    this.columns = payload.columns;
    const returnedIds = new Set(payload.rows.map((row) => row.id));
    for (const id of returnedIds) {
      for (const key of this.cellErrors.keys()) {
        if (key.startsWith(`${id}:`)) this.cellErrors.delete(key);
      }
    }
    for (const row of payload.rows) this.rowsById.set(row.id, row);
    for (const exception of payload.exceptions) {
      if (exception.code === "log_not_found") {
        this.removeLog(exception.id);
      } else if (
        exception.code === "column_script_error" &&
        exception.column_index !== undefined &&
        exception.column_index !== null
      ) {
        this.cellErrors.set(
          cellErrorKey(exception.id, exception.column_index),
          exception.message,
        );
      }
    }
  },

  async hydrate(ids: number[], force = false): Promise<void> {
    const sessionId = sessionsStore.viewingSessionId;
    if (sessionId === null) return;
    if (ids.length === 0) {
      const payload = await backend.getLogViews({
        session_id: sessionId,
        logs: [],
      });
      if (sessionsStore.viewingSessionId === sessionId) this.mergeViews(payload);
      return;
    }
    for (let index = 0; index < ids.length; index += VIEW_BATCH_SIZE) {
      if (sessionsStore.viewingSessionId !== sessionId) return;
      const batch = ids.slice(index, index + VIEW_BATCH_SIZE);
      const payload = await backend.getLogViews({
        session_id: sessionId,
        logs: batch.map((id) => {
          const updatedAt = this.rowsById.get(id)?.updated_at;
          return force || updatedAt === undefined ? { id } : { id, updated_at: updatedAt };
        }),
      });
      if (sessionsStore.viewingSessionId !== sessionId) return;
      this.mergeViews(payload);
    }
  },

  async discoverInitial(): Promise<void> {
    const sessionId = sessionsStore.viewingSessionId;
    if (sessionId === null) return;
    const payload = await backend.getLogIds({
      session_id: sessionId,
    });
    if (sessionsStore.viewingSessionId !== sessionId) return;
    this.syncAppliedFilter(payload.filter);
    const added = this.mergeIds(payload.ids);
    if (added.length > 0) {
      await this.hydrate(added);
    } else if (this.columns.length === 0) {
      // 会话暂无日志：仅在列定义缺失（如 loadSession 失败后的恢复路径）时才发空列表请求补列。
      await this.hydrate(added);
    }
  },

  async poll(): Promise<void> {
    const sessionId = sessionsStore.viewingSessionId;
    if (sessionId === null || pollInFlight || this.loading) return;
    pollInFlight = true;
    try {
      const isActiveView = sessionsStore.activeSessionId === sessionId;
      if (!isActiveView) {
        // 查看非活跃会话：不会有新记录到达，只需跟踪已有活跃记录直到完成。
        await this.hydrateActive();
      } else if (this.filterActive) {
        const payload = await backend.getLogIds({
          session_id: sessionId,
        });
        if (sessionsStore.viewingSessionId !== sessionId) return;
        this.syncAppliedFilter(payload.filter);
        const added = this.replaceNewestIdPage(payload.ids);
        if (added.length > 0) await this.hydrate(added);
        await this.hydrateActive();
      } else if (this.ids.length === 0) {
        await this.discoverInitial();
      } else {
        const payload = await backend.getLogIds({
          session_id: sessionId,
          min_id: this.ids[0],
        });
        if (sessionsStore.viewingSessionId !== sessionId) return;
        this.syncAppliedFilter(payload.filter);
        const added = this.mergeIds(payload.ids);
        if (added.length > 0) await this.hydrate(added);
        await this.hydrateActive();
      }
    } catch {
      // Polling failures are transient (for example, while sessions switch).
    } finally {
      pollInFlight = false;
    }
  },

  /** 只轮询处于 in_progress 的活跃记录；无活跃记录时跳过（已完成记录不会再变化）。 */
  async hydrateActive(): Promise<void> {
    const ids: number[] = [];
    for (const [id, row] of this.rowsById) {
      if (
        row.outcome === "in_progress" &&
        !isStaleInProgress(row.outcome, row.created_at, proxyStore.status)
      ) {
        ids.push(id);
      }
    }
    if (ids.length > 0) {
      await this.hydrate(ids);
    } else if (this.columns.length === 0) {
      // 列定义缺失（如 loadSession 失败后的恢复路径）时补取列。
      await this.hydrate([]);
    }
  },

  async loadOlder(): Promise<void> {
    const sessionId = sessionsStore.viewingSessionId;
    const oldestId = this.ids[this.ids.length - 1];
    if (
      sessionId === null ||
      oldestId === undefined ||
      olderInFlight ||
      this.olderExhausted ||
      this.loading
    ) {
      return;
    }
    olderInFlight = true;
    try {
      const payload = await backend.getLogIds({
        session_id: sessionId,
        max_id: oldestId,
      });
      if (sessionsStore.viewingSessionId !== sessionId) return;
      this.syncAppliedFilter(payload.filter);
      this.olderExhausted = payload.ids.length === 0;
      const added = this.mergeIds(payload.ids);
      if (added.length > 0) await this.hydrate(added);
    } catch (error) {
      reportError(error, "加载更早记录失败");
    } finally {
      olderInFlight = false;
    }
  },

  async refreshView(): Promise<void> {
    this.columns = [];
    this.cellErrors = new Map();
    await this.hydrate(this.ids, true);
  },

  cellError(id: number, columnIndex: number): string | undefined {
    return this.cellErrors.get(cellErrorKey(id, columnIndex));
  },

  async loadSession(): Promise<void> {
    const sessionId = sessionsStore.viewingSessionId;
    if (sessionId === null) return;
    this.loading = true;
    try {
      const view = await backend.getSessionView(sessionId);
      if (sessionsStore.viewingSessionId !== sessionId) return;
      this.appliedFilter = cloneSessionFilter(view.filter);
      this.resetData();
      await this.discoverInitial();
    } catch (error) {
      reportError(error, "加载会话记录失败");
    } finally {
      if (sessionsStore.viewingSessionId === sessionId) this.loading = false;
    }
  },

  async applyFilter(filter: SessionFilter): Promise<boolean> {
    const sessionId = sessionsStore.viewingSessionId;
    if (sessionId === null) return false;
    this.loading = true;
    try {
      const payload = await backend.getLogIds({
        session_id: sessionId,
        filter: cloneSessionFilter(filter),
        persist_filter: !backend.capabilities.readonly,
      });
      if (sessionsStore.viewingSessionId !== sessionId) return false;
      this.appliedFilter = cloneSessionFilter(payload.filter);
      this.resetData();
      const added = this.mergeIds(payload.ids);
      await this.hydrate(added);
      return true;
    } catch (error) {
      reportError(error, "过滤失败");
      return false;
    } finally {
      if (sessionsStore.viewingSessionId === sessionId) this.loading = false;
    }
  },

  startPolling(): void {
    this.stopPolling();
    const tick = () => {
      void this.poll();
      pollTimer = window.setTimeout(
        tick,
        this.filterActive
          ? FILTER_POLL_INTERVAL
          : this.hasActive
            ? POLL_INTERVAL
            : IDLE_POLL_INTERVAL,
      );
    };
    pollTimer = window.setTimeout(tick, POLL_INTERVAL);
  },

  stopPolling(): void {
    if (pollTimer !== undefined) {
      window.clearTimeout(pollTimer);
      pollTimer = undefined;
    }
  },
});

watch(
  () => sessionsStore.viewingSessionId,
  (id, previous) => {
    if (id === previous) return;
    logsStore.reset();
    if (id !== null) {
      void logsStore.loadSession();
    }
  },
);
