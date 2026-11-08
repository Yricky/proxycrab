import { describe, expect, test } from "vitest";
import {
  formatHex,
  formatAscii,
  hexByteAt,
  hexLayout,
  hexViewport,
  hexScrollThumb,
  HEX_HEADER_HEIGHT,
  HEX_ROW_HEIGHT,
} from "./hex-viewer";

describe("hex layout", () => {
  test.each([false, true])("fits complete four-byte groups, ASCII=%s", (ascii) => {
    for (let width = 280; width <= 1800; width += 13) {
      const layout = hexLayout(width, 7.2, ascii);
      expect(layout.bytesPerRow % 4).toBe(0);
      expect(layout.width).toBe(width);
      const wider = hexLayout(width + 7.2 * (ascii ? 13 : 9), 7.2, ascii);
      expect(wider.groups).toBe(layout.groups + 1);
    }
  });

  test("uses extra width when ASCII is hidden and retains a whole group in narrow views", () => {
    expect(hexLayout(800, 7.2, false).groups).toBeGreaterThan(hexLayout(800, 7.2, true).groups);
    const narrow = hexLayout(100, 7.2, true);
    expect(narrow.groups).toBe(1);
    expect(narrow.width).toBeGreaterThan(100);
  });
});

describe("byte hit testing", () => {
  const layout = hexLayout(800, 7, true);
  const y = HEX_HEADER_HEIGHT + HEX_ROW_HEIGHT / 2;

  test("both hex digits select the same byte and group gaps select the preceding byte", () => {
    for (let byte = 0; byte < layout.bytesPerRow; byte++) {
      const x = layout.hexStart + Math.floor(byte / 4) * layout.groupStride + (byte % 4) * 14;
      expect(hexByteAt(layout, x + 1, y, 0, 1000)).toBe(byte);
      expect(hexByteAt(layout, x + 13, y, 0, 1000)).toBe(byte);
    }
    expect(hexByteAt(layout, layout.hexStart + 8 * 7 + 5, y, 0, 1000)).toBe(3);
  });

  test("ASCII and Hex coordinates resolve to the same bytes after scrolling", () => {
    for (let byte = 0; byte < layout.bytesPerRow; byte++) {
      const x = layout.asciiStart + byte * layout.charWidth + 1;
      expect(hexByteAt(layout, x, y, HEX_ROW_HEIGHT * 3, 1000, "ascii")).toBe(
        layout.bytesPerRow * 3 + byte,
      );
    }
    expect(hexByteAt(layout, layout.asciiStart + 1000, y, 0, 3, "ascii")).toBe(2);
    expect(hexByteAt(layout, layout.asciiStart - 100, y, 0, 1000, "ascii")).toBe(0);
  });

  test("accounts for partial-row scrolling and the pinned header", () => {
    expect(hexByteAt(layout, layout.hexStart, y, HEX_ROW_HEIGHT * 3, 1000)).toBe(
      layout.bytesPerRow * 3,
    );
    expect(hexByteAt(layout, layout.hexStart, HEX_HEADER_HEIGHT + 1, 5, 1000)).toBe(0);
  });

  test("clamps dragging outside the data and handles empty or incomplete rows", () => {
    expect(hexByteAt(layout, -100, -100, 0, 1000)).toBe(0);
    expect(hexByteAt(layout, 10000, 10000, 0, 17)).toBe(16);
    expect(hexByteAt(layout, layout.hexStart, y, 0, 0)).toBeNull();
    expect(hexByteAt(layout, layout.hexStart + 1000, y, 0, 3)).toBe(2);
  });
});

describe("canvas scrolling", () => {
  test("reserves scrollbar space only when needed and keeps complete groups", () => {
    const small = hexViewport(800, 600, 7.2, true, 4);
    expect(small.bodyWidth).toBe(800);
    expect(small.maxScrollTop).toBe(0);
    const large = hexViewport(800, 600, 7.2, true, 1024 ** 3);
    expect(large.bodyWidth).toBe(792);
    expect(large.layout.bytesPerRow % 4).toBe(0);
    expect(large.maxScrollTop).toBe(large.contentHeight - large.bodyHeight);
    expect(large.maxScrollLeft).toBe(0);
  });

  test("accounts for both scrollbars in narrow views", () => {
    const narrow = hexViewport(100, 100, 7.2, true, 256);
    expect(narrow.bodyWidth).toBe(92);
    expect(narrow.bodyHeight).toBe(72);
    expect(narrow.maxScrollLeft).toBeGreaterThan(0);
    expect(narrow.maxScrollTop).toBeGreaterThan(0);
  });

  test("maps scrollbar endpoints exactly and clamps the thumb", () => {
    expect(hexScrollThumb(100, 100, 0)).toBeNull();
    expect(hexScrollThumb(0, 100, 0)).toBeNull();
    expect(hexScrollThumb(100, 1000, -100)?.start).toBe(0);
    expect(hexScrollThumb(100, 1000, 0)?.size).toBe(20);
    const bottom = hexScrollThumb(100, 1000, 10000)!;
    expect(bottom.start + bottom.size).toBe(100);
    expect(hexScrollThumb(10, 1000, 500)?.travel).toBe(0);
  });
});

describe("ASCII copy", () => {
  test("copies visible ASCII without row breaks and preserves spaces", () => {
    expect(formatAscii(new TextEncoder().encode("Hello, ASCII! "))).toBe("Hello, ASCII! ");
  });
  test("uses dots for nonprintable bytes, including high bytes", () => {
    expect(formatAscii(new Uint8Array([0, 9, 10, 13, 31, 32, 33, 126, 127, 128, 255]))).toBe(
      "..... !~...",
    );
    expect(formatAscii(new Uint8Array())).toBe("");
  });
});

describe("hex copy formats", () => {
  const bytes = new Uint8Array([0, 1, 15, 16, 127, 128, 254, 255, 42]);

  test("defaults to four-byte groups and preserves an incomplete final group", () => {
    expect(formatHex(bytes)).toBe("00010f10 7f80feff 2a");
  });

  test("supports byte-separated and compact copies without offsets or ASCII", () => {
    expect(formatHex(bytes, "byte")).toBe("00 01 0f 10 7f 80 fe ff 2a");
    expect(formatHex(bytes, "compact")).toBe("00010f107f80feff2a");
    expect(formatHex(bytes.subarray(2, 7))).toBe("0f107f80 fe");
  });

  test.each(["group", "byte", "compact"] as const)("empty data, format=%s", (format) => {
    expect(formatHex(new Uint8Array(), format)).toBe("");
  });
});
