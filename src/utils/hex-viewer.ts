export type HexCopyFormat = "group" | "byte" | "compact";
export type HexViewerCopyFormat = HexCopyFormat | "ascii";
export type HexRightPane = "ascii" | "none";
export interface HexSelection {
  start: number;
  end: number;
}

export const HEX_BYTES = Array.from({ length: 256 }, (_, byte) =>
  byte.toString(16).padStart(2, "0"),
);
export const ASCII_BYTES = Array.from({ length: 256 }, (_, byte) =>
  byte >= 0x20 && byte <= 0x7e ? String.fromCharCode(byte) : ".",
);
export const HEX_ROW_HEIGHT = 18;
export const HEX_HEADER_HEIGHT = 20;
export const HEX_SCROLLBAR_SIZE = 8;

export function hexLayout(width: number, charWidth: number, ascii: boolean) {
  const hexStart = 6 + charWidth * 8 + 12;
  const groupStride = charWidth * 9;
  const asciiGap = ascii ? 14 : 0;
  const groupWidth = groupStride + (ascii ? charWidth * 4 : 0);
  const groups = Math.max(
    1,
    Math.floor((width - hexStart - 6 - asciiGap + charWidth) / groupWidth),
  );
  const hexEnd = hexStart + groups * groupStride - charWidth;
  const asciiStart = hexEnd + asciiGap;
  return {
    groups,
    bytesPerRow: groups * 4,
    charWidth,
    hexStart,
    hexEnd,
    groupStride,
    asciiStart,
    width: Math.max(width, (ascii ? asciiStart + groups * 4 * charWidth : hexEnd) + 6),
  };
}

export type HexLayout = ReturnType<typeof hexLayout>;

export function hexViewport(
  width: number,
  height: number,
  charWidth: number,
  ascii: boolean,
  length: number,
) {
  const initial = hexLayout(width, charWidth, ascii);
  const initialHeight = Math.max(
    0,
    height - HEX_HEADER_HEIGHT - (initial.width > width ? HEX_SCROLLBAR_SIZE : 0),
  );
  const vertical = Math.ceil(length / initial.bytesPerRow) * HEX_ROW_HEIGHT > initialHeight;
  const bodyWidth = Math.max(0, width - (vertical ? HEX_SCROLLBAR_SIZE : 0));
  const layout = hexLayout(bodyWidth, charWidth, ascii);
  const bodyHeight = Math.max(
    0,
    height - HEX_HEADER_HEIGHT - (layout.width > bodyWidth ? HEX_SCROLLBAR_SIZE : 0),
  );
  const contentHeight = Math.ceil(length / layout.bytesPerRow) * HEX_ROW_HEIGHT;
  return {
    layout,
    bodyWidth,
    bodyHeight,
    contentHeight,
    maxScrollLeft: Math.max(0, layout.width - bodyWidth),
    maxScrollTop: Math.max(0, contentHeight - bodyHeight),
  };
}

export function hexScrollThumb(visible: number, total: number, position: number) {
  if (visible <= 0 || total <= visible) return null;
  const size = Math.min(visible, Math.max(20, (visible * visible) / total));
  const travel = visible - size;
  const maxScroll = total - visible;
  return {
    size,
    travel,
    maxScroll,
    start: Math.max(0, Math.min(1, position / maxScroll)) * travel,
  };
}

/** Clamp to the nearest displayed byte; blank bytes in the last row never become selectable. */
export function hexByteAt(
  layout: HexLayout,
  x: number,
  y: number,
  scrollTop: number,
  length: number,
  pane: "hex" | "ascii" = "hex",
): number | null {
  if (!length) return null;
  const row = Math.max(0, Math.floor((y - HEX_HEADER_HEIGHT + scrollTop) / HEX_ROW_HEIGHT));
  if (pane === "ascii") {
    const column = Math.max(
      0,
      Math.min(layout.bytesPerRow - 1, Math.floor((x - layout.asciiStart) / layout.charWidth)),
    );
    return Math.min(length - 1, row * layout.bytesPerRow + column);
  }
  const local = Math.max(0, x - layout.hexStart);
  const group = Math.min(layout.groups - 1, Math.floor(local / layout.groupStride));
  const byte = Math.min(
    3,
    Math.floor((local - group * layout.groupStride) / (layout.charWidth * 2)),
  );
  return Math.min(length - 1, row * layout.bytesPerRow + group * 4 + byte);
}

export function formatAscii(bytes: Uint8Array): string {
  let text = "";
  for (const byte of bytes) text += ASCII_BYTES[byte];
  return text;
}

export function formatHex(bytes: Uint8Array, format: HexCopyFormat = "group"): string {
  const groupSize = format === "group" ? 4 : format === "byte" ? 1 : bytes.length || 1;
  const groups: string[] = [];
  for (let start = 0; start < bytes.length; start += groupSize) {
    let group = "";
    for (let i = start; i < Math.min(start + groupSize, bytes.length); i++) {
      group += HEX_BYTES[bytes[i]];
    }
    groups.push(group);
  }
  return groups.join(" ");
}
