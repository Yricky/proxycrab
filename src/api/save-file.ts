import type { Backend } from "./backend";
import type { SaveFileRequest } from "./types";

export interface FileWriter {
  append(data: Uint8Array<ArrayBuffer>): Promise<void>;
  markAsFinish(): Promise<void>;
  markAsError(): Promise<void>;
}

/** Serialize operations and reject writes after failure or closure. */
export function createFileWriter(operations: FileWriter): FileWriter {
  let state: "open" | "finished" | "failed" = "open";
  let pending = Promise.resolve();
  function enqueue(action: () => Promise<void>): Promise<void> {
    const result = pending.then(action);
    pending = result.catch(() => {});
    return result;
  }
  return {
    append: (data) =>
      enqueue(async () => {
        if (state !== "open") throw new Error("文件已关闭或写入失败");
        try {
          await operations.append(data);
        } catch (error) {
          state = "failed";
          throw error;
        }
      }),
    markAsFinish: () =>
      enqueue(async () => {
        if (state === "finished") return;
        if (state !== "open") throw new Error("文件写入失败，无法提交");
        try {
          await operations.markAsFinish();
          state = "finished";
        } catch (error) {
          state = "failed";
          throw error;
        }
      }),
    markAsError: () =>
      enqueue(async () => {
        if (state === "finished") return;
        state = "failed";
        await operations.markAsError();
      }),
  };
}

export function base64ToBytes(data: string): Uint8Array<ArrayBuffer> {
  const binary = atob(data);
  return Uint8Array.from(binary, (char) => char.charCodeAt(0));
}

/** Browser fallback buffers parts until finish; abort never triggers a download. */
export async function saveFileInWebview(request: SaveFileRequest): Promise<FileWriter> {
  let parts: Uint8Array<ArrayBuffer>[] = [];
  return createFileWriter({
    async append(data) {
      parts.push(data.slice());
    },
    async markAsFinish() {
      await saveBlobInWebview(new Blob(parts), request.filename);
      parts = [];
    },
    async markAsError() {
      parts = [];
    },
  });
}

/** Small existing exports share the same commit/cleanup lifecycle. */
export async function saveBytesFile(
  backend: Pick<Backend, "saveFile">,
  filename: string,
  data: Uint8Array<ArrayBuffer>,
): Promise<boolean> {
  const writer = await backend.saveFile({ filename });
  if (!writer) return false;
  try {
    await writer.append(data);
    await writer.markAsFinish();
    return true;
  } catch (error) {
    await writer.markAsError().catch(() => {});
    throw error;
  }
}

export function saveBlobInWebview(blob: Blob, filename: string): Promise<boolean> {
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  document.body.appendChild(link);
  try {
    link.click();
  } finally {
    link.remove();
    window.setTimeout(() => URL.revokeObjectURL(url), 60_000);
  }
  return Promise.resolve(true);
}
