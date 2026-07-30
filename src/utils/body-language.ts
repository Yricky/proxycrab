import type { BodyPayload, HeaderItem } from "../api/types";

function contentType(headers: HeaderItem[]): string {
  return (
    headers
      .find((header) => header.name.toLowerCase() === "content-type")
      ?.value.split(";", 1)[0]
      ?.trim()
      .toLowerCase() ?? ""
  );
}

function contentTypeLanguage(mediaType: string): string | null {
  if (mediaType === "application/json" || mediaType.endsWith("+json")) return "json";
  if (mediaType === "text/html" || mediaType === "application/xhtml+xml") return "html";
  if (
    mediaType === "application/xml" ||
    mediaType === "text/xml" ||
    mediaType.endsWith("+xml") ||
    mediaType === "image/svg+xml"
  ) {
    return "xml";
  }

  switch (mediaType) {
    case "text/css":
      return "css";
    case "application/javascript":
    case "application/ecmascript":
    case "text/javascript":
    case "text/ecmascript":
      return "javascript";
    case "application/typescript":
    case "text/typescript":
      return "typescript";
    case "text/markdown":
      return "markdown";
    case "application/yaml":
    case "application/x-yaml":
    case "text/yaml":
    case "text/x-yaml":
      return "yaml";
    case "application/graphql":
      return "graphql";
    case "application/sql":
    case "text/x-sql":
      return "sql";
    case "text/x-python":
      return "python";
    case "application/x-sh":
    case "text/x-shellscript":
      return "shell";
    default:
      return null;
  }
}

function sniffTextLanguage(content: string): string {
  const trimmed = content.trimStart();
  if (trimmed.startsWith("{") || trimmed.startsWith("[")) return "json";
  if (/^<!doctype\s+html\b/i.test(trimmed) || /^<html\b/i.test(trimmed)) return "html";
  if (/^<\?xml\b/i.test(trimmed)) return "xml";
  return "plaintext";
}

export function bodyLanguage(body: BodyPayload, headers: HeaderItem[]): string {
  if (body.type === "json") return "json";
  if (body.type !== "text") return "plaintext";
  return contentTypeLanguage(contentType(headers)) ?? sniffTextLanguage(body.content);
}
