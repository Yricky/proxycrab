import type { LogDetail } from "../api/types";

function shellQuote(value: string): string {
  return `'${value.split("'").join(`'"'"'`)}'`;
}

export function fullCurl(detail: LogDetail): string {
  const lines = [
    "curl \\",
    `  --request ${shellQuote(detail.request.method)} \\`,
    `  --url ${shellQuote(detail.request.uri)}`,
  ];
  const headers = detail.request.headers.filter(
    (header) => header.name.toLowerCase() !== "content-length",
  );
  for (const header of headers) {
    lines[lines.length - 1] += " \\";
    lines.push(`  --header ${shellQuote(`${header.name}: ${header.value}`)}`);
  }
  const body = detail.request.body;
  if (body.type === "text" || body.type === "json") {
    const content = body.type === "text" ? body.content : (JSON.stringify(body.content) ?? "null");
    lines[lines.length - 1] += " \\";
    lines.push(`  --data-raw ${shellQuote(content)}`);
  } else if ((body.type === "binary" || body.type === "large") && body.size > 0) {
    lines.unshift("# Body 未内联：请将原始 Body 通过 stdin 传给 curl。");
    lines[lines.length - 1] += " \\";
    lines.push("  --data-binary @-");
  }
  return lines.join("\n");
}
