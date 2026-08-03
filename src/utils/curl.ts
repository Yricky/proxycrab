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
  if (body.type !== "empty" && body.path) {
    lines[lines.length - 1] += " \\";
    lines.push(`  --data-binary ${shellQuote(`@${body.path}`)}`);
  }
  return lines.join("\n");
}
