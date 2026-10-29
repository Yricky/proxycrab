import { afterEach, expect, test, vi } from "vitest";
import { createFileWriter, saveBytesFile, saveFileInWebview } from "./save-file";

afterEach(() => vi.unstubAllGlobals());
test("writer orders chunks, commits once, rejects later appends and never aborts committed files", async () => {
  const seen: number[] = [];
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  const operations = {
    append: vi.fn(async (bytes: Uint8Array) => {
      if (!seen.length) await gate;
      seen.push(bytes[0]!);
    }),
    markAsFinish: vi.fn(async () => {}),
    markAsError: vi.fn(async () => {}),
  };
  const writer = createFileWriter(operations);
  const a = writer.append(new Uint8Array([1]));
  const b = writer.append(new Uint8Array([2]));
  const finish = writer.markAsFinish();
  await Promise.resolve();
  expect(seen).toEqual([]);
  release();
  await Promise.all([a, b, finish]);
  expect(seen).toEqual([1, 2]);
  await writer.markAsFinish();
  await writer.markAsError();
  expect(operations.markAsFinish).toHaveBeenCalledTimes(1);
  expect(operations.markAsError).not.toHaveBeenCalled();
  await expect(writer.append(new Uint8Array())).rejects.toThrow("文件已关闭");
});
test.each(["append", "markAsFinish"] as const)(
  "%s failure prevents commit but permits cleanup",
  async (method) => {
    const operations = {
      append: vi.fn(async () => {}),
      markAsFinish: vi.fn(async () => {}),
      markAsError: vi.fn(async () => {}),
    };
    operations[method].mockRejectedValueOnce(new Error("disk full"));
    const writer = createFileWriter(operations);
    await expect(
      method === "append" ? writer.append(new Uint8Array([1])) : writer.markAsFinish(),
    ).rejects.toThrow("disk full");
    await expect(writer.markAsFinish()).rejects.toThrow("无法提交");
    await writer.markAsError();
    expect(operations.markAsError).toHaveBeenCalledTimes(1);
  },
);
test("browser writer snapshots bytes and downloads only on finish, abort discards all content", async () => {
  const click = vi.fn();
  const objectUrl = vi.fn(() => "blob:test");
  vi.stubGlobal("URL", { createObjectURL: objectUrl, revokeObjectURL: vi.fn() });
  vi.stubGlobal("document", {
    createElement: () => ({ click, remove: vi.fn() }),
    body: { appendChild: vi.fn() },
  });
  vi.stubGlobal("window", { setTimeout: vi.fn() });
  const bytes = new Uint8Array([1, 2]);
  const writer = await saveFileInWebview({ filename: "capture.har" });
  await writer.append(bytes);
  bytes[0] = 9;
  await writer.append(new Uint8Array([3]));
  expect(click).not.toHaveBeenCalled();
  await writer.markAsFinish();
  expect(click).toHaveBeenCalledTimes(1);
  const blob = objectUrl.mock.calls[0]![0] as Blob;
  expect([...new Uint8Array(await blob.arrayBuffer())]).toEqual([1, 2, 3]);
  const aborted = await saveFileInWebview({ filename: "partial.har" });
  await aborted.append(bytes);
  await aborted.markAsError();
  await expect(aborted.markAsFinish()).rejects.toThrow();
  expect(click).toHaveBeenCalledTimes(1);
});
test("small exports respect picker cancellation and clean up after write failure", async () => {
  const saveFile = vi.fn().mockResolvedValueOnce(null);
  expect(await saveBytesFile({ saveFile }, "a", new Uint8Array())).toBe(false);
  const writer = {
    append: vi.fn().mockRejectedValue(new Error("write failed")),
    markAsFinish: vi.fn(),
    markAsError: vi.fn().mockResolvedValue(undefined),
  };
  saveFile.mockResolvedValueOnce(writer);
  await expect(saveBytesFile({ saveFile }, "a", new Uint8Array())).rejects.toThrow("write failed");
  expect(writer.markAsError).toHaveBeenCalledOnce();
  expect(writer.markAsFinish).not.toHaveBeenCalled();
});
