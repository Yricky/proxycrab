import { reactive, watch } from "vue";
import { runtimeBackend as backend } from "../api/runtime-backend";
import type {
  Column,
  LogViewRow,
  LogViewsPayload,
  SessionFilter,
} from "../api/types";
import { LruKeys } from "../utils/lru";
import { reportError } from "./app";
import { proxyStore } from "./proxy";
import { sessionsStore } from "./sessions";

const VIEW_BATCH_SIZE = 200;
const ID_PAGE_SIZE = 10_000;
const ROW_CACHE_CAPACITY = 5_000;
const POLL_INTERVAL = 1_000;
const IDLE_POLL_INTERVAL = 2_000;

let generation = 0;
let rowGeneration = 0;
let maxId = 0;
let pollTimer: number | undefined;
let pollingEnabled = false;
let pollInFlight = false;
let pollAgain = false;
let hydrateQueue = Promise.resolve();
let viewportTimer: number | undefined;
const pendingFilterIds = new Set<number>();
const viewportIds = new Set<number>();
const rowLru = new LruKeys<number>(ROW_CACHE_CAPACITY);

function cellErrorKey(id: number, columnIndex: number): string {
  return `${id}:${columnIndex}`;
}

function sortedUnique(ids: Iterable<number>): number[] {
  return [...new Set(ids)].sort((left, right) => right - left);
}

function maxOf(ids: number[]): number | undefined {
  return ids.reduce<number | undefined>(
    (current, id) => (current === undefined || id > current ? id : current),
    undefined,
  );
}

function minOf(ids: number[]): number | undefined {
  return ids.reduce<number | undefined>(
    (current, id) => (current === undefined || id < current ? id : current),
    undefined,
  );
}

function columnContentSignature(columns: Column[]): string {
  return JSON.stringify(
    columns.map((column) => {
      const { width: _width, ...content } = column;
      return content;
    }),
  );
}

function sameFilter(left: SessionFilter, right: SessionFilter): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

function evictRows(): void {
  const protectedIds = new Set(viewportIds);
  for (const id of proxyStore.activeNetlogIds(sessionsStore.viewingSessionId)) {
    protectedIds.add(id);
  }
  for (const id of rowLru.evict(protectedIds)) {
    logsStore.rowsById.delete(id);
    for (const key of logsStore.cellErrors.keys()) {
      if (key.startsWith(`${id}:`)) logsStore.cellErrors.delete(key);
    }
  }
}

function clearPollTimer(): void {
  if (pollTimer !== undefined) {
    window.clearTimeout(pollTimer);
    pollTimer = undefined;
  }
}

function schedulePoll(delay?: number): void {
  clearPollTimer();
  if (!pollingEnabled || sessionsStore.viewingSessionId === null) return;
  const hasWork = logsStore.hasActive || pendingFilterIds.size > 0;
  const discoversNew = sessionsStore.viewingSessionId === sessionsStore.activeSessionId;
  if (!hasWork && !discoversNew) return;
  pollTimer = window.setTimeout(
    async () => {
      pollTimer = undefined;
      await logsStore.poll();
      schedulePoll();
    },
    delay ?? (hasWork ? POLL_INTERVAL : IDLE_POLL_INTERVAL),
  );
}

function wakePoll(): void {
  if (pollingEnabled) schedulePoll(0);
}

export function emptySessionFilter(): SessionFilter {
  return { option: null, input: "" };
}

export function cloneSessionFilter(filter: SessionFilter): SessionFilter {
  return JSON.parse(JSON.stringify(filter)) as SessionFilter;
}

export const logsStore = reactive({
  columns: [] as Column[],
  /** Complete matching ID set, retained newest first. */
  ids: [] as number[],
  rowsById: new Map<number, LogViewRow>(),
  cellErrors: new Map<string, string>(),
  sortDesc: true,
  appliedFilter: emptySessionFilter() as SessionFilter,
  loading: false,

  get filterActive(): boolean {
    return this.appliedFilter.option !== null && this.appliedFilter.input.length > 0;
  },

  get hasActive(): boolean {
    return proxyStore.activeNetlogCount(sessionsStore.viewingSessionId) > 0;
  },

  get sortedIds(): number[] {
    return this.sortDesc ? this.ids : [...this.ids].reverse();
  },

  row(id: number): LogViewRow {
    const cached = this.rowsById.get(id);
    if (cached) {
      rowLru.touch(id);
      return cached;
    }
    return {
      id,
      created_at: 0,
      updated_at: 0,
      outcome: proxyStore.isNetlogActive(sessionsStore.viewingSessionId, id)
        ? "in_progress"
        : "success",
      cells: this.columns.map(() => "…"),
    };
  },

  syncAppliedFilter(filter: SessionFilter): void {
    if (!sameFilter(this.appliedFilter, filter)) {
      this.appliedFilter = cloneSessionFilter(filter);
    }
  },

  applyColumns(columns: Column[], forceContentRefresh = false): void {
    const contentChanged =
      forceContentRefresh ||
      columnContentSignature(this.columns) !== columnContentSignature(columns);
    this.columns = columns;
    if (contentChanged) {
      rowGeneration += 1;
      this.clearRows();
    }
  },

  clearRows(): void {
    this.rowsById = new Map();
    this.cellErrors = new Map();
    rowLru.clear();
  },

  clearIds(): void {
    this.ids = [];
    maxId = 0;
    pendingFilterIds.clear();
  },

  reset(): void {
    generation += 1;
    rowGeneration += 1;
    this.columns = [];
    this.clearIds();
    this.clearRows();
    this.appliedFilter = emptySessionFilter();
    this.sortDesc = true;
    this.loading = false;
    viewportIds.clear();
  },

  mergeIds(incoming: number[]): void {
    if (incoming.length > 0) this.ids = sortedUnique([...this.ids, ...incoming]);
  },

  removeLog(id: number): void {
    this.ids = this.ids.filter((item) => item !== id);
    this.rowsById.delete(id);
    pendingFilterIds.delete(id);
    rowLru.delete(id);
    for (const key of this.cellErrors.keys()) {
      if (key.startsWith(`${id}:`)) this.cellErrors.delete(key);
    }
  },

  mergeViews(payload: LogViewsPayload, requestedIds: number[]): void {
    if (this.columns.length === 0) this.applyColumns(payload.columns);
    const returnedIds = new Set(payload.rows.map((row) => row.id));
    for (const id of returnedIds) {
      for (const key of this.cellErrors.keys()) {
        if (key.startsWith(`${id}:`)) this.cellErrors.delete(key);
      }
    }
    for (const row of payload.rows) {
      this.rowsById.set(row.id, row);
      rowLru.touch(row.id);
    }
    for (const id of requestedIds) {
      if (this.rowsById.has(id)) rowLru.touch(id);
    }
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
    evictRows();
  },

  async hydrateNow(
    sessionId: number,
    targetGeneration: number,
    targetRowGeneration: number,
    ids: number[],
    force: boolean,
  ): Promise<void> {
    if (
      ids.length === 0 ||
      sessionsStore.viewingSessionId !== sessionId ||
      generation !== targetGeneration ||
      rowGeneration !== targetRowGeneration
    ) {
      return;
    }
    for (let index = 0; index < ids.length; index += VIEW_BATCH_SIZE) {
      if (
        sessionsStore.viewingSessionId !== sessionId ||
        generation !== targetGeneration ||
        rowGeneration !== targetRowGeneration
      ) {
        return;
      }
      const batch = ids.slice(index, index + VIEW_BATCH_SIZE);
      const payload = await backend.getLogViews({
        session_id: sessionId,
        logs: batch.map((id) => {
          const updatedAt = this.rowsById.get(id)?.updated_at;
          return force || updatedAt === undefined ? { id } : { id, updated_at: updatedAt };
        }),
        view: { columns: this.columns },
      });
      if (
        sessionsStore.viewingSessionId !== sessionId ||
        generation !== targetGeneration ||
        rowGeneration !== targetRowGeneration
      ) {
        return;
      }
      this.mergeViews(payload, batch);
    }
  },

  async hydrate(ids: number[], force = false): Promise<void> {
    const sessionId = sessionsStore.viewingSessionId;
    if (sessionId === null || ids.length === 0) return;
    await this.hydrateFor(sessionId, generation, ids, force);
  },

  async hydrateFor(
    sessionId: number,
    targetGeneration: number,
    ids: number[],
    force = false,
  ): Promise<void> {
    if (ids.length === 0) return;
    const unique = [...new Set(ids)];
    const targetRowGeneration = rowGeneration;
    const request = hydrateQueue.then(() =>
      this.hydrateNow(
        sessionId,
        targetGeneration,
        targetRowGeneration,
        unique,
        force,
      ),
    );
    hydrateQueue = request.catch(() => undefined);
    await request;
  },

  setViewportIds(ids: number[]): void {
    viewportIds.clear();
    for (const id of ids) {
      viewportIds.add(id);
      if (this.rowsById.has(id)) rowLru.touch(id);
    }
    evictRows();
    if (viewportTimer !== undefined) window.clearTimeout(viewportTimer);
    viewportTimer = window.setTimeout(() => {
      viewportTimer = undefined;
      const missing = [...viewportIds].filter((id) => !this.rowsById.has(id));
      void this.hydrate(missing).catch(() => undefined);
    }, 0);
  },

  applyCandidateResult(
    candidates: number[],
    matchedIds: number[],
    inProgressIds: number[],
  ): void {
    const displayed = new Set(this.ids);
    const matched = new Set(matchedIds);
    const inProgress = new Set(inProgressIds);
    for (const id of candidates) {
      if (matched.has(id)) displayed.add(id);
      else displayed.delete(id);
      if (inProgress.has(id)) pendingFilterIds.add(id);
      else pendingFilterIds.delete(id);
    }
    this.ids = sortedUnique(displayed);
  },

  async filterCandidates(
    sessionId: number,
    targetGeneration: number,
    candidates: number[],
  ): Promise<void> {
    for (let index = 0; index < candidates.length; index += ID_PAGE_SIZE) {
      if (sessionsStore.viewingSessionId !== sessionId || generation !== targetGeneration) {
        return;
      }
      const batch = candidates.slice(index, index + ID_PAGE_SIZE);
      const payload = await backend.getLogIds({
        session_id: sessionId,
        filter: cloneSessionFilter(this.appliedFilter),
        ids: batch,
      });
      if (sessionsStore.viewingSessionId !== sessionId || generation !== targetGeneration) {
        return;
      }
      this.applyCandidateResult(batch, payload.matched_ids, payload.in_progress_ids);
    }
  },

  async loadAllIds(
    sessionId: number,
    targetGeneration: number,
    filter: SessionFilter,
  ): Promise<void> {
    const latest = await backend.getLogIds({
      session_id: sessionId,
      filter: emptySessionFilter(),
      limit: 1,
    });
    if (sessionsStore.viewingSessionId !== sessionId || generation !== targetGeneration) return;
    const initialMaxId = latest.matched_ids[0] ?? 0;
    let cursor = initialMaxId > 0 ? initialMaxId + 1 : undefined;
    const matched = new Set<number>();
    const pending = new Set<number>();
    while (true) {
      const payload = await backend.getLogIds({
        session_id: sessionId,
        filter: cloneSessionFilter(filter),
        max_id: cursor,
        limit: ID_PAGE_SIZE,
      });
      if (sessionsStore.viewingSessionId !== sessionId || generation !== targetGeneration) return;
      for (const id of payload.matched_ids) matched.add(id);
      for (const id of payload.in_progress_ids) pending.add(id);
      if (payload.matched_ids.length < ID_PAGE_SIZE) break;
      cursor = minOf(payload.matched_ids);
    }
    this.ids = sortedUnique(matched);
    pendingFilterIds.clear();
    for (const id of pending) pendingFilterIds.add(id);
    maxId = initialMaxId;
  },

  async loadSession(): Promise<void> {
    const sessionId = sessionsStore.viewingSessionId;
    if (sessionId === null) return;
    const targetGeneration = ++generation;
    this.loading = true;
    this.columns = [];
    this.clearIds();
    this.clearRows();
    try {
      const view = await backend.getSessionView(sessionId);
      if (sessionsStore.viewingSessionId !== sessionId || generation !== targetGeneration) return;
      this.applyColumns(view.columns);
      this.syncAppliedFilter(view.filter);
      await this.loadAllIds(sessionId, targetGeneration, view.filter);
    } catch (error) {
      if (sessionsStore.viewingSessionId === sessionId && generation === targetGeneration) {
        reportError(error, "加载会话记录失败");
      }
    } finally {
      if (sessionsStore.viewingSessionId === sessionId && generation === targetGeneration) {
        this.loading = false;
        wakePoll();
      }
    }
  },

  async reloadFilter(filter: SessionFilter): Promise<boolean> {
    const sessionId = sessionsStore.viewingSessionId;
    if (sessionId === null) return false;
    const targetGeneration = ++generation;
    this.loading = true;
    this.syncAppliedFilter(filter);
    this.clearIds();
    try {
      await this.loadAllIds(sessionId, targetGeneration, filter);
      return sessionsStore.viewingSessionId === sessionId && generation === targetGeneration;
    } catch (error) {
      if (sessionsStore.viewingSessionId === sessionId && generation === targetGeneration) {
        reportError(error, "过滤失败");
      }
      return false;
    } finally {
      if (sessionsStore.viewingSessionId === sessionId && generation === targetGeneration) {
        this.loading = false;
        wakePoll();
      }
    }
  },

  async applyFilter(filter: SessionFilter): Promise<boolean> {
    const sessionId = sessionsStore.viewingSessionId;
    if (sessionId === null) return false;
    const targetGeneration = generation;
    const previous = cloneSessionFilter(this.appliedFilter);
    this.syncAppliedFilter(filter);
    if (!backend.capabilities.readonly) {
      try {
        await backend.replaceSessionFilter(sessionId, cloneSessionFilter(filter));
        if (
          sessionsStore.viewingSessionId !== sessionId ||
          generation !== targetGeneration
        ) {
          return false;
        }
      } catch (error) {
        if (
          sessionsStore.viewingSessionId === sessionId &&
          generation === targetGeneration
        ) {
          this.syncAppliedFilter(previous);
          reportError(error, "保存过滤条件失败");
        }
        return false;
      }
    }
    return this.reloadFilter(filter);
  },

  async syncSessionView(): Promise<void> {
    const sessionId = sessionsStore.viewingSessionId;
    if (sessionId === null) return;
    try {
      const view = await backend.getSessionView(sessionId);
      if (sessionsStore.viewingSessionId !== sessionId) return;
      this.applyColumns(view.columns);
      if (!backend.capabilities.readonly && !sameFilter(this.appliedFilter, view.filter)) {
        await this.reloadFilter(view.filter);
      } else {
        this.setViewportIds([...viewportIds]);
      }
    } catch (error) {
      reportError(error, "同步 Session 视图失败");
    }
  },

  async discoverNew(sessionId: number, targetGeneration: number): Promise<void> {
    while (sessionsStore.viewingSessionId === sessionId && generation === targetGeneration) {
      const payload = await backend.getLogIds({
        session_id: sessionId,
        filter: emptySessionFilter(),
        min_id: maxId,
        limit: ID_PAGE_SIZE,
      });
      if (sessionsStore.viewingSessionId !== sessionId || generation !== targetGeneration) return;
      const rawIds = payload.matched_ids;
      if (rawIds.length === 0) return;
      if (this.filterActive) {
        await this.filterCandidates(sessionId, targetGeneration, rawIds);
        if (sessionsStore.viewingSessionId !== sessionId || generation !== targetGeneration) return;
      } else {
        this.mergeIds(rawIds);
      }
      maxId = maxOf(rawIds) ?? maxId;
      if (rawIds.length < ID_PAGE_SIZE) return;
    }
  },

  async refreshFilterCandidates(sessionId: number, targetGeneration: number): Promise<void> {
    if (!this.filterActive) {
      pendingFilterIds.clear();
      return;
    }
    const candidates = sortedUnique([
      ...proxyStore.activeNetlogIds(sessionId),
      ...pendingFilterIds,
    ]);
    await this.filterCandidates(sessionId, targetGeneration, candidates);
  },

  async poll(): Promise<void> {
    const sessionId = sessionsStore.viewingSessionId;
    if (sessionId === null || this.loading) return;
    if (pollInFlight) {
      pollAgain = true;
      return;
    }
    pollInFlight = true;
    const targetGeneration = generation;
    try {
      if (sessionsStore.activeSessionId === sessionId) {
        await this.discoverNew(sessionId, targetGeneration);
      }
      if (sessionsStore.viewingSessionId !== sessionId || generation !== targetGeneration) return;
      await this.refreshFilterCandidates(sessionId, targetGeneration);
      if (sessionsStore.viewingSessionId !== sessionId || generation !== targetGeneration) return;
      const activeIds = proxyStore.activeNetlogIds(sessionId);
      if (activeIds.length > 0) {
        await this.hydrateFor(sessionId, targetGeneration, activeIds);
      }
    } catch {
      // Polling failures are transient; the next tick retries from the same maxId.
    } finally {
      pollInFlight = false;
      if (pollAgain) {
        pollAgain = false;
        void this.poll();
      }
    }
  },

  async finalize(ids: number[]): Promise<void> {
    const sessionId = sessionsStore.viewingSessionId;
    if (sessionId === null || ids.length === 0) return;
    const targetGeneration = generation;
    try {
      if (this.filterActive) {
        await this.filterCandidates(sessionId, targetGeneration, ids);
      }
      if (sessionsStore.viewingSessionId !== sessionId || generation !== targetGeneration) return;
      await this.hydrateFor(sessionId, targetGeneration, ids, true);
    } catch {
      if (sessionsStore.viewingSessionId === sessionId && generation === targetGeneration) {
        for (const id of ids) pendingFilterIds.add(id);
      }
    } finally {
      wakePoll();
    }
  },

  async refreshView(): Promise<void> {
    const sessionId = sessionsStore.viewingSessionId;
    if (sessionId === null) return;
    try {
      const view = await backend.getSessionView(sessionId);
      if (sessionsStore.viewingSessionId !== sessionId) return;
      this.applyColumns(view.columns, true);
      this.setViewportIds([...viewportIds]);
      await this.hydrate(proxyStore.activeNetlogIds(sessionId), true);
    } catch (error) {
      reportError(error, "刷新日志列失败");
    }
  },

  cellError(id: number, columnIndex: number): string | undefined {
    return this.cellErrors.get(cellErrorKey(id, columnIndex));
  },

  startPolling(): void {
    pollingEnabled = true;
    schedulePoll(0);
  },

  stopPolling(): void {
    pollingEnabled = false;
    clearPollTimer();
  },
});

const stopViewingSessionWatch = watch(
  () => sessionsStore.viewingSessionId,
  (id, previous) => {
    if (id === previous) return;
    logsStore.reset();
    if (id !== null) void logsStore.loadSession();
  },
  { immediate: true },
);

const stopActivityWatch = watch(
  () => {
    const sessionId = sessionsStore.viewingSessionId;
    return { sessionId, ids: proxyStore.activeNetlogIds(sessionId) };
  },
  (current, previous) => {
    if (current.sessionId === null || current.sessionId !== previous.sessionId) return;
    const currentIds = new Set(current.ids);
    const removed = previous.ids.filter((id) => !currentIds.has(id));
    if (removed.length > 0) void logsStore.finalize(removed);
    evictRows();
    void logsStore.poll();
    wakePoll();
  },
  { deep: true },
);

if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    stopViewingSessionWatch();
    stopActivityWatch();
    logsStore.stopPolling();
    if (viewportTimer !== undefined) window.clearTimeout(viewportTimer);
    generation += 1;
  });
}
