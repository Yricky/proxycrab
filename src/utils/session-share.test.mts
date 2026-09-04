import { expect, test } from "vitest";
import { buildHarShareLinks, buildSessionShareLinks } from "./session-share.ts";

test("share links prefer reachable non-loopback IPv4 addresses", () => {
  expect(
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
  ).toEqual([
    "http://192.168.1.5:18089/session?token=pcrab_share_a%2Bb",
    "http://10.0.0.2:18089/session?token=pcrab_share_a%2Bb",
  ]);
});

test("share links fall back to loopback when no external address exists", () => {
  expect(
    buildSessionShareLinks(
      ["0.0.0.0", "127.0.0.1", "127.0.0.1"],
      18089,
      "token",
    ),
  ).toEqual(["http://127.0.0.1:18089/session?token=token"]);
});

test("HAR share links reuse address selection and target the download route", () => {
  expect(
    buildHarShareLinks(
      ["127.0.0.1", "192.168.1.5", "192.168.1.5"],
      18089,
      "pcrab_har_a+b",
    ),
  ).toEqual([
    "http://192.168.1.5:18089/session.har?token=pcrab_har_a%2Bb",
  ]);
});
