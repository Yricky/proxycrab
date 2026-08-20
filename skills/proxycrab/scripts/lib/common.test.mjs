import assert from "node:assert/strict";
import test from "node:test";
import {
  buildRemoteFilter,
  normalizeBaseUrl,
  parseArgs,
  requestHeaders,
  REQUEST_TIMEOUT_MS,
  UsageError,
} from "./common.mjs";

test("normalizeBaseUrl keeps anonymous access limited to loopback", () => {
  const previous = process.env.PROXYCRAB_API_KEY;
  delete process.env.PROXYCRAB_API_KEY;
  try {
    assert.equal(
      normalizeBaseUrl({ "base-url": "http://127.0.0.1:18089/" }),
      "http://127.0.0.1:18089",
    );
    assert.throws(
      () => normalizeBaseUrl({ "base-url": "http://server.example:18089" }),
      /PROXYCRAB_API_KEY/,
    );
  } finally {
    if (previous === undefined) delete process.env.PROXYCRAB_API_KEY;
    else process.env.PROXYCRAB_API_KEY = previous;
  }
});

test("normalizeBaseUrl allows authenticated remote management APIs", () => {
  const previous = process.env.PROXYCRAB_API_KEY;
  process.env.PROXYCRAB_API_KEY = "pcrab_test_secret";
  try {
    assert.equal(
      normalizeBaseUrl({ "base-url": "http://server.example:18089/" }),
      "http://server.example:18089",
    );
  } finally {
    if (previous === undefined) delete process.env.PROXYCRAB_API_KEY;
    else process.env.PROXYCRAB_API_KEY = previous;
  }
});

test("requestHeaders omits authorization without an API key", () => {
  const previous = process.env.PROXYCRAB_API_KEY;
  delete process.env.PROXYCRAB_API_KEY;
  try {
    assert.equal(requestHeaders(), undefined);
    assert.deepEqual(requestHeaders({ json: true }), { "content-type": "application/json" });
  } finally {
    if (previous === undefined) delete process.env.PROXYCRAB_API_KEY;
    else process.env.PROXYCRAB_API_KEY = previous;
  }
});

test("requestHeaders reads the API key only from the environment", () => {
  const previous = process.env.PROXYCRAB_API_KEY;
  process.env.PROXYCRAB_API_KEY = "pcrab_test_secret";
  try {
    assert.deepEqual(requestHeaders(), { authorization: "Bearer pcrab_test_secret" });
    assert.equal(REQUEST_TIMEOUT_MS, 40_000);
  } finally {
    if (previous === undefined) delete process.env.PROXYCRAB_API_KEY;
    else process.env.PROXYCRAB_API_KEY = previous;
  }
});

test("requestHeaders rejects newline injection", () => {
  const previous = process.env.PROXYCRAB_API_KEY;
  process.env.PROXYCRAB_API_KEY = "pcrab_test\nforged";
  try {
    assert.throws(() => requestHeaders(), UsageError);
  } finally {
    if (previous === undefined) delete process.env.PROXYCRAB_API_KEY;
    else process.env.PROXYCRAB_API_KEY = previous;
  }
});

test("column filters use the regex wire flag", () => {
  assert.deepEqual(
    buildRemoteFilter(parseArgs(["--uri", "^/api", "--regex"])),
    {
      option: { kind: "column", column: { kind: "uri" }, regex: true },
      input: "^/api",
    },
  );
  assert.deepEqual(buildRemoteFilter(parseArgs(["--method", "GET"])), {
    option: { kind: "column", column: { kind: "method" }, regex: false },
    input: "GET",
  });
});
