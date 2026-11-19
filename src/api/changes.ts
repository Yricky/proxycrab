import type { Unsubscribe } from "./backend";

/** One transport per Backend, owned by its current subscribers. */
export function shareChanges<T>(
  connect: (handler: (change: T) => void) => Promise<Unsubscribe>,
): (handler: (change: T) => void) => Promise<Unsubscribe> {
  const handlers = new Set<(change: T) => void>();
  let connection: Promise<Unsubscribe> | undefined;
  return async (handler) => {
    const listener = (change: T) => handler(change);
    handlers.add(listener);
    const current = (connection ??= Promise.resolve().then(() =>
      connect((change) => {
        if (connection !== current) return;
        for (const callback of handlers) {
          try {
            callback(change);
          } catch (error) {
            console.error(error);
          }
        }
      }),
    ));
    let stop: Unsubscribe;
    try {
      stop = await current;
    } catch (error) {
      handlers.delete(listener);
      if (connection === current) connection = undefined;
      throw error;
    }
    return () => {
      if (!handlers.delete(listener)) return;
      if (!handlers.size && connection === current) {
        connection = undefined;
        stop();
      }
    };
  };
}
