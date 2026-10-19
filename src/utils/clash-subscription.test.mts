import { deepStrictEqual, equal } from "node:assert/strict";
import { test } from "vitest";
import { clashImportUrl, clashServerAddresses, clashSubscriptionUrl } from "./clash-subscription.ts";

test("Clash server candidates exclude unusable addresses without loopback fallback", () => {
  deepStrictEqual(clashServerAddresses([
    "0.0.0.0", "127.0.0.1", "127.0.0.2", "::1", "localhost", "224.0.0.1",
    "255.255.255.255", "256.1.1.1", "1.2.3", "01.2.3.4", "0.1.2.3",
    "192.168.1.5", "192.0.2.10", "192.168.1.5",
  ]), ["192.168.1.5", "192.0.2.10"]);
  deepStrictEqual(clashServerAddresses(["0.0.0.0", "127.0.0.1"]), []);
});

test("subscription and import links preserve the selected host, port and encoded token", () => {
  const url = clashSubscriptionUrl("192.168.1.5", 18090, "pcrab_clash_a+b&c");
  const download = new URL(url);
  equal(download.host, "192.168.1.5:18090");
  equal(download.pathname, "/clash.yaml");
  equal(download.searchParams.get("token"), "pcrab_clash_a+b&c");
  const install = new URL(clashImportUrl(url));
  equal(install.protocol, "clash:");
  equal(install.hostname, "install-config");
  equal(install.searchParams.get("url"), url);
});
