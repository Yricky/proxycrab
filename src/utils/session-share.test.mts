import test from "node:test";
import assert from "node:assert/strict";
import {
  DEFAULT_SHARE_HOURS,
  MAX_SHARE_HOURS,
  buildSessionShareLinks,
  parseShareHours,
} from "./session-share.ts";

test("share hours accept only integers from 1 through 720", () => {
  assert.equal(DEFAULT_SHARE_HOURS, 24);
  assert.equal(parseShareHours("1"), 1);
  assert.equal(parseShareHours(String(MAX_SHARE_HOURS)), MAX_SHARE_HOURS);
  for (const value of ["", "0", "721", "1.5", "-1", "abc"]) {
    assert.equal(parseShareHours(value), null);
  }
});

test("share links exclude wildcard and loopback and reuse one encoded token", () => {
  assert.deepEqual(
    buildSessionShareLinks(
      ["127.0.0.1", "192.168.1.5", "0.0.0.0", "10.0.0.2", "192.168.1.5"],
      18089,
      "pcrab_share_a+b",
    ),
    [
      "http://192.168.1.5:18089/session?token=pcrab_share_a%2Bb",
      "http://10.0.0.2:18089/session?token=pcrab_share_a%2Bb",
    ],
  );
});
