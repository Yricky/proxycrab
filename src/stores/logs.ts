import { effectScope, reactive, watch } from "vue";
import { runtimeBackend as backend } from "../api/runtime-backend";
import type {
  HttpApiChange,
  Column,
  LogViewRow,
  LogViewsPayload,
  SessionFilter,
} from "../api/types";
import { LruKeys } from "../utils/lru";
import { reportError } from "./app";
import { proxyStore } from "./proxy";
import { sessionsStore } from "./sessions";
import { HTTP_API_CHANGE_EVENT } from "./http-api-sync";

export function emptySessionFilter(): SessionFilter {
  return { option: null, input: "" };
}

export function cloneSessionFilter(filter: SessionFilter): SessionFilter {
  return JSON.parse(JSON.stringify(filter)) as SessionFilter;
}

/** 创建一份日志视图状态（会话自持，事件自行监听）。 */
export function createLogsStore() {
  const VIEW_BATCH_SIZE = 200;
  const ID_PAGE_SIZE = 5_000;
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
  let viewportQueued = false;
  let viewportRunning = false;
  let viewportDirty = false;
  let lastViewportMid: number | null = null;
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
    for (const id of proxyStore.activeNetlogIds(logsStore.sessionId)) {
      protectedIds.add(id);
    }
    for (const id of rowLru.evict(protectedIds)) {
      logsStore.rowsById.delete(id);
      for (const key of logsStore.cellErrors.keys()) {
        if (key.startsWith(`${id}:`)) logsStore.cellErrors.delete(key);
      }
    }
  }

  function indexOfId(sorted: number[], id: number): number {
    let low = 0;
    let high = sorted.length - 1;
    const ascending = sorted.length < 2 || sorted[low] <= sorted[high];
    while (low <= high) {
      const mid = (low + high) >> 1;
      const value = sorted[mid];
      if (value === id) return mid;
      if (value < id === ascending) low = mid + 1;
      else high = mid - 1;
    }
    return -1;
  }

  function queueViewportHydrate(): void {
    viewportDirty = true;
    if (viewportQueued || viewportRunning) return;
    viewportQueued = true;
    const run = hydrateQueue.then(async () => {
      viewportQueued = false;
      viewportRunning = true;
      viewportDirty = false;
      try {
        await logsStore.loadViewportWindow();
      } finally {
        viewportRunning = false;
        if (viewportDirty) queueViewportHydrate();
      }
    });
    hydrateQueue = run.catch(() => undefined);
  }

  function clearPollTimer(): void {
    if (pollTimer !== undefined) {
      window.clearTimeout(pollTimer);
      pollTimer = undefined;
    }
  }

  function schedulePoll(delay?: number): void {
    clearPollTimer();
    if (!pollingEnabled || logsStore.sessionId === null) return;
    const hasWork = logsStore.hasActive || pendingFilterIds.size > 0;
    const discoversNew = logsStore.sessionId === sessionsStore.activeSessionId;
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

  const logsStore = reactive({
    /** 本实例展示的会话；由持有实例的组件用 view() 切换。 */
    sessionId: null as number | null,
    columns: [] as Column[],
    /** Complete matching ID set, retained newest first. */
    ids: [] as number[],
    rowsById: new Map<number, LogViewRow>(),
    cellErrors: new Map<string, string>(),
    sortDesc: true,
    appliedFilter: emptySessionFilter() as SessionFilter,
    loading: false,
    /** ID 扫描进度（0..1）；null 表示尚未探测到最大 ID，加载遮罩显示 spinner。 */
    loadProgress: null as number | null,

    get filterActive(): boolean {
      return this.appliedFilter.option !== null && this.appliedFilter.input.length > 0;
    },

    get hasActive(): boolean {
      return proxyStore.activeNetlogCount(logsStore.sessionId) > 0;
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
        outcome: proxyStore.isNetlogActive(logsStore.sessionId, id)
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
      lastViewportMid = null;
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
      this.loadProgress = null;
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
      for (let index = 0; index < ids.length; index += VIEW_BATCH_SIZE) {
        if (
          logsStore.sessionId !== sessionId ||
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
          logsStore.sessionId !== sessionId ||
          generation !== targetGeneration ||
          rowGeneration !== targetRowGeneration
        ) {
          return;
        }
        this.mergeViews(payload, batch);
      }
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
      queueViewportHydrate();
    },

    async loadViewportWindow(): Promise<void> {
      const sessionId = logsStore.sessionId;
      if (sessionId === null || viewportIds.size === 0) return;
      const targetGeneration = generation;
      const targetRowGeneration = rowGeneration;
      const displayed = this.sortedIds;
      let visibleStart = -1;
      let visibleEnd = -1;
      for (const id of viewportIds) {
        const index = indexOfId(displayed, id);
        if (index < 0) continue;
        if (visibleStart < 0 || index < visibleStart) visibleStart = index;
        if (index > visibleEnd) visibleEnd = index;
      }
      if (visibleStart < 0) return;
      const mid = (visibleStart + visibleEnd) / 2;
      const scrollingUp = lastViewportMid !== null && mid < lastViewportMid;
      lastViewportMid = mid;
      let windowIds: number[];
      if (scrollingUp) {
        // 向上：在可视区底部向前 200 条内找最后一个未缓存位置，从那里向前顺取 200 条
        let frontier = -1;
        const limit = Math.max(0, visibleEnd + 1 - VIEW_BATCH_SIZE);
        for (let index = visibleEnd; index >= limit; index -= 1) {
          if (!this.rowsById.has(displayed[index])) {
            frontier = index;
            break;
          }
        }
        windowIds =
          frontier < 0
            ? []
            : displayed.slice(Math.max(0, frontier + 1 - VIEW_BATCH_SIZE), frontier + 1);
      } else {
        // 向下（含初始加载）：在可视区顶部向后 200 条内找第一个未缓存位置，从那里向后顺取 200 条
        let frontier = -1;
        const limit = Math.min(displayed.length, visibleStart + VIEW_BATCH_SIZE);
        for (let index = visibleStart; index < limit; index += 1) {
          if (!this.rowsById.has(displayed[index])) {
            frontier = index;
            break;
          }
        }
        windowIds = frontier < 0 ? [] : displayed.slice(frontier, frontier + VIEW_BATCH_SIZE);
      }
      const missing = windowIds.filter((id) => !this.rowsById.has(id));
      if (missing.length > 0) {
        const payload = await backend.getLogViews({
          session_id: sessionId,
          logs: missing.map((id) => ({ id })),
          view: { columns: this.columns },
        });
        if (
          logsStore.sessionId !== sessionId ||
          generation !== targetGeneration ||
          rowGeneration !== targetRowGeneration
        ) {
          return;
        }
        this.mergeViews(payload, missing);
      }
      for (const id of windowIds) {
        if (this.rowsById.has(id)) rowLru.touch(id);
      }
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
        if (logsStore.sessionId !== sessionId || generation !== targetGeneration) {
          return;
        }
        const batch = candidates.slice(index, index + ID_PAGE_SIZE);
        const payload = await backend.getLogIds({
          session_id: sessionId,
          filter: cloneSessionFilter(this.appliedFilter),
          ids: batch,
        });
        if (logsStore.sessionId !== sessionId || generation !== targetGeneration) {
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
      if (logsStore.sessionId !== sessionId || generation !== targetGeneration) return;
      const initialMaxId = latest.matched_ids[0] ?? 0;
      let cursor = initialMaxId > 0 ? initialMaxId + 1 : undefined;
      this.loadProgress = initialMaxId > 0 ? 0 : null;
      const matched = new Set<number>();
      const pending = new Set<number>();
      while (true) {
        const payload = await backend.getLogIds({
          session_id: sessionId,
          filter: cloneSessionFilter(filter),
          max_id: cursor,
          limit: ID_PAGE_SIZE,
        });
        if (logsStore.sessionId !== sessionId || generation !== targetGeneration) return;
        for (const id of payload.matched_ids) matched.add(id);
        for (const id of payload.in_progress_ids) pending.add(id);
        const minMatched = minOf(payload.matched_ids);
        if (initialMaxId > 0) {
          // 粗略扫描进度：cursor 在 ID 空间中的位置，单调递增并封顶 100%。
          this.loadProgress =
            minMatched === undefined
              ? 1
              : Math.min(1, Math.max(0, (initialMaxId - minMatched + 1) / initialMaxId));
        }
        if (payload.matched_ids.length < ID_PAGE_SIZE) break;
        cursor = minMatched;
      }
      this.ids = sortedUnique(matched);
      pendingFilterIds.clear();
      for (const id of pending) pendingFilterIds.add(id);
      maxId = initialMaxId;
    },

    async loadSession(): Promise<void> {
      const sessionId = logsStore.sessionId;
      if (sessionId === null) return;
      const targetGeneration = ++generation;
      this.loading = true;
      this.loadProgress = null;
      this.columns = [];
      this.clearIds();
      this.clearRows();
      try {
        const view = await backend.getSessionView(sessionId);
        if (logsStore.sessionId !== sessionId || generation !== targetGeneration) return;
        this.applyColumns(view.columns);
        this.syncAppliedFilter(view.filter);
        await this.loadAllIds(sessionId, targetGeneration, view.filter);
      } catch (error) {
        if (logsStore.sessionId === sessionId && generation === targetGeneration) {
          reportError(error, "加载会话记录失败");
        }
      } finally {
        if (logsStore.sessionId === sessionId && generation === targetGeneration) {
          this.loading = false;
          wakePoll();
        }
      }
    },

    async reloadFilter(filter: SessionFilter): Promise<boolean> {
      const sessionId = logsStore.sessionId;
      if (sessionId === null) return false;
      const targetGeneration = ++generation;
      this.loading = true;
      this.loadProgress = null;
      this.syncAppliedFilter(filter);
      this.clearIds();
      try {
        await this.loadAllIds(sessionId, targetGeneration, filter);
        return logsStore.sessionId === sessionId && generation === targetGeneration;
      } catch (error) {
        if (logsStore.sessionId === sessionId && generation === targetGeneration) {
          reportError(error, "过滤失败");
        }
        return false;
      } finally {
        if (logsStore.sessionId === sessionId && generation === targetGeneration) {
          this.loading = false;
          wakePoll();
        }
      }
    },

    /**
     * 应用过滤。主内容区是唯一"同步到后端"的过滤面：写回 Session 配置，并接受后端
     * 后续的过滤变更；没有 filterPersistence 能力的面（分享页）只本地生效。
     */
    async applyFilter(filter: SessionFilter): Promise<boolean> {
      const sessionId = logsStore.sessionId;
      if (sessionId === null) return false;
      const targetGeneration = generation;
      const previous = cloneSessionFilter(this.appliedFilter);
      this.syncAppliedFilter(filter);
      if (backend.capabilities.filterPersistence) {
        try {
          await backend.replaceSessionFilter(sessionId, cloneSessionFilter(filter));
          if (
            logsStore.sessionId !== sessionId ||
            generation !== targetGeneration
          ) {
            return false;
          }
        } catch (error) {
          if (
            logsStore.sessionId === sessionId &&
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
      const sessionId = logsStore.sessionId;
      if (sessionId === null) return;
      try {
        const view = await backend.getSessionView(sessionId);
        if (logsStore.sessionId !== sessionId) return;
        this.applyColumns(view.columns);
        // 不同步的面不跟随后端过滤变更：Session 保存的过滤只在加载时当初始值。
        if (
          backend.capabilities.filterPersistence &&
          !sameFilter(this.appliedFilter, view.filter)
        ) {
          await this.reloadFilter(view.filter);
        } else {
          this.setViewportIds([...viewportIds]);
        }
      } catch (error) {
        reportError(error, "同步 Session 视图失败");
      }
    },

    async discoverNew(sessionId: number, targetGeneration: number): Promise<void> {
      while (logsStore.sessionId === sessionId && generation === targetGeneration) {
        const payload = await backend.getLogIds({
          session_id: sessionId,
          filter: emptySessionFilter(),
          min_id: maxId,
          limit: ID_PAGE_SIZE,
        });
        if (logsStore.sessionId !== sessionId || generation !== targetGeneration) return;
        const rawIds = payload.matched_ids;
        if (rawIds.length === 0) return;
        if (this.filterActive) {
          await this.filterCandidates(sessionId, targetGeneration, rawIds);
          if (logsStore.sessionId !== sessionId || generation !== targetGeneration) return;
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
      const sessionId = logsStore.sessionId;
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
        if (logsStore.sessionId !== sessionId || generation !== targetGeneration) return;
        await this.refreshFilterCandidates(sessionId, targetGeneration);
        if (logsStore.sessionId !== sessionId || generation !== targetGeneration) return;
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
      const sessionId = logsStore.sessionId;
      if (sessionId === null || ids.length === 0) return;
      const targetGeneration = generation;
      try {
        if (this.filterActive) {
          await this.filterCandidates(sessionId, targetGeneration, ids);
        }
        if (logsStore.sessionId !== sessionId || generation !== targetGeneration) return;
        await this.hydrateFor(sessionId, targetGeneration, ids, true);
      } catch {
        if (logsStore.sessionId === sessionId && generation === targetGeneration) {
          for (const id of ids) pendingFilterIds.add(id);
        }
      } finally {
        wakePoll();
      }
    },

    async refreshView(): Promise<void> {
      const sessionId = logsStore.sessionId;
      if (sessionId === null) return;
      try {
        const view = await backend.getSessionView(sessionId);
        if (logsStore.sessionId !== sessionId) return;
        this.applyColumns(view.columns, true);
        this.setViewportIds([...viewportIds]);
        await this.hydrateFor(
          sessionId,
          generation,
          proxyStore.activeNetlogIds(sessionId),
          true,
        );
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

    view(id: number | null): void {
      this.sessionId = id;
    },

    /** 持有实例的组件卸载时调用：停止轮询、收起内部 watcher 与事件监听。 */
    dispose(): void {
      this.stopPolling();
      scope.stop();
      window.removeEventListener(HTTP_API_CHANGE_EVENT, onHttpApiChange);
      window.removeEventListener("column-scripts-changed", onColumnScriptsChanged);
      window.removeEventListener("filter-scripts-changed", onFilterScriptsChanged);
    },
  });


  function onHttpApiChange(event: Event): void {
    const change = (event as CustomEvent<HttpApiChange>).detail;
    // 后端事件由 http-api-sync 统一转成前端事件，实例只关心自己这个会话。
    if (change.session_id !== undefined && change.session_id !== logsStore.sessionId) return;
    if (change.resources.includes("all") || change.resources.includes("session_view")) {
      void logsStore.syncSessionView();
    }
  }

  // 列脚本改变的是单元格内容，重算视图即可；过滤脚本会改变匹配结果，需要整体重扫。
  function onColumnScriptsChanged(): void {
    void logsStore.refreshView();
  }

  function onFilterScriptsChanged(): void {
    void logsStore.loadSession();
  }

  const scope = effectScope();
  scope.run(() => {
    watch(
      () => logsStore.sessionId,
      (id, previous) => {
        if (id === previous) return;
        logsStore.reset();
        if (id !== null) void logsStore.loadSession();
      },
      { immediate: true },
    );

    watch(
      () => ({
        sessionId: logsStore.sessionId,
        ids: proxyStore.activeNetlogIds(logsStore.sessionId),
      }),
      (current, previous) => {
        if (current.sessionId === null || current.sessionId !== previous.sessionId) return;
        const currentIds = new Set(current.ids);
        const removed = previous.ids.filter((id) => !currentIds.has(id));
        if (removed.length > 0) void logsStore.finalize(removed);
        evictRows();
        wakePoll();
      },
      { deep: true },
    );
  });

  window.addEventListener(HTTP_API_CHANGE_EVENT, onHttpApiChange);
  window.addEventListener("column-scripts-changed", onColumnScriptsChanged);
  window.addEventListener("filter-scripts-changed", onFilterScriptsChanged);

  return logsStore;
}

/** 每个日志视图一个实例：组件创建、持有，卸载时 dispose()。 */
export type LogsStore = ReturnType<typeof createLogsStore>;
