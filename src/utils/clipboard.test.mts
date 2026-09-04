import { expect, test } from "vitest";
import { copyText, setNativeClipboardWriter } from "./clipboard.ts";

function legacyDocument(copyResult = true): {
  document: Document;
  textarea: {
    value: string;
    focused: boolean;
    selected: boolean;
    removed: boolean;
  };
} {
  const textarea = {
    value: "",
    focused: false,
    selected: false,
    removed: false,
    style: {},
    setAttribute() {},
    focus() {
      this.focused = true;
    },
    select() {
      this.selected = true;
    },
    remove() {
      this.removed = true;
    },
  };
  const document = {
    createElement(name: string) {
      expect(name).toBe("textarea");
      return textarea;
    },
    body: {
      appendChild(node: unknown) {
        expect(node).toBe(textarea);
      },
    },
    execCommand(command: string) {
      expect(command).toBe("copy");
      return copyResult;
    },
  } as unknown as Document;
  return { document, textarea };
}

test("copyText prefers the Clipboard API", async () => {
  let copied = "";
  const clipboard = {
    async writeText(value: string) {
      copied = value;
    },
  } as Clipboard;

  await copyText("secret", clipboard);

  expect(copied).toBe("secret");
});

test("copyText prefers the configured native clipboard writer", async () => {
  let nativeCopied = "";
  let browserCalled = false;
  setNativeClipboardWriter(async (value) => {
    nativeCopied = value;
  });

  try {
    await copyText("secret", {
      async writeText() {
        browserCalled = true;
      },
    });
  } finally {
    setNativeClipboardWriter(undefined);
  }

  expect(nativeCopied).toBe("secret");
  expect(browserCalled).toBe(false);
});

test("copyText falls back to execCommand outside a secure context", async () => {
  const legacy = legacyDocument();

  await copyText("secret", undefined, legacy.document);

  expect(legacy.textarea.value).toBe("secret");
  expect(legacy.textarea.focused).toBe(true);
  expect(legacy.textarea.selected).toBe(true);
  expect(legacy.textarea.removed).toBe(true);
});

test("copyText reports a failed legacy copy and still removes the textarea", async () => {
  const legacy = legacyDocument(false);

  await expect(copyText("secret", undefined, legacy.document)).rejects.toThrow(
    /clipboard/i,
  );

  expect(legacy.textarea.removed).toBe(true);
});
