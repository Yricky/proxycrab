import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { promisify } from "node:util";
import test from "node:test";
const exec = promisify(execFile);
const script = new URL("./redaction-upsert.mjs", import.meta.url).href;

async function runUpsert(respond) {
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
    process.argv = ["node", ${JSON.stringify(script)}, "--name", "mask", "--file", ${JSON.stringify(file)}];
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
