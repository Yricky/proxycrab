import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import test from "node:test";

const execFileAsync = promisify(execFile);
const scriptUrl = new URL("./session-list.mjs", import.meta.url).href;

for (const active_session_id of [3, null]) {
  test(`session-list preserves columns and active_session_id=${active_session_id}`, async () => {
    const sessions = [
      { id: 3, name: "capture", created_at: 10, description: null },
      { id: 4, name: "other", created_at: 20, description: "saved" },
    ];
    const responses = {
      "/api/sessions": { sessions, active_session_id },
      "/api/session-view?session_id=3": {
        columns: [{ kind: "method" }, { kind: "custom", script_name: "auth-token" }],
      },
      "/api/session-view?session_id=4": { columns: [{ kind: "uri" }] },
    };
    const { stdout, stderr } = await execFileAsync(process.execPath, [
      "--input-type=module", "-e", `
        const responses = ${JSON.stringify(responses)};
        globalThis.fetch = async (url) => {
          const key = url.pathname + url.search;
          if (!Object.hasOwn(responses, key)) throw new Error("Unexpected request: " + key);
          return new Response(JSON.stringify({ ok: true, data: responses[key] }));
        };
        process.argv = [process.execPath, ${JSON.stringify(scriptUrl)}, "--base-url", "http://127.0.0.1:18089"];
        await import(${JSON.stringify(scriptUrl)});
      `,
    ]);
    assert.equal(stderr, "");
    assert.deepEqual(JSON.parse(stdout), {
      sessions: [
        { ...sessions[0], columns: ["method", "auth-token"] },
        { ...sessions[1], columns: ["uri"] },
      ],
      active_session_id,
    });
  });
}

test("session-list emits an empty list with a null active ID", async () => {
  const { stdout, stderr } = await execFileAsync(process.execPath, [
    "--input-type=module", "-e", `
      globalThis.fetch = async (url) => {
        if (url.pathname !== "/api/sessions") throw new Error("Unexpected request");
        return new Response(JSON.stringify({ ok: true, data: { sessions: [], active_session_id: null } }));
      };
      process.argv = [process.execPath, ${JSON.stringify(scriptUrl)}, "--base-url", "http://127.0.0.1:18089"];
      await import(${JSON.stringify(scriptUrl)});
    `,
  ]);
  assert.equal(stderr, "");
  assert.deepEqual(JSON.parse(stdout), { sessions: [], active_session_id: null });
});
