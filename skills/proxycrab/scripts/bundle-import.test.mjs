import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { mkdtemp, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
const exec = promisify(execFile);
const script = new URL("./bundle-import.mjs",import.meta.url).href;
test("import helper sends explicit IDs and file via multipart without a JSON content type",async()=>{
  const directory=await mkdtemp(join(tmpdir(),"bundle-helper-"));
  try {
    const file=join(directory,"bundle.zip");await writeFile(file,"zip");
    const {stdout}=await exec(process.execPath,["--input-type=module","-e",`
      globalThis.fetch=async(url,init)=>{
        if(url.pathname!='/api/imports'||init.body.get('accepted_intent_ids')!=='["chosen"]'||await init.body.get('file').text()!=='zip') throw Error('wrong payload');
        if(init.headers?.['content-type']) throw Error('multipart content-type must be generated');
        return new Response(JSON.stringify({ok:true,data:{import_id:null,files:[],sessions:[]}}));
      };
      process.argv=['node',${JSON.stringify(script)},'--file',${JSON.stringify(file)},'--accepted-intent-ids','["chosen"]'];
      await import(${JSON.stringify(script)});
    `]);assert.equal(JSON.parse(stdout).import_id,null);
  } finally {await rm(directory,{recursive:true,force:true});}
});
