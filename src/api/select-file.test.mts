import { afterEach, expect, test, vi } from "vitest";
import { selectFile } from "./select-file.ts";
afterEach(() => vi.unstubAllGlobals());
test.each(["change", "cancel"])("single-file selection cleans up after %s", async (event) => {
  const listeners = new Map<string, () => void>();
  const file = new Blob(["hello"]);
  const input = {
    files: [file],
    remove: vi.fn(),
    click: vi.fn(),
    multiple: true,
    addEventListener: (name: string, callback: () => void) => listeners.set(name, callback),
  };
  vi.stubGlobal("document", { createElement: () => input, body: { append: vi.fn() } });
  const result = selectFile();
  expect(input.multiple).toBe(false);
  expect(input.click).toHaveBeenCalledOnce();
  listeners.get(event)!();
  expect(await result).toBe(event === "change" ? file : null);
  expect(input.remove).toHaveBeenCalledOnce();
});
