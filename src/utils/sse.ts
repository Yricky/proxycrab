export interface SseBlock {
  raw: string;
  complete: boolean;
}

/** Split raw SSE blocks without interpreting fields or discarding an unfinished tail. */
export function splitSseBlocks(text: string): SseBlock[] {
  const blocks: SseBlock[] = [];
  const endings = /\r\n|[\r\n]/g;
  let start = 0;
  let lineStart = text.startsWith("\uFEFF") ? 1 : 0;
  let hasContent = false;
  for (const ending of text.matchAll(endings)) {
    const end = ending.index + ending[0].length;
    if (ending.index === lineStart) {
      if (hasContent) blocks.push({ raw: text.slice(start, end), complete: true });
      start = end;
      hasContent = false;
    } else {
      hasContent = true;
    }
    lineStart = end;
  }
  if (hasContent || lineStart < text.length) {
    blocks.push({ raw: text.slice(start), complete: false });
  }
  return blocks;
}
