import test from "node:test";
import assert from "node:assert/strict";
import { buildSessionShareLinks } from "./session-share.ts";

test("share links prefer reachable non-loopback IPv4 addresses", () => {
  assert.deepEqual(
    buildSessionShareLinks(
      [
        "127.0.0.1",
        "192.168.1.5",
        "0.0.0.0",
        "10.0.0.2",
        "192.168.1.5",
      ],
      18089,
      "pcrab_share_a+b",
    ),
    [
      "http://192.168.1.5:18089/session?token=pcrab_share_a%2Bb",
      "http://10.0.0.2:18089/session?token=pcrab_share_a%2Bb",
    ],
  );
});

test("share links fall back to loopback when no external address exists", () => {
  assert.deepEqual(
    buildSessionShareLinks(
      ["0.0.0.0", "127.0.0.1", "127.0.0.1"],
      18089,
      "token",
    ),
    [
      "http://127.0.0.1:18089/session?token=token",
    ],
  );
});
