import type { Unsubscribe } from "../api/backend";
import { runtimeBackend as backend } from "../api/runtime-backend";
import type { HttpApiChange, HttpApiResource } from "../api/types";
import { reportError } from "./app";
import { proxyStore } from "./proxy";
import { sessionsStore } from "./sessions";
import { routingStore } from "./routing";
import { bypassStore } from "./bypass";

export const HTTP_API_CHANGE_EVENT = "proxycrab-http-api-change";
const ALL_RESOURCES: HttpApiResource[] = [
  "workspace",
  "config",
  "proxy",
  "sessions",
  "archived_sessions",
  "active_session",
  "session_share",
  "session_har_share",
  "session_view",
  "column_scripts",
  "filter_scripts",
  "routing_scripts",
  "routing_selection",
  "bypass",
  "interceptors",
  "session_interceptors",
  "certificate",
  "clash_subscription",
  "system_logs",
];

let unlisten: Unsubscribe | undefined;
let flushTimer: number | undefined;
const pending = new Map<HttpApiResource, Set<number | null>>();

function enqueue(change: HttpApiChange): void {
  const resources = change.resources.includes("all") ? ALL_RESOURCES : change.resources;
  for (const resource of resources) {
    const scopes = pending.get(resource) ?? new Set<number | null>();
    scopes.add(change.session_id ?? null);
    pending.set(resource, scopes);
  }
  if (flushTimer === undefined) {
    flushTimer = window.setTimeout(() => void flush(), 40);
  }
}

async function flush(): Promise<void> {
  flushTimer = undefined;
  const batch = new Map(pending);
  pending.clear();
  const resources = [...batch.keys()];

  // 这里只处理全局资源；会话相关的刷新（日志、拦截器）由各自持有会话的
  // store/组件监听下面的前端事件后自行判断。
  if (
    batch.has("sessions") ||
    batch.has("archived_sessions") ||
    batch.has("active_session") ||
    batch.has("config")
  ) {
    await sessionsStore.syncFromBackend();
  }
  if (batch.has("proxy") || batch.has("config")) {
    await proxyStore.refresh();
  }
  if (batch.has("routing_scripts") || batch.has("routing_selection")) {
    await routingStore.refresh();
  }
  if (batch.has("bypass")) {
    await bypassStore.refresh();
  }

  const scopedIds = new Set<number>();
  for (const scopes of batch.values()) {
    for (const scope of scopes) {
      if (scope !== null) scopedIds.add(scope);
    }
  }
  const detail: HttpApiChange = {
    resources,
    ...(scopedIds.size === 1 ? { session_id: [...scopedIds][0] } : {}),
  };
  window.dispatchEvent(new CustomEvent<HttpApiChange>(HTTP_API_CHANGE_EVENT, { detail }));
}

export async function startHttpApiSync(): Promise<void> {
  if (unlisten) return;
  try {
    unlisten = await backend.subscribeChanges(enqueue);
  } catch (error) {
    reportError(error, "启动 HTTP API 界面同步失败");
  }
}

export function stopHttpApiSync(): void {
  unlisten?.();
  unlisten = undefined;
  if (flushTimer !== undefined) {
    window.clearTimeout(flushTimer);
    flushTimer = undefined;
  }
  pending.clear();
}
