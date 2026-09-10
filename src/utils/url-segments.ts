// URL 分段着色：按原始文本拆分（保留编码，不做重编码），逐段着色。

export interface UrlSegment {
  text: string;
  cls: string;
}

// 按原始文本拆分 query（保留编码，不做重编码），逐对着色
function querySegments(search: string): UrlSegment[] {
  const hasQuestion = search.startsWith("?");
  const raw = hasQuestion ? search.slice(1) : search;
  const pairs = raw.split("&");
  const segments: UrlSegment[] = [];
  if (hasQuestion) segments.push({ text: "?", cls: "url-query-sep" });
  pairs.forEach((pair, i) => {
    if (i > 0) segments.push({ text: "&", cls: "url-query-sep" });
    const eq = pair.indexOf("=");
    if (eq >= 0) {
      segments.push({ text: pair.slice(0, eq), cls: "url-query-key" });
      segments.push({ text: "=", cls: "url-query-eq" });
      segments.push({ text: pair.slice(eq + 1), cls: "url-query-value" });
    } else {
      segments.push({ text: pair, cls: "url-query-key" });
    }
  });
  return segments;
}

// 只有带显式 scheme（scheme://）的 URI 才交给 URL 解析器。HTTPS CONNECT 请求的
// URI 是裸 authority（如 internal-api-lark-api-usttp.larksuite.com:443），
// new URL 会把 host 误解析成 scheme、把 port 当成 opaque path，
// 导致显示成 internal-api-lark-api-usttp.larksuite.com://443。
export function parseUrlSegments(uri: string): UrlSegment[] | null {
  if (/^[a-zA-Z][a-zA-Z0-9+.-]*:\/\//.test(uri)) {
    try {
      const url = new URL(uri);
      const segments: UrlSegment[] = [
        { text: `${url.protocol}//`, cls: "url-scheme" },
        { text: url.host, cls: "url-host" },
      ];
      const path = url.pathname;
      if (path && path !== "/") segments.push({ text: path, cls: "url-path" });
      else if (path) segments.push({ text: path, cls: "url-scheme" });
      if (url.search) segments.push(...querySegments(url.search));
      if (url.hash) segments.push({ text: url.hash, cls: "url-query" });
      return segments;
    } catch {
      return null;
    }
  }
  // 裸 authority（CONNECT 目标）：host、host:port 或 [ipv6]:port
  const authority = /^(\[[^\]]+\]|[^\s:/?#]+)(?::(\d{1,5}))?$/.exec(uri);
  if (authority) {
    const segments: UrlSegment[] = [{ text: authority[1], cls: "url-host" }];
    if (authority[2])
      segments.push({ text: `:${authority[2]}`, cls: "url-path" });
    return segments;
  }
  return null;
}

export function urlSegments(uri: string): UrlSegment[] {
  return parseUrlSegments(uri) ?? [{ text: uri, cls: "url-path" }];
}
