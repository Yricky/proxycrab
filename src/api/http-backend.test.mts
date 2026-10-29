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
