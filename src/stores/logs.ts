import { reactive, watch } from "vue";
import { createTauriBackend } from "../api/tauri-backend";
import type {
  ColumnView,
  LogViewRow,
  LogViewsPayload,
  SessionFilter,
} from "../api/types";
import { reportError } from "./app";
import { sessionsStore } from "./sessions";

const backend = createTauriBackend();

const VIEW_BATCH_SIZE = 200;
const ID_PAGE_SIZE = 10_000;
const POLL_INTERVAL = 1000;
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
  columns: [] as ColumnView[],
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

  get displayRows(): LogViewRow[] {
    const ids = this.sortDesc ? this.ids : [...this.ids].reverse();
    return ids.map(
      (id) =>
        this.rowsById.get(id) ?? {
          id,
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
    await this.hydrate(added);
  },

  async poll(): Promise<void> {
    const sessionId = sessionsStore.viewingSessionId;
    if (sessionId === null || pollInFlight || this.loading) return;
    pollInFlight = true;
    try {
      if (this.filterActive) {
        const payload = await backend.getLogIds({
          session_id: sessionId,
        });
        if (sessionsStore.viewingSessionId !== sessionId) return;
        this.syncAppliedFilter(payload.filter);
        const added = this.replaceNewestIdPage(payload.ids);
        await this.hydrate(added);
        await this.hydrate(this.ids.slice(0, VIEW_BATCH_SIZE));
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
        await this.hydrate(added);
        await this.hydrate(this.ids.slice(0, VIEW_BATCH_SIZE));
      }
    } catch {
      // Polling failures are transient (for example, while sessions switch).
    } finally {
      pollInFlight = false;
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
      await this.hydrate(added);
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
        this.filterActive ? FILTER_POLL_INTERVAL : POLL_INTERVAL,
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
