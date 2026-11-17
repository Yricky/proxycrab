import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import test from "node:test";
const exec = promisify(execFile);
const script = new URL("./session-page-import.mjs", import.meta.url).href;
test("page import sends raw bytes and replaces only when explicitly requested", async () => {
  for (const replace of [false, true]) {
    const { stdout } = await exec(process.execPath, [
      "--input-type=module",
      "-e",
      `
      import { mkdtemp,writeFile,rm } from 'node:fs/promises';
      import { tmpdir } from 'node:os';
      const root=await mkdtemp(tmpdir()+'/page-import-');
      const file=root+'/page.zip';await writeFile(file,new Uint8Array([0,255,1]));
      globalThis.fetch=async(url,init)=>{
        if(url.pathname!=='/api/session-pages'||init.method!=='POST')throw Error('wrong route');
        if((url.searchParams.get('replace')==='true')!==${replace})throw Error('wrong replacement');
        if(init.headers['content-type']!=='application/zip'||init.body[1]!==255)throw Error('wrong binary body');
        await rm(root,{recursive:true});
        return new Response(JSON.stringify({ok:true,data:{id:'demo'}}));
      };
      process.argv=['node',${JSON.stringify(script)},'--file',file${replace ? ",'--replace','true'" : ""}];
      await import(${JSON.stringify(script)});
    `,
    ]);
    assert.deepEqual(JSON.parse(stdout), { id: "demo" });
  }
});
