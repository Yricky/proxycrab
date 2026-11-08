import type { BodySide, BodyTarget } from "../api/body";
import type { HeaderItem } from "../api/generated/HeaderItem";

const extensions: Record<string, string> = {
  "application/json": "json",
  "application/xml": "xml",
  "application/pdf": "pdf",
  "application/zip": "zip",
  "application/gzip": "gz",
  "application/javascript": "js",
  "application/x-protobuf": "pb",
  "application/protobuf": "pb",
  "application/wasm": "wasm",
  "text/plain": "txt",
  "text/html": "html",
  "text/css": "css",
  "text/csv": "csv",
  "text/xml": "xml",
  "text/javascript": "js",
  "image/png": "png",
  "image/jpeg": "jpg",
  "image/gif": "gif",
  "image/webp": "webp",
  "image/svg+xml": "svg",
  "audio/mpeg": "mp3",
  "audio/wav": "wav",
  "audio/ogg": "ogg",
  "video/mp4": "mp4",
  "video/webm": "webm",
};

function safeFilename(value: string): string {
  return (value.split(/[/\\]/).pop() ?? "")
    .replace(/[\x00-\x1f\x7f<>:"|?*]/g, "_")
    .replace(/[. ]+$/g, "")
    .trim();
}

export function bodyFilename(target: BodyTarget, side: BodySide, headers: HeaderItem[]): string {
  const header = (name: string) =>
    headers.find((item) => item.name.toLowerCase() === name)?.value ?? "";
  const disposition = header("content-disposition");
  const parameters = new Map<string, string>();
  const pattern = /;\s*([\w*]+)\s*=\s*(?:"((?:\\.|[^"\\])*)"|([^;]*))/g;
  for (const match of disposition.matchAll(pattern)) {
    parameters.set(
      match[1].toLowerCase(),
      match[2] !== undefined ? match[2].replace(/\\(.)/g, "$1") : match[3].trim(),
    );
  }
  const encoded = parameters.get("filename*");
  if (encoded) {
    const match = /^UTF-8'[^']*'(.*)$/i.exec(encoded);
    if (match) {
      try {
        const name = safeFilename(decodeURIComponent(match[1]));
        if (name) return name;
      } catch {
        /* Fall back to filename or the capture identifier. */
      }
    }
  }
  const name = safeFilename(parameters.get("filename") ?? "");
  if (name) return name;
  const media = header("content-type").split(";", 1)[0].trim().toLowerCase();
  const extension =
    extensions[media] ??
    (media.endsWith("+json") ? "json" : media.endsWith("+xml") ? "xml" : "bin");
  const prefix = target.kind === "breakpoint" ? "breakpoint" : "log";
  const execution = target.kind === "interceptor" ? `-execution-${target.executionId}` : "";
  return `${prefix}-${target.id}${execution}-${side}.${extension}`;
}
