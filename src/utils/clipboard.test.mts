import assert from "node:assert/strict";
import test from "node:test";
import { copyText } from "./clipboard.ts";

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
      assert.equal(name, "textarea");
      return textarea;
    },
    body: {
      appendChild(node: unknown) {
        assert.equal(node, textarea);
      },
    },
    execCommand(command: string) {
      assert.equal(command, "copy");
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

  assert.equal(copied, "secret");
});

test("copyText falls back to execCommand outside a secure context", async () => {
  const legacy = legacyDocument();

  await copyText("secret", undefined, legacy.document);

  assert.equal(legacy.textarea.value, "secret");
  assert.equal(legacy.textarea.focused, true);
  assert.equal(legacy.textarea.selected, true);
  assert.equal(legacy.textarea.removed, true);
});

test("copyText reports a failed legacy copy and still removes the textarea", async () => {
  const legacy = legacyDocument(false);

  await assert.rejects(copyText("secret", undefined, legacy.document), /clipboard/i);

  assert.equal(legacy.textarea.removed, true);
});
