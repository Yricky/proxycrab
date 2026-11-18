import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import test from "node:test";

const execFileAsync = promisify(execFile);
const fixture = new URL("../evals/fixtures/test-filter.lua", import.meta.url);

for (const sessionId of [3, undefined]) {
  test(`inline query preserves Lua and empty input with Session ${sessionId ?? "default"}`, async () => {
    const script = new URL("./log-filter.mjs", import.meta.url).href;
    const source = await readFile(fixture, "utf8");
    const result = { matched_ids: [199, 101], in_progress_ids: [199] };
    const argv = [
      "--file",
      fileURLToPath(fixture),
      "--min-id",
      "100",
      "--max-id",
      "200",
      "--limit",
      "2",
    ];
    if (sessionId !== undefined) argv.push("--session-id", String(sessionId));
    const { stdout, stderr } = await execFileAsync(process.execPath, [
      "--input-type=module",
      "-e",
      `
      import assert from 'node:assert/strict';
      globalThis.fetch = async (url, init) => {
        assert.equal(url.pathname, '/api/logs/ids');
        assert.equal(url.search, '');
        assert.equal(init.method, 'POST');
        assert.equal(init.headers['content-type'], 'application/json');
        assert.deepEqual(JSON.parse(init.body), ${JSON.stringify({
          session_id: sessionId,
          filter: { option: { kind: "script_content", content: source }, input: "" },
          min_id: 100,
          max_id: 200,
          limit: 2,
        })});
        return Response.json({ok: true, data: ${JSON.stringify(result)}});
      };
      process.argv = [process.execPath, ${JSON.stringify(script)}, ...${JSON.stringify(argv)}];
      await import(${JSON.stringify(script)});
    `,
    ]);
    assert.equal(stderr, "");
    assert.deepEqual(JSON.parse(stdout), result);
  });
}

for (const matched of [true, false]) {
  test(`saved filter debug uses inline source and a single-ID range: matched=${matched}`, async () => {
    const script = new URL("./filter-debug.mjs", import.meta.url).href;
    const { stdout, stderr } = await execFileAsync(process.execPath, [
      "--input-type=module",
      "-e",
      `
      import assert from 'node:assert/strict';
      let calls = 0;
      globalThis.fetch = async (url, init) => {
        calls++;
        if (calls === 1) {
          assert.equal(url.pathname, '/api/filter-scripts/business%2Ferrors');
          return Response.json({ok: true, data: {content: 'return false\\n'}});
        }
        assert.equal(calls, 2);
        assert.equal(url.pathname, '/api/logs/ids');
        assert.deepEqual(JSON.parse(init.body), {
          session_id: 3, min_id: 1041, max_id: 1043,
          filter: {option: {kind: 'script_content', content: 'return false\\n'}, input: '  x & y  '}
        });
        return Response.json({ok: true, data: {matched_ids: ${matched ? "[1042]" : "[]"}, in_progress_ids: []}});
      };
      process.argv = [process.execPath, ${JSON.stringify(script)}, '--name', 'business/errors',
        '--session-id', '3', '--log-id', '1042', '--input', '  x & y  '];
      await import(${JSON.stringify(script)});
    `,
    ]);
    assert.equal(stderr, "");
    assert.deepEqual(JSON.parse(stdout), { name: "business/errors", matched });
  });
}
