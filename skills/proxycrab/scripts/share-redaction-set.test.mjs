import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import test from "node:test";
const exec = promisify(execFile);
const script = new URL("./share-redaction-set.mjs", import.meta.url).href;
for (const kind of ["page", "har"]) {
  test(`sets ${kind} binding without printing token`, async () => {
    const { stdout } = await exec(process.execPath, ["--input-type=module", "-e", `
      globalThis.fetch = async (url, init) => {
        if ('${kind}' === 'page') {
          if (url.pathname === '/api/session-shares/7') return new Response(JSON.stringify({ok:true,data:{session_id:7,enabled:true,persistent:true}}));
          if (url.pathname !== '/api/session-shares' || init.method !== 'POST') throw Error('wrong route');
          const body = JSON.parse(init.body);
          if (body.session_id !== 7 || body.session_share_config.redaction_script !== 'mask' || body.session_share_config.persistent !== true) throw Error('lost share config');
        } else {
          if (url.pathname !== '/api/session-har-shares/7/redaction') throw Error('wrong route');
          if (init.method !== 'PUT' || JSON.parse(init.body).redaction_script !== 'mask') throw Error('wrong binding');
        }
        return new Response(JSON.stringify({ok:true,data:{session_id:7,enabled:true,redaction_script:'mask',token:'must-not-print'}}));
      };
      process.argv = ['node', ${JSON.stringify(script)}, '--session', '7', '--kind', '${kind}', '--name', 'mask'];
      await import(${JSON.stringify(script)});
    `]);
    assert.deepEqual(JSON.parse(stdout), {session_id:7, enabled:true, redaction_script:"mask"});
    assert.ok(!stdout.includes("must-not-print"));
  });
}
