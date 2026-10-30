export type FileKind =
  | "lua"
  | "json"
  | "html"
  | "xml"
  | "markdown"
  | "yaml"
  | "code"
  | "image"
  | "media"
  | "cert"
  | "text"
  | "file";

function extension(path: string): string {
  const name = path.slice(path.lastIndexOf("/") + 1).toLocaleLowerCase();
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(dot + 1) : "";
}

/** Same mapping drives the tree icon and the preview language, so they can never disagree. */
export function fileKind(path: string): FileKind {
  switch (extension(path)) {
    case "lua":
      return "lua";
    case "json":
    case "json5":
    case "jsonl":
    case "har":
      return "json";
    case "html":
    case "htm":
      return "html";
    case "xml":
    case "plist":
    case "graphql":
      return "xml";
    case "md":
    case "markdown":
    case "mdx":
      return "markdown";
    case "yml":
    case "yaml":
      return "yaml";
    case "js":
    case "mjs":
    case "cjs":
    case "ts":
    case "mts":
    case "css":
    case "scss":
    case "sh":
    case "py":
    case "rs":
    case "go":
      return "code";
    case "png":
    case "jpg":
    case "jpeg":
    case "gif":
    case "webp":
    case "bmp":
    case "ico":
    case "svg":
      return "image";
    case "mp4":
    case "webm":
    case "mov":
    case "mp3":
    case "wav":
    case "ogg":
    case "flac":
      return "media";
    case "pem":
    case "crt":
    case "cer":
    case "key":
    case "p12":
    case "pfx":
    case "pub":
      return "cert";
    case "txt":
    case "log":
    case "csv":
    case "ini":
    case "conf":
      return "text";
    default:
      return "file";
  }
}

const LANGUAGES: Record<string, string> = {
  lua: "lua",
  json: "json",
  html: "html",
  htm: "html",
  xml: "xml",
  plist: "xml",
  md: "markdown",
  markdown: "markdown",
  yml: "yaml",
  yaml: "yaml",
  js: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  ts: "typescript",
  mts: "typescript",
  css: "css",
  sh: "shell",
  py: "python",
  rs: "rust",
  go: "go",
  svg: "xml",
  graphql: "graphql",
};

/** Monaco language for a workspace path; unknown extensions fall back to plain text. */
export function fileLanguage(path: string): string {
  return LANGUAGES[extension(path)] ?? "plaintext";
}

/** Text kinds get a syntax-highlighted preview; everything else only gets metadata. */
export function isTextKind(kind: FileKind): boolean {
  return (
    kind === "lua" ||
    kind === "json" ||
    kind === "html" ||
    kind === "xml" ||
    kind === "markdown" ||
    kind === "yaml" ||
    kind === "code" ||
    kind === "text" ||
    kind === "cert"
  );
}
