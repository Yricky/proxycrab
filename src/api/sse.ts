import type { Unsubscribe } from "./backend";
import { BackendError } from "./backend-error";
import type { ManagerError } from "./generated/ManagerError";

/** Both browser backends keep their existing authentication on the SSE request. */
export function subscribeSse<T>(
  url: URL,
  init: RequestInit,
  handler: (value: T) => void,
  onError: (error: BackendError) => boolean = () => false,
): Unsubscribe {
  const abort = new AbortController();
  let retry: ReturnType<typeof setTimeout> | undefined;
  let reader: ReadableStreamDefaultReader<Uint8Array> | undefined;
  const run = async () => {
    try {
      const response = await fetch(url, {
        ...init,
        headers: { Accept: "text/event-stream", ...init.headers },
        signal: abort.signal,
      });
      if (abort.signal.aborted) return;
      if (!response.ok) {
        const payload = (await response.json().catch(() => null)) as {
          error?: ManagerError;
        } | null;
        const error = new BackendError(
          payload?.error ?? {
            code: `http_${response.status}`,
            message: `HTTP ${response.status}`,
          },
        );
        if (onError(error) || response.status === 401 || response.status === 403) {
          abort.abort();
          return;
        }
        throw error;
      }
      if (
        !response.headers.get("Content-Type")?.startsWith("text/event-stream") ||
        !response.body
      ) {
        throw new Error("Expected an SSE response");
      }
      reader = response.body.getReader();
      const activeReader = reader;
      const decoder = new TextDecoder();
      let buffer = "";
      try {
        while (!abort.signal.aborted) {
          const { value, done } = await activeReader.read();
          if (done || abort.signal.aborted) break;
          buffer += decoder.decode(value, { stream: true });
          let boundary: RegExpExecArray | null;
          while ((boundary = /\r?\n\r?\n/.exec(buffer))) {
            const frame = buffer.slice(0, boundary.index);
            buffer = buffer.slice(boundary.index + boundary[0].length);
            let event = "message";
            const data: string[] = [];
            for (const line of frame.split(/\r?\n/)) {
              if (line.startsWith("event:")) event = line.slice(6).trim();
              if (line.startsWith("data:")) data.push(line.slice(5).replace(/^ /, ""));
            }
            if (!data.length) continue;
            const payload = JSON.parse(data.join("\n"));
            if (event === "error") {
              if (onError(new BackendError(payload as ManagerError))) abort.abort();
            } else handler(payload as T);
            if (abort.signal.aborted) break;
          }
        }
      } finally {
        await activeReader.cancel().catch(() => undefined);
        activeReader.releaseLock();
        reader = undefined;
      }
    } catch {
      // Network failures reconnect; each new stream starts with a full invalidation.
    }
    if (!abort.signal.aborted) retry = setTimeout(() => void run(), 1000);
  };
  void run();
  return () => {
    abort.abort();
    void reader?.cancel().catch(() => undefined);
    if (retry !== undefined) clearTimeout(retry);
  };
}
