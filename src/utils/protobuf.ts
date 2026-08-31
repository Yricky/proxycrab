interface Cursor {
  bytes: Uint8Array;
  offset: number;
}

interface ProtoField {
  field: number;
  wire: number;
  value: unknown;
}

function readVarint(cursor: Cursor): bigint {
  let value = 0n;
  for (let shift = 0n; shift < 70n; shift += 7n) {
    if (cursor.offset >= cursor.bytes.length) throw new Error("截断的 varint");
    const byte = cursor.bytes[cursor.offset++];
    value |= BigInt(byte & 0x7f) << shift;
    if ((byte & 0x80) === 0) return value;
  }
  throw new Error("varint 超过 10 字节");
}

function hex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join(" ");
}

function printableUtf8(bytes: Uint8Array): string | null {
  try {
    const text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    return [...text].every((char) => char === "\n" || char === "\r" || char === "\t" || char >= " ")
      ? text
      : null;
  } catch {
    return null;
  }
}

function parseMessage(cursor: Cursor, depth = 0, endGroup?: number): ProtoField[] {
  const fields: ProtoField[] = [];
  while (cursor.offset < cursor.bytes.length) {
    const tag = readVarint(cursor);
    const field = Number(tag >> 3n);
    const wire = Number(tag & 7n);
    if (field === 0) throw new Error("字段号不能为 0");
    if (wire === 4) {
      if (endGroup !== field) throw new Error("不匹配的 end-group");
      return fields;
    }
    let value: unknown;
    if (wire === 0) {
      value = readVarint(cursor).toString();
    } else if (wire === 1) {
      if (cursor.offset + 8 > cursor.bytes.length) throw new Error("截断的 fixed64");
      value = new DataView(
        cursor.bytes.buffer,
        cursor.bytes.byteOffset + cursor.offset,
        8,
      ).getBigUint64(0, true).toString();
      cursor.offset += 8;
    } else if (wire === 2) {
      const length = Number(readVarint(cursor));
      if (!Number.isSafeInteger(length) || cursor.offset + length > cursor.bytes.length) {
        throw new Error("截断的 length-delimited 字段");
      }
      const bytes = cursor.bytes.subarray(cursor.offset, cursor.offset + length);
      cursor.offset += length;
      const text = printableUtf8(bytes);
      if (text !== null) {
        value = { len:length, txt: text };
      } else if (depth < 64 && bytes.length > 0) {
        try {
          const nestedCursor: Cursor = { bytes, offset: 0 };
          const message = parseMessage(nestedCursor, depth + 1);
          if (nestedCursor.offset !== bytes.length || message.length === 0) throw new Error();
          value = { len:length, msg: message };
        } catch {
          value = { len:length, bytes: hex(bytes) };
        }
      } else {
        value = { len:length, bytes: hex(bytes) };
      }
    } else if (wire === 3) {
      value = { group: parseMessage(cursor, depth + 1, field) };
    } else if (wire === 5) {
      if (cursor.offset + 4 > cursor.bytes.length) throw new Error("截断的 fixed32");
      value = new DataView(
        cursor.bytes.buffer,
        cursor.bytes.byteOffset + cursor.offset,
        4,
      ).getUint32(0, true);
      cursor.offset += 4;
    } else {
      throw new Error(`不支持的 wire type ${wire}`);
    }
    fields.push({ field, wire, value });
  }
  if (endGroup !== undefined) throw new Error("缺少 end-group");
  return fields;
}

function formatJson(value: unknown, depth = 0): string[] {
  const indent = " ".repeat(depth);
  if (value === null || typeof value !== "object") {
    return [`${indent}${JSON.stringify(value)}`];
  }

  if (Array.isArray(value)) {
    if (value.length === 0) return [`${indent}[]`];
    const lines = [`${indent}[`];
    value.forEach((item, index) => {
      const itemLines = formatJson(item, depth + 1);
      if (index < value.length - 1) itemLines[itemLines.length - 1] += ",";
      lines.push(...itemLines);
    });
    lines.push(`${indent}]`);
    return lines;
  }

  const record = value as Record<string, unknown>;
  const entries = Object.entries(record);
  const inlineEntry = (prefix: string, item: unknown): string[] => {
    const itemLines = formatJson(item, depth + 1);
    const firstItemLine = itemLines.shift()?.trimStart() ?? "null";
    const lines = [`${indent}${prefix}${firstItemLine}`, ...itemLines];
    lines[lines.length - 1] += " }";
    return lines;
  };
  const isProtoField =
    typeof record.field === "number" &&
    typeof record.wire === "number" &&
    Object.prototype.hasOwnProperty.call(record, "value");
  if (isProtoField) {
    return inlineEntry(
      `{ "field": ${record.field}, "wire": ${record.wire}, "value": `,
      record.value,
    );
  }
  const dataKey =
    typeof record.len === "number" && entries.length === 2
      ? entries.find(([key]) => key !== "len")?.[0]
      : undefined;
  if (dataKey !== undefined) {
    return inlineEntry(
      `{ "len": ${record.len}, ${JSON.stringify(dataKey)}: `,
      record[dataKey],
    );
  }

  if (entries.length === 0) return [`${indent}{}`];
  const lines = [`${indent}{`];
  entries.forEach(([key, item], index) => {
    const itemLines = formatJson(item, depth + 1);
    const firstItemLine = itemLines.shift()?.trimStart() ?? "null";
    const entryLines = [
      `${indent} ${JSON.stringify(key)}: ${firstItemLine}`,
      ...itemLines,
    ];
    if (index < entries.length - 1) entryLines[entryLines.length - 1] += ",";
    lines.push(...entryLines);
  });
  lines.push(`${indent}}`);
  return lines;
}

function format(value: unknown): string {
  const json = JSON.stringify(value);
  return json === undefined ? "" : formatJson(JSON.parse(json)).join("\n");
}

export function inspectProtobuf(bytes: Uint8Array): string {
  try {
    const cursor: Cursor = { bytes, offset: 0 };
    return format(parseMessage(cursor));
  } catch (error) {
    return `Protobuf 解析失败：${String(error)}\n\n${hex(bytes)}`;
  }
}

async function decompressFrame(bytes: Uint8Array, encoding: string): Promise<Uint8Array | null> {
  const format = encoding === "deflate" ? "deflate" : encoding === "gzip" ? "gzip" : null;
  if (!format || typeof DecompressionStream === "undefined") return null;
  try {
    const stream = new Blob([bytes.slice().buffer as ArrayBuffer])
      .stream()
      .pipeThrough(new DecompressionStream(format));
    return new Uint8Array(await new Response(stream).arrayBuffer());
  } catch {
    return null;
  }
}

export async function inspectGrpc(bytes: Uint8Array, encoding: string): Promise<string> {
  const frames: unknown[] = [];
  let offset = 0;
  while (offset < bytes.length) {
    if (offset + 5 > bytes.length) throw new Error("截断的 gRPC frame header");
    const compressed = bytes[offset] !== 0;
    const length = new DataView(bytes.buffer, bytes.byteOffset + offset + 1, 4).getUint32(0);
    offset += 5;
    if (offset + length > bytes.length) throw new Error("截断的 gRPC frame body");
    const raw = bytes.subarray(offset, offset + length);
    offset += length;
    const decoded = compressed ? await decompressFrame(raw, encoding) : raw;
    const inspected = decoded ? inspectProtobuf(decoded) : null;
    let message: unknown = inspected;
    if (inspected) {
      try {
        message = JSON.parse(inspected);
      } catch {
        // Preserve the complete parser diagnostic as text.
      }
    }
    frames.push({
      compressed,
      encoding: compressed ? encoding || "unknown" : undefined,
      length,
      message: decoded ? message : undefined,
      bytes: decoded ? undefined : hex(raw),
    });
  }
  return format(frames);
}
