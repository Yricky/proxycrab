import { reactive } from "vue";
import type { Backend } from "../api/backend";
import type {
  CaptureOutcome,
  Column,
  FilterColumn,
  LogViewRow,
} from "../api/types";
import {
  columnGroupKey,
  copyFilterColumn,
  newestId,
  oldestId,
  type ColumnGroupSort,
} from "../utils/column-groups";
import { reportError } from "./app";
import { logsStore } from "./logs";
import { proxyStore } from "./proxy";

const ID_PAGE_SIZE = 10_000;
const VIEW_BATCH_SIZE = 200;

interface CachedGroupRow {
  createdAt: number;
  updatedAt: number;
  outcome: CaptureOutcome;
  value: string;
  error: boolean;
}

export interface ColumnGroupCache {
  key: string;
  sessionId: number;
  column: FilterColumn;
  rows: Map<number, CachedGroupRow>;
  knownIds: Set<number>;
  pendingIds: number[];
  activeIds: Set<number>;
  groups: Map<string, number>;
  emptyCount: number;
  errorCount: number;
  olderCursor: number | null;
  olderExhausted: boolean;
  search: string;
  sort: ColumnGroupSort;
  descending: boolean;
  loading: boolean;
}

export const columnGroupsState = reactive({
  caches: new Map<string, ColumnGroupCache>(),
  activeKey: null as string | null,
});

let runSequence = 0;
let pollTimer: number | undefined;

/** Only surface the progress bar once a sync actually takes longer than this. */
const LOADING_DELAY_MS = 200;
const pendingLoadingTimers = new Map<string, number>();

function scheduleLoading(cache: ColumnGroupCache, runId: number): void {
  if (cache.loading) return;
  clearScheduledLoading(cache.key);
  pendingLoadingTimers.set(
    cache.key,
    window.setTimeout(() => {
      pendingLoadingTimers.delete(cache.key);
      if (isActive(cache, runId)) cache.loading = true;
    }, LOADING_DELAY_MS),
  );
}

function clearScheduledLoading(key: string): void {
  const timer = pendingLoadingTimers.get(key);
  if (timer !== undefined) {
    window.clearTimeout(timer);
    pendingLoadingTimers.delete(key);
  }
}

function cacheKey(sessionId: number, column: FilterColumn): string {
  return `${sessionId}:${columnGroupKey(column)}`;
}

export function getColumnGroupCache(
  sessionId: number,
  column: FilterColumn,
): ColumnGroupCache {
  const key = cacheKey(sessionId, column);
  const existing = columnGroupsState.caches.get(key);
  if (existing) return existing;
  const cache: ColumnGroupCache = {
    key,
    sessionId,
    column: copyFilterColumn(column),
    rows: new Map(),
    knownIds: new Set(),
    pendingIds: [],
    activeIds: new Set(),
    groups: new Map(),
    emptyCount: 0,
    errorCount: 0,
    olderCursor: null,
    olderExhausted: false,
    search: "",
    sort: "count",
    descending: true,
    loading: false,
  };
  columnGroupsState.caches.set(key, cache);
  return columnGroupsState.caches.get(key)!;
}

function isActive(cache: ColumnGroupCache, runId: number): boolean {
  return columnGroupsState.activeKey === cache.key && runSequence === runId;
}

function asViewColumn(column: FilterColumn): Column {
  return column.kind === "script"
    ? { kind: "script", script_name: column.script_name, width: 100 }
    : { kind: column.kind, width: 100 };
}

function addContribution(cache: ColumnGroupCache, row: CachedGroupRow): void {
  if (row.error) {
    cache.errorCount += 1;
  } else if (row.value.length === 0) {
    cache.emptyCount += 1;
  } else {
    cache.groups.set(row.value, (cache.groups.get(row.value) ?? 0) + 1);
  }
}

function removeContribution(cache: ColumnGroupCache, row: CachedGroupRow): void {
  if (row.error) {
    cache.errorCount = Math.max(0, cache.errorCount - 1);
  } else if (row.value.length === 0) {
    cache.emptyCount = Math.max(0, cache.emptyCount - 1);
  } else {
    const count = cache.groups.get(row.value) ?? 0;
    if (count <= 1) cache.groups.delete(row.value);
    else cache.groups.set(row.value, count - 1);
  }
}

function replaceRow(cache: ColumnGroupCache, row: LogViewRow, error: boolean): void {
  const previous = cache.rows.get(row.id);
  if (previous) removeContribution(cache, previous);
  const next: CachedGroupRow = {
    createdAt: row.created_at,
    updatedAt: row.updated_at,
    outcome: row.outcome,
    value: row.cells[0] ?? "",
    error,
  };
  cache.rows.set(row.id, next);
  addContribution(cache, next);
}

function removeId(cache: ColumnGroupCache, id: number): void {
  const previous = cache.rows.get(id);
  if (previous) {
    removeContribution(cache, previous);
    cache.rows.delete(id);
  }
  cache.knownIds.delete(id);
}

function queueIds(cache: ColumnGroupCache, ids: number[]): void {
  for (const id of ids) {
    if (cache.knownIds.has(id)) continue;
    cache.knownIds.add(id);
    cache.pendingIds.push(id);
  }
}

async function hydrateIds(
  backend: Backend,
  cache: ColumnGroupCache,
  ids: number[],
  conditional: boolean,
  runId: number,
): Promise<void> {
  for (let index = 0; index < ids.length; index += VIEW_BATCH_SIZE) {
    if (!isActive(cache, runId)) return;
    const batch = ids.slice(index, index + VIEW_BATCH_SIZE);
    const payload = await backend.getLogViews({
      session_id: cache.sessionId,
      logs: batch.map((id) => ({
        id,
        updated_at: conditional ? cache.rows.get(id)?.updatedAt : undefined,
      })),
      view: { columns: [asViewColumn(cache.column)] },
    });
    if (!isActive(cache, runId)) return;
    const failedIds = new Set(
      payload.exceptions
        .filter((error) => error.code === "column_script_error")
        .map((error) => error.id),
    );
    for (const error of payload.exceptions) {
      if (error.code === "log_not_found") removeId(cache, error.id);
    }
    for (const row of payload.rows) replaceRow(cache, row, failedIds.has(row.id));
  }
}

async function hydratePending(
  backend: Backend,
  cache: ColumnGroupCache,
  runId: number,
): Promise<void> {
  while (cache.pendingIds.length > 0 && isActive(cache, runId)) {
    const batch = cache.pendingIds.slice(0, VIEW_BATCH_SIZE);
    await hydrateIds(backend, cache, batch, false, runId);
    if (!isActive(cache, runId)) return;
    cache.pendingIds.splice(0, batch.length);
  }
}

function maxKnownId(cache: ColumnGroupCache): number | undefined {
  let max: number | undefined;
  for (const id of cache.knownIds) {
    if (max === undefined || id > max) max = id;
  }
  return max;
}

async function discoverNew(
  backend: Backend,
  cache: ColumnGroupCache,
  runId: number,
): Promise<void> {
  let minId = maxKnownId(cache);
  if (minId === undefined) {
    cache.olderCursor = null;
    cache.olderExhausted = false;
    await discoverOlder(backend, cache, runId);
    return;
  }
  while (isActive(cache, runId)) {
    const payload = await backend.getLogIds({
      session_id: cache.sessionId,
      filter: { option: null, input: "" },
      min_id: minId,
      limit: ID_PAGE_SIZE,
    });
    if (!isActive(cache, runId)) return;
    queueIds(cache, payload.matched_ids);
    await hydratePending(backend, cache, runId);
    if (payload.matched_ids.length < ID_PAGE_SIZE) return;
    minId = newestId(payload.matched_ids)!;
  }
}

async function discoverOlder(
  backend: Backend,
  cache: ColumnGroupCache,
  runId: number,
): Promise<void> {
  while (!cache.olderExhausted && isActive(cache, runId)) {
    const payload = await backend.getLogIds({
      session_id: cache.sessionId,
      filter: { option: null, input: "" },
      max_id: cache.olderCursor,
      limit: ID_PAGE_SIZE,
    });
    if (!isActive(cache, runId)) return;
    queueIds(cache, payload.matched_ids);
    if (payload.matched_ids.length === 0) {
      cache.olderExhausted = true;
      return;
    }
    cache.olderCursor = oldestId(payload.matched_ids)!;
    cache.olderExhausted = payload.matched_ids.length < ID_PAGE_SIZE;
    await hydratePending(backend, cache, runId);
  }
}

async function refreshInProgress(
  backend: Backend,
  cache: ColumnGroupCache,
  runId: number,
): Promise<void> {
  const active = new Set(proxyStore.activeNetlogIds(cache.sessionId));
  const current = [...cache.rows.keys()].filter((id) => active.has(id));
  const completed = [...cache.activeIds].filter((id) => !active.has(id));
  await hydrateIds(backend, cache, current, true, runId);
  await hydrateIds(backend, cache, completed, false, runId);
  if (isActive(cache, runId)) cache.activeIds = active;
}

function schedulePoll(backend: Backend, cache: ColumnGroupCache, runId: number): void {
  if (!isActive(cache, runId)) return;
  pollTimer = window.setTimeout(
    () => void synchronize(backend, cache, runId, false),
    logsStore.filterActive ? 2000 : 1000,
  );
}

async function synchronize(
  backend: Backend,
  cache: ColumnGroupCache,
  runId: number,
  includeOlder: boolean,
): Promise<void> {
  if (!isActive(cache, runId)) return;
  scheduleLoading(cache, runId);
  try {
    await hydratePending(backend, cache, runId);
    await discoverNew(backend, cache, runId);
    if (includeOlder) await discoverOlder(backend, cache, runId);
    await refreshInProgress(backend, cache, runId);
  } catch (error) {
    if (isActive(cache, runId)) reportError(error, "计算列分组失败");
  } finally {
    clearScheduledLoading(cache.key);
    if (isActive(cache, runId)) {
      cache.loading = false;
      schedulePoll(backend, cache, runId);
    }
  }
}

export function openColumnGroups(
  backend: Backend,
  sessionId: number,
  column: FilterColumn,
): ColumnGroupCache {
  closeColumnGroups();
  const cache = getColumnGroupCache(sessionId, column);
  columnGroupsState.activeKey = cache.key;
  const runId = ++runSequence;
  void synchronize(backend, cache, runId, true);
  return cache;
}

export function closeColumnGroups(): void {
  runSequence += 1;
  columnGroupsState.activeKey = null;
  if (pollTimer !== undefined) {
    window.clearTimeout(pollTimer);
    pollTimer = undefined;
  }
  for (const key of [...pendingLoadingTimers.keys()]) clearScheduledLoading(key);
  for (const cache of columnGroupsState.caches.values()) cache.loading = false;
}

export function invalidateScriptColumnGroups(): void {
  const activeKey = columnGroupsState.activeKey;
  if (activeKey?.includes(":script:")) closeColumnGroups();
  for (const [key, cache] of columnGroupsState.caches) {
    if (cache.column.kind === "script") columnGroupsState.caches.delete(key);
  }
}
