import { afterEach, expect, test, vi } from "vitest";
import { createHttpBackend } from "./http-backend.ts";

afterEach(() => vi.unstubAllGlobals());

test.each([
  ["http://192.0.2.10:18089/", "192.0.2.10"],
  ["https://proxy.example.com:8443/", "proxy.example.com"],
  ["http://[::1]:18089/", "[::1]"],
])("web addresses come from the page hostname without a request: %s", async (url, hostname) => {
  vi.stubGlobal("window", { location: new URL(url) });
  const fetchMock = vi.fn();
  vi.stubGlobal("fetch", fetchMock);
  const backend = createHttpBackend("http://different-backend:18089", "test-key", {
    target: "cli",
    workspace: { current_path: "/workspace", configured_path: "/workspace" },
    http_service: { running: true, host: "0.0.0.0", port: 18089, error: null },
  });
  expect(await backend.listLocalIps!()).toEqual([hostname]);
  expect(fetchMock).not.toHaveBeenCalled();
});

test("certificate reads public PEM from the backend without management credentials", async () => {
  const pem = "-----BEGIN CERTIFICATE-----\npublic-ca\n-----END CERTIFICATE-----\n";
  const fetchMock = vi.fn().mockResolvedValue(new Response(pem));
  vi.stubGlobal("fetch", fetchMock);
  const backend = createHttpBackend("http://certificate-backend:18089", "test-key", {
    target: "cli",
    workspace: { current_path: "/workspace", configured_path: "/workspace" },
    http_service: { running: true, host: "0.0.0.0", port: 18089, error: null },
  });
  expect(await backend.getCertificate()).toEqual({ pem });
  expect(fetchMock).toHaveBeenCalledExactlyOnceWith(
    new URL("http://certificate-backend:18089/ca.crt"),
  );

  fetchMock.mockResolvedValueOnce(new Response("unavailable", { status: 500 }));
  await expect(backend.getCertificate()).rejects.toMatchObject({ code: "http_500" });
  fetchMock.mockRejectedValueOnce(new TypeError("connection closed"));
  await expect(backend.getCertificate()).rejects.toMatchObject({ code: "network_error" });
});

test("Session sharing creates and updates config through the same POST", async () => {
  const state = { session_id: 7, enabled: true, persistent: true, token: "same-token" };
  const fetchMock = vi
    .fn()
    .mockImplementation(async () => new Response(JSON.stringify({ ok: true, data: state })));
  vi.stubGlobal("fetch", fetchMock);
  const backend = createHttpBackend("http://backend:18089", "test-key", {
    target: "cli",
    workspace: { current_path: "/workspace", configured_path: "/workspace" },
    http_service: { running: true, host: "0.0.0.0", port: 18089, error: null },
  });
  expect(
    await backend.enableSessionShare(7, { redaction_script: "mask", persistent: true }),
  ).toEqual(state);
  expect(
    await backend.enableSessionShare(7, { redaction_script: null, persistent: false }),
  ).toEqual(state);
  await backend.enableSessionShare(7);
  for (const [url, init] of fetchMock.mock.calls) {
    expect(new URL(url).pathname).toBe("/api/session-shares");
    expect(init.method).toBe("POST");
  }
  expect(fetchMock.mock.calls.map(([, init]) => JSON.parse(init.body))).toEqual([
    { session_id: 7, session_share_config: { redaction_script: "mask", persistent: true } },
    { session_id: 7, session_share_config: { redaction_script: null, persistent: false } },
    { session_id: 7 },
  ]);
});

test("Session sharing summaries use one batch read", async () => {
  const payload = { shares: [{ session_id: 3, persistent: true }] };
  const fetchMock = vi
    .fn()
    .mockResolvedValue(new Response(JSON.stringify({ ok: true, data: payload })));
  vi.stubGlobal("fetch", fetchMock);
  const backend = createHttpBackend("http://backend:18089", "test-key", {
    target: "cli",
    workspace: { current_path: "/workspace", configured_path: "/workspace" },
    http_service: { running: true, host: "0.0.0.0", port: 18089, error: null },
  });
  expect(await backend.listSessionShares!()).toEqual(payload);
  expect(fetchMock).toHaveBeenCalledTimes(1);
  expect(new URL(fetchMock.mock.calls[0][0]).pathname).toBe("/api/session-shares");
});

test("asset upload preserves binary data and explicit content type", async () => {
  const metadata = { id: "images/icon.png", size: 3 };
  const fetchMock = vi
    .fn()
    .mockResolvedValue(new Response(JSON.stringify({ ok: true, data: metadata }), { status: 201 }));
  vi.stubGlobal("fetch", fetchMock);
  const backend = createHttpBackend("http://localhost:18089", "test-key", {
    target: "cli",
    workspace: { current_path: "/workspace", configured_path: "/workspace" },
    http_service: { running: true, host: "0.0.0.0", port: 18089, error: null },
  });
  const file = new Blob([new Uint8Array([0, 255, 1])], { type: "image/png" });
  expect(await backend.uploadAsset!(metadata.id, file)).toEqual(metadata);
  const [url, options] = fetchMock.mock.calls[0];
  expect(String(url)).toBe("http://localhost:18089/api/assets/images/icon.png");
  expect(options.method).toBe("POST");
  expect(options.body).toBe(file);
  expect(options.headers.get("Content-Type")).toBe("image/png");
  expect(options.headers.get("Authorization")).toBe("Bearer test-key");
});

test("bundle imports submit the original file and explicit accepted IDs, then resolve by import ID", async () => {
  const response = { import_id: "pending", files: [], sessions: [] };
  const fetchMock = vi
    .fn()
    .mockImplementation(async () => new Response(JSON.stringify({ ok: true, data: response })));
  vi.stubGlobal("fetch", fetchMock);
  const backend = createHttpBackend("http://localhost", "key", {
    target: "cli",
    workspace: { current_path: "/workspace", configured_path: "/workspace" },
    http_service: { running: true, host: "0.0.0.0", port: 18089, error: null },
  });
  const file = new File(["zip bytes"], "bundle.zip", { type: "application/zip" });
  expect(await backend.importBundle!(file, ["chosen"])).toEqual(response);
  const [url, init] = fetchMock.mock.calls[0]!;
  expect(url.pathname).toBe("/api/imports");
  expect(init.headers.get("Content-Type")).toBeNull();
  expect(init.headers.get("Authorization")).toBe("Bearer key");
  expect(init.body.get("accepted_intent_ids")).toBe('["chosen"]');
  expect(await init.body.get("file").text()).toBe("zip bytes");
  await backend.resolveBundle!("pending", [
    { path: "assets/a.txt", fingerprint: "sha", action: "keep" },
  ]);
  expect(fetchMock.mock.calls[1]![0].pathname).toBe("/api/imports/pending/resolve");
});
