import type { Unsubscribe } from "../api/backend";
import { runtimeBackend as backend } from "../api/runtime-backend";
import type { HttpApiChange } from "../api/generated/HttpApiChange";
import type { HttpApiResource } from "../api/generated/HttpApiResource";
import type { ResourceKind } from "../utils/api-changes";
import { reportError } from "./app";
import { t } from "../i18n";
import { proxyStore } from "./proxy";
import { sessionsStore } from "./sessions";
import { routingStore } from "./routing";
import { bypassStore } from "./bypass";

export const HTTP_API_CHANGE_EVENT = "proxycrab-http-api-change";

let unlisten: Unsubscribe | undefined;
let flushTimer: number | undefined;
const pending = new Map<number | null, Map<ResourceKind, HttpApiResource>>();

function enqueue(change: HttpApiChange): void {
  const scope = change.session_id ?? null;
  const batch = pending.get(scope) ?? new Map<ResourceKind, HttpApiResource>();
  for (const resource of change.resources) {
    const previous = batch.get(resource.kind);
    batch.set(
      resource.kind,
      resource.kind === "column_scripts" && previous?.kind === "column_scripts"
        ? { kind: "column_scripts", names: [...new Set([...previous.names, ...resource.names])] }
        : resource,
    );
  }
  pending.set(scope, batch);
  if (flushTimer === undefined) {
    flushTimer = window.setTimeout(() => void flush(), 40);
  }
}

async function flush(): Promise<void> {
  flushTimer = undefined;
  const batch = new Map(pending);
  pending.clear();
  const kinds = new Set([...batch.values()].flatMap((resources) => [...resources.keys()]));
  const has = (kind: ResourceKind) => kinds.has("all") || kinds.has(kind);

  // 全局资源每批刷新一次；会话事件保留各自的范围，由持有会话的组件处理。
  if (has("sessions") || has("archived_sessions") || has("active_session") || has("config")) {
    await sessionsStore.syncFromBackend();
  } else if (has("session_share")) {
    await sessionsStore.refreshShares();
  }
  if (has("proxy") || has("config")) {
    await proxyStore.refresh();
  }
  if (has("routing_scripts") || has("routing_selection")) {
    await routingStore.refresh();
  }
  if (has("bypass")) {
    await bypassStore.refresh();
  }

  for (const [scope, resources] of batch) {
    const detail: HttpApiChange = {
      resources: [...resources.values()],
      ...(scope !== null ? { session_id: scope } : {}),
    };
    window.dispatchEvent(new CustomEvent<HttpApiChange>(HTTP_API_CHANGE_EVENT, { detail }));
  }
}

export async function startHttpApiSync(): Promise<void> {
  if (unlisten) return;
  try {
    unlisten = await backend.subscribeChanges(enqueue);
  } catch (error) {
    reportError(error, t("启动 HTTP API 界面同步失败"));
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
