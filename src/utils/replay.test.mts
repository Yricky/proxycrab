import { describe, expect, it } from "vitest";
import {
  contentLengthIssue,
  guessContentType,
  kvRowsToTuples,
  normalizeKvRows,
  prefillFromLog,
  prefillFromSnapshot,
} from "./replay";

describe("normalizeKvRows", () => {
  it("removes middle empty rows and keeps exactly one trailing empty row", () => {
    expect(
      normalizeKvRows([
        { name: "a", value: "1" },
        { name: "", value: "" },
        { name: "b", value: "2" },
        { name: "", value: "" },
      ]),
    ).toEqual([
      { name: "a", value: "1" },
      { name: "b", value: "2" },
      { name: "", value: "" },
    ]);
  });
});

describe("kvRowsToTuples", () => {
  it("drops empty rows", () => {
    expect(
      kvRowsToTuples([
        { name: "a", value: "1" },
        { name: "", value: "" },
      ]),
    ).toEqual([["a", "1"]]);
  });
});

describe("prefillFromLog", () => {
  const detail = {
    id: 7,
    session_id: 3,
    request: {
      method: "POST",
      uri: "http://x.test/a?b=1",
      headers: [{ name: "content-type", value: "text/plain" }],
      body: { type: "text", content: "hi", size: 2, path: null },
    },
  } as never;
  it("uses final values and body_ref for non-empty bodies", () => {
    expect(prefillFromLog(detail)).toEqual({
      method: "POST",
      url: "http://x.test/a?b=1",
      headers: [["content-type", "text/plain"]],
      body: { type: "body_ref", session_id: 3, log_id: 7, side: "request" },
    });
  });
});

describe("prefillFromSnapshot", () => {
  it("returns null without a request snapshot", () => {
    expect(
      prefillFromSnapshot(1, 2, { request: null, response: null } as never),
    ).toBeNull();
  });
  it("maps string bodies to text mode", () => {
    const draft = prefillFromSnapshot(1, 2, {
      request: {
        method: "PUT",
        uri: "http://x.test/p",
        version: "HTTP/1.1",
        headers: { a: ["1", "2"] },
        body: { type: "string", content: "v" },
      },
      response: null,
    } as never);
    expect(draft).toEqual({
      method: "PUT",
      url: "http://x.test/p",
      headers: [
        ["a", "1"],
        ["a", "2"],
      ],
      body: { type: "text", text: "v", charset: "utf8" },
    });
  });
  it("maps original bodies to body_ref and asset bodies to asset mode", () => {
    const original = prefillFromSnapshot(1, 2, {
      request: {
        method: "POST",
        uri: "http://x.test/p",
        version: "HTTP/1.1",
        headers: {},
        body: { type: "original" },
      },
      response: null,
    } as never);
    expect(original?.body).toEqual({
      type: "body_ref",
      session_id: 1,
      log_id: 2,
      side: "request",
    });
    const asset = prefillFromSnapshot(1, 2, {
      request: {
        method: "POST",
        uri: "http://x.test/p",
        version: "HTTP/1.1",
        headers: {},
        body: { type: "asset", asset_id: "fixtures/a.bin" },
      },
      response: null,
    } as never);
    expect(asset?.body).toEqual({
      type: "asset",
      asset_id: "fixtures/a.bin",
    });
  });
});

describe("contentLengthIssue", () => {
  it("detects missing and mismatch, case-insensitively", () => {
    expect(contentLengthIssue([], 5)).toBe("missing");
    expect(contentLengthIssue([["Content-Length", "4"]], 5)).toBe("mismatch");
    expect(contentLengthIssue([["content-length", "5"]], 5)).toBeNull();
    expect(contentLengthIssue([], 0)).toBeNull();
    expect(contentLengthIssue([], null)).toBeNull();
  });
});

describe("guessContentType", () => {
  it("guesses json / text / octet-stream", () => {
    expect(guessContentType('{"a":1}')).toBe("application/json");
    expect(guessContentType("hello")).toBe("text/plain");
    expect(guessContentType("   ")).toBe("application/octet-stream");
  });
});
