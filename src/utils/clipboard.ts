export async function copyText(
  value: string,
  clipboard: Pick<Clipboard, "writeText"> | undefined =
    typeof navigator === "undefined" ? undefined : navigator.clipboard,
  documentRef: Document | undefined = typeof document === "undefined" ? undefined : document,
): Promise<void> {
  if (clipboard) {
    try {
      await clipboard.writeText(value);
      return;
    } catch {
      // The legacy path may still work when Clipboard API permission is denied.
    }
  }

  if (!documentRef?.body) {
    throw new Error("Clipboard API is unavailable");
  }

  const textarea = documentRef.createElement("textarea");
  textarea.value = value;
  textarea.setAttribute("readonly", "");
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  textarea.style.pointerEvents = "none";
  documentRef.body.appendChild(textarea);
  try {
    textarea.focus({ preventScroll: true });
    textarea.select();
    if (!documentRef.execCommand("copy")) {
      throw new Error("Clipboard copy command failed");
    }
  } finally {
    textarea.remove();
  }
}
