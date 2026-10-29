import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { promisify } from "node:util";
import test from "node:test";
const exec = promisify(execFile);
const script = new URL("./redaction-upsert.mjs", import.meta.url).href;

async function runUpsert(respond, name = "mask") {
  const dir = await mkdtemp(path.join(tmpdir(), "redaction-upsert-"));
  const file = path.join(dir, "mask.lua");
  await writeFile(file, "context.req.headers:set('authorization', '***')\n");
  const { stdout } = await exec(process.execPath, ["--input-type=module", "-e", `
    globalThis.fetch = async (url, init = {}) => {
      const method = init.method ?? "GET";
      const pathname = new URL(url).pathname;
      if (method === "GET") throw new Error("read attempted: " + pathname);
      return globalThis.__respond(method, pathname, init.body);
    };
    const respond = ${respond};
    globalThis.__respond = respond;
    process.argv = ["node", ${JSON.stringify(script)}, "--name", ${JSON.stringify(name)}, "--file", ${JSON.stringify(file)}];
    await import(${JSON.stringify(script)});
  `]);
  return JSON.parse(stdout);
}

test("creates a script without reading it by name", async () => {
  const result = await runUpsert(`(method, pathname) => {
    if (method !== "POST" || pathname !== "/api/redaction-scripts") throw new Error("unexpected " + method + " " + pathname);
    return new Response(JSON.stringify({ ok: true, data: {} }));
  }`);
  assert.deepEqual(result, { action: "created", name: "mask" });
});

test("replaces an existing script after a create conflict", async () => {
  const result = await runUpsert(`(method, pathname) => {
    if (method === "POST" && pathname === "/api/redaction-scripts") {
      return new Response(JSON.stringify({ ok: false, error: { code: "conflict", message: "script mask already exists" } }), { status: 409 });
    }
    if (method === "PUT" && pathname === "/api/redaction-scripts/mask") {
      return new Response(JSON.stringify({ ok: true, data: {} }));
    }
    throw new Error("unexpected " + method + " " + pathname);
  }`);
  assert.deepEqual(result, { action: "updated", name: "mask" });
});

test("disabled management is reported without attempting an update", async () => {
  await assert.rejects(runUpsert(`(method, pathname) => {
    if (method !== "POST" || pathname !== "/api/redaction-scripts") throw new Error("unexpected retry");
    return new Response(JSON.stringify({ ok: false, error: {
      code: "redaction_management_disabled", message: "脱敏接口管理功能已禁用"
    } }), { status: 403 });
  }`), (error) => {
    assert.equal(error.code, 1);
    assert.match(error.stderr, /redaction_management_disabled/);
    assert.match(error.stderr, /脱敏接口管理功能已禁用/);
    assert.doesNotMatch(error.stderr, /unexpected retry/);
    return true;
  });
});


test("nested script identity is raw in JSON and encoded as one URL parameter", async () => {
  const result = await runUpsert(`(method, pathname, body) => {
    if (method === "POST" && pathname === "/api/redaction-scripts") {
      if (JSON.parse(body).name !== "business/mask") throw new Error("incorrect name");
      return new Response(JSON.stringify({ ok: false, error: { code: "conflict", message: "exists" } }), { status: 409 });
    }
    if (method === "PUT" && pathname === "/api/redaction-scripts/business%2Fmask") {
      return new Response(JSON.stringify({ ok: true, data: {} }));
    }
    throw new Error("unexpected " + method + " " + pathname);
  }`, "business/mask");
  assert.deepEqual(result, { action: "updated", name: "business/mask" });
});
