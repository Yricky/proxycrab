import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import test from "node:test";
const exec = promisify(execFile);
const script = new URL("./websocket-messages.mjs", import.meta.url).href;
test("WebSocket index uses per-direction cursor without fetching payload", async () => {
  const { stdout } = await exec(process.execPath, ["--input-type=module", "-e", `
    globalThis.fetch = async (url) => {
      if (url.pathname !== '/api/logs/7/websocket/messages') throw Error('wrong route');
      for (const [k,v] of Object.entries({session_id:'3',side:'response',after_id:'9',limit:'100'})) {
        if(url.searchParams.get(k) !== v) throw Error('wrong query '+k);
      }
      return new Response(JSON.stringify({ok:true,data:{messages:[],next_after_id:9,has_more:false}}));
    };
    process.argv = ['node', ${JSON.stringify(script)}, '--log-id','7','--session-id','3','--side','response','--after-id','9','--limit','100'];
    await import(${JSON.stringify(script)});
  `]);
  assert.deepEqual(JSON.parse(stdout), {messages:[],next_after_id:9,has_more:false});
});
for (const args of [["--log-id","7","--side","invalid"], ["--log-id","7","--side","request","--limit","2001"]]) {
  test(`rejects invalid arguments ${args.join(' ')}`, async () => {
    await assert.rejects(exec(process.execPath, [new URL('./websocket-messages.mjs', import.meta.url).pathname, ...args]));
  });
}

test("preserves generated-message lineage, drop state and callback errors", async () => {
  const message = {
    id: 4, sequence: 9, type: "text", status: "dropped",
    root: { side: "request", id: 1 }, parent: { side: "response", id: 3 },
    producer: { execution_id: 17, name: "reply", invocation_id: 20, send_index: 2 },
    dropped_by: { execution_id: 18, name: "guard", invocation_id: 21 },
    errors: [{ execution_id: 19, name: "inspect", invocation_id: 19, error: "budget exceeded" }],
  };
  const expected = { messages: [message], next_after_id: 4, has_more: false };
  const { stdout } = await exec(process.execPath, ["--input-type=module", "-e", `
    globalThis.fetch = async () => new Response(JSON.stringify({ok:true,data:${JSON.stringify(expected)}}));
    process.argv = ['node', ${JSON.stringify(script)}, '--log-id','7','--side','response'];
    await import(${JSON.stringify(script)});
  `]);
  assert.deepEqual(JSON.parse(stdout), expected);
});
