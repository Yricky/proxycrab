import type { HttpApiChange } from "../api/generated/HttpApiChange";
import type { HttpApiResource } from "../api/generated/HttpApiResource";
import type { Backend, Unsubscribe } from "../api/backend";
import { onBeforeUnmount, onMounted } from "vue";
import { reportError } from "../stores/app";
import { t } from "../i18n";

export type ResourceKind = HttpApiResource["kind"];

export function resourceKinds(change: HttpApiChange): ResourceKind[] {
  return change.resources.map((resource) => resource.kind);
}

/** Each owner batches its own invalidations, preserving Session scopes and column names. */
export function watchBackendChanges(
  backend: Pick<Backend, "subscribeChanges">,
  handler: (change: HttpApiChange) => void | Promise<void>,
): Unsubscribe {
  let disposed = false;
  let unlisten: Unsubscribe | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let flushing = false;
  const pending = new Map<number | undefined, Map<ResourceKind, HttpApiResource>>();
  const schedule = () => {
    if (!disposed && !flushing && timer === undefined && pending.size) {
      timer = setTimeout(() => void flush(), 40);
    }
  };
  const flush = async () => {
    timer = undefined;
    flushing = true;
    const batch = new Map(pending);
    pending.clear();
    try {
      for (const [session_id, resources] of batch) {
        if (disposed) break;
        await handler({
          resources: [...resources.values()],
          ...(session_id !== undefined ? { session_id } : {}),
        });
      }
    } catch (error) {
      reportError(error, t("同步界面状态失败"));
    } finally {
      flushing = false;
      schedule();
    }
  };
  void backend
    .subscribeChanges((change) => {
      if (disposed) return;
      const resources = pending.get(change.session_id) ?? new Map<ResourceKind, HttpApiResource>();
      for (const resource of change.resources) {
        const previous = resources.get(resource.kind);
        resources.set(
          resource.kind,
          resource.kind === "column_scripts" && previous?.kind === "column_scripts"
            ? {
                kind: "column_scripts",
                names: [...new Set([...previous.names, ...resource.names])],
              }
            : resource,
        );
      }
      pending.set(change.session_id, resources);
      schedule();
    })
    .then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    })
    .catch((error) => {
      if (!disposed) reportError(error, t("订阅界面变更失败"));
    });
  return () => {
    if (disposed) return;
    disposed = true;
    unlisten?.();
    if (timer !== undefined) clearTimeout(timer);
    pending.clear();
  };
}

/** Components subscribe while mounted; stores use watchBackendChanges with their own disposal. */
export function useBackendChanges(
  backend: Pick<Backend, "subscribeChanges">,
  handler: (change: HttpApiChange) => void | Promise<void>,
): void {
  let stop: Unsubscribe | undefined;
  onMounted(() => {
    stop = watchBackendChanges(backend, handler);
  });
  onBeforeUnmount(() => stop?.());
}
