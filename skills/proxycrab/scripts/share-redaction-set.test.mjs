import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import test from "node:test";
const exec = promisify(execFile);
const script = new URL("./share-redaction-set.mjs", import.meta.url).href;
test("sets page binding while preserving persistence and never printing token", async () => {
  const { stdout } = await exec(process.execPath, ["--input-type=module", "-e", `
    globalThis.fetch = async (url, init) => {
      if (url.pathname === '/api/session-shares/7') return new Response(JSON.stringify({ok:true,data:{session_id:7,enabled:true,persistent:true}}));
      if (url.pathname !== '/api/session-shares' || init.method !== 'POST') throw Error('wrong route');
      const body = JSON.parse(init.body);
      if (body.session_id !== 7 || body.session_share_config.redaction_script !== 'mask' || body.session_share_config.persistent !== true) throw Error('lost share config');
      return new Response(JSON.stringify({ok:true,data:{session_id:7,enabled:true,redaction_script:'mask',token:'must-not-print'}}));
    };
    process.argv = ['node', ${JSON.stringify(script)}, '--session', '7', '--kind', 'page', '--name', 'mask'];
    await import(${JSON.stringify(script)});
  `]);
  assert.deepEqual(JSON.parse(stdout), {session_id:7, enabled:true, redaction_script:"mask"});
  assert.ok(!stdout.includes("must-not-print"));
});
test("removed HAR mode is rejected before any request", async () => {
  await assert.rejects(exec(process.execPath, ["--input-type=module", "-e", `
    globalThis.fetch = async () => { throw Error('must not request'); };
    process.argv = ['node', ${JSON.stringify(script)}, '--session', '7', '--kind', 'har', '--name', 'mask'];
    await import(${JSON.stringify(script)});
  `]), (error) => { assert.match(error.stderr, /HAR link sharing has been removed/); return true; });
});
for (const [flag, value] of [["clear", ""], ["inherit", null]]) {
  test(`${flag} uses the POST config contract and preserves persistence`, async () => {
    const { stdout } = await exec(process.execPath, ["--input-type=module", "-e", `
      globalThis.fetch = async (url, init) => {
        if (url.pathname === '/api/session-shares/7') return new Response(JSON.stringify({ok:true,data:{enabled:true,persistent:true}}));
        if (url.pathname !== '/api/session-shares' || init.method !== 'POST') throw Error('removed route used');
        const config = JSON.parse(init.body).session_share_config;
        if (config.redaction_script !== ${JSON.stringify(value)} || !config.persistent) throw Error('incorrect policy');
        return new Response(JSON.stringify({ok:true,data:{enabled:true,token:'private'}}));
      };
      process.argv = ['node', ${JSON.stringify(script)}, '--session', '7', '--${flag}', 'true'];
      await import(${JSON.stringify(script)});
    `]);
    assert.deepEqual(JSON.parse(stdout), {enabled:true});
  });
}
