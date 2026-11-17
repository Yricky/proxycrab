import { afterEach, expect, test, vi } from "vitest";
import { createHttpBackend } from "./http-backend";
import { createTauriBackend } from "./tauri-backend";
import { createShareBackend } from "./share-backend";
import { invoke } from "@tauri-apps/api/core";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
afterEach(() => {
  vi.unstubAllGlobals();
  vi.mocked(invoke).mockReset();
});
function http() {
  return createHttpBackend("http://localhost:18089", "owner", {
    target: "cli",
    workspace: { current_path: "/workspace", configured_path: "/workspace" },
    http_service: { running: true, host: "0.0.0.0", port: 18089, error: null },
  });
}
test("page upload keeps raw ZIP bytes and requires an explicit replacement flag", async () => {
  const fetch = vi.fn(async () => new Response(JSON.stringify({ ok: true, data: { id: "demo" } })));
  vi.stubGlobal("fetch", fetch);
  const file = new File([new Uint8Array([0, 255, 1])], "page.zip", { type: "application/zip" });
  await http().importSessionPage!(file);
  await http().importSessionPage!(file, true);
  expect(String(fetch.mock.calls[0]![0])).toBe(
    "http://localhost:18089/api/session-pages?replace=false",
  );
  expect(String(fetch.mock.calls[1]![0])).toBe(
    "http://localhost:18089/api/session-pages?replace=true",
  );
  const options = fetch.mock.calls[0]![1] as RequestInit;
  expect(options.body).toBe(file);
  expect((options.headers as Headers).get("Content-Type")).toBe("application/zip");
  expect((options.headers as Headers).get("Authorization")).toBe("Bearer owner");
  vi.mocked(invoke).mockResolvedValue({ id: "demo" });
  await createTauriBackend().importSessionPage!(file, true);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("import_session_page", await file.arrayBuffer(), {
    headers: { "x-page-replace": "true" },
  });
});
test("page exports preserve bytes and surface authorization errors", async () => {
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(new Response(new Uint8Array([0, 255, 1])))
    .mockResolvedValueOnce(
      new Response(JSON.stringify({ error: { code: "invalid_api_key", message: "denied" } }), {
        status: 401,
      }),
    );
  vi.stubGlobal("fetch", fetch);
  expect(await http().exportSessionPage!("demo")).toEqual(new Uint8Array([0, 255, 1]));
  await expect(http().exportSessionPage!("demo")).rejects.toMatchObject({
    code: "invalid_api_key",
  });
});
test("shared pages expose no page management methods", () => {
  const shared = createShareBackend("http://localhost", "share", {
    session: { id: 7, name: "selected", created_at: 0, description: null },
    view: { session_id: 7, columns: [], filter: { option: null, input: "" } },
    proxy_status: { status: "stopped" },
    api_port: 18089,
  });
  for (const name of [
    "listSessionPages",
    "importSessionPage",
    "exportSessionPage",
    "deleteSessionPage",
  ]) {
    expect(Reflect.get(shared, name)).toBeUndefined();
  }
});
