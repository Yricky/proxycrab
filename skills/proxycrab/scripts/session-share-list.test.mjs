import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import test from "node:test";
const exec = promisify(execFile);
const script = new URL("./session-share-list.mjs", import.meta.url).href;
test("lists enabled share summaries in one read", async () => {
  const { stdout } = await exec(process.execPath, ["--input-type=module", "-e", `
    globalThis.fetch = async (url, init) => {
      if (url.pathname !== '/api/session-shares' || init.method !== 'GET') throw Error('wrong route');
      return new Response(JSON.stringify({ok:true,data:{shares:[{session_id:7,persistent:true}]}}));
    };
    process.argv = ['node', ${JSON.stringify(script)}];
    await import(${JSON.stringify(script)});
  `]);
  assert.deepEqual(JSON.parse(stdout), { shares: [{ session_id: 7, persistent: true }] });
});
