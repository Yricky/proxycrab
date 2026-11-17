import { createShareBackend, verifyShareAccess, SHARE_INVALID_EVENT } from "./api/share-backend";
import type { Backend } from "./api/session-page-backend";

function showError(error: unknown): void {
  const message = error instanceof Error ? error.message : String(error);
  const main = document.createElement("main");
  main.style.cssText = "padding:20px;font:14px system-ui;color:#b42318;white-space:pre-wrap";
  main.textContent = message;
  document.body.replaceChildren(main);
}
async function initialize(): Promise<Backend> {
  const tokens = new URLSearchParams(window.location.search).getAll("token");
  if (tokens.length !== 1 || !tokens[0])
    throw new Error("Session page requires a valid access token");
  const token = tokens[0];
  const base = new URL("/", window.location.href).href;
  const bootstrap = await verifyShareAccess(base, token);
  window.addEventListener(SHARE_INVALID_EVENT, (event) => {
    showError((event as CustomEvent<{ message: string }>).detail.message);
  });
  return createShareBackend(base, token, bootstrap);
}

/** Native module initialization completes before a page's importing module executes. */
export const backend: Backend = await initialize().catch((error: unknown) => {
  showError(error);
  throw error;
});
