import { computed, reactive, watch } from "vue";
import { createTauriBackend } from "../api/tauri-backend";
import type { ColumnView, LogRow } from "../api/types";
import { reportError } from "./app";
import { sessionsStore } from "./sessions";

const backend = createTauriBackend();

const SNAPSHOT_LIMIT = 300;
const FILTER_LIMIT = 1000;
const MAX_ROWS = 5000;
const POLL_INTERVAL = 1000;
const FILTER_POLL_INTERVAL = 2000;

let pollTimer: number | undefined;
let pollInFlight = false;

export const logsStore = reactive({
  columns: [] as ColumnView[],
  /** Newest first (descending id). */
  rows: [] as LogRow[],
  sortDesc: true,
  filterScript: "",
  appliedFilter: "",
  loading: false,

  get filterActive(): boolean {
    return this.appliedFilter.trim().length > 0;
  },

  /** Rows in display order. */
  displayRows: computed((): LogRow[] => {
    return logsStore.sortDesc ? logsStore.rows : [...logsStore.rows].reverse();
  }),

  reset(): void {
    this.columns = [];
    this.rows = [];
    this.filterScript = "";
    this.appliedFilter = "";
    this.sortDesc = true;
  },

  mergeRows(incoming: LogRow[]): void {
    if (incoming.length === 0) return;
    const byId = new Map<number, LogRow>();
    for (const row of this.rows) byId.set(row.id, row);
    for (const row of incoming) byId.set(row.id, row);
    const merged = [...byId.values()].sort((a, b) => b.id - a.id);
    if (merged.length > MAX_ROWS) merged.length = MAX_ROWS;
    this.rows = merged;
  },

  async poll(): Promise<void> {
    const sessionId = sessionsStore.viewingSessionId;
    if (sessionId === null || pollInFlight) return;
    pollInFlight = true;
    try {
      if (this.filterActive) {
        const payload = await backend.filterLogs({
          session_id: sessionId,
          limit: FILTER_LIMIT,
          script: this.appliedFilter,
        });
        this.columns = payload.columns;
        this.rows = payload.rows;
      } else {
        const payload = await backend.listLogs({
          session_id: sessionId,
          limit: SNAPSHOT_LIMIT,
        });
        this.columns = payload.columns;
        this.mergeRows(payload.rows);
      }
    } catch {
      // Polling errors are transient (e.g. no active session); stay quiet.
    } finally {
      pollInFlight = false;
    }
  },

  async applyFilter(script: string): Promise<void> {
    const sessionId = sessionsStore.viewingSessionId;
    const trimmed = script.trim();
    if (trimmed === "") {
      this.appliedFilter = "";
      this.rows = [];
      await this.poll();
      return;
    }
    if (sessionId === null) return;
    this.loading = true;
    try {
      const payload = await backend.filterLogs({
        session_id: sessionId,
        limit: FILTER_LIMIT,
        script: trimmed,
      });
      this.columns = payload.columns;
      this.rows = payload.rows;
      this.appliedFilter = trimmed;
      await backend.addFilterHistory(trimmed);
    } catch (error) {
      reportError(error, "过滤失败");
    } finally {
      this.loading = false;
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
      logsStore.loading = true;
      void logsStore.poll().finally(() => {
        logsStore.loading = false;
      });
    }
  },
);
