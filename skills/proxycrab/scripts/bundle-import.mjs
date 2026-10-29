#!/usr/bin/env node
import { readFile } from "node:fs/promises";
import { basename } from "node:path";
import { apiRequest, assertAllowedArgs, parseArgs, printHelp, printJson, requiredString, run } from "./lib/common.mjs";
run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, ["file", "accepted-intent-ids", "import-id", "resolutions-file"]);
  if (args.help) {
    printHelp('Usage: node bundle-import.mjs --file BUNDLE.zip --accepted-intent-ids \'["session-123"]\'\nOr: node bundle-import.mjs --import-id ID --resolutions-file choices.json\nInitial import executes immediately if no conflicts. Only explicitly user-accepted intent IDs may be submitted.');
    return;
  }
  if (args["import-id"] !== undefined) {
    if (args.file !== undefined || args["accepted-intent-ids"] !== undefined) throw Error("cannot combine import and resolution options");
    const resolutions = JSON.parse(await readFile(requiredString(args, "resolutions-file"), "utf8"));
    if (!Array.isArray(resolutions)) throw Error("resolutions must be an array");
    printJson(await apiRequest(args, `/api/imports/${encodeURIComponent(requiredString(args,"import-id"))}/resolve`, {method:"POST",body:resolutions}));
  } else {
    if (args["resolutions-file"] !== undefined) throw Error("--resolutions-file requires --import-id");
    const accepted = JSON.parse(requiredString(args,"accepted-intent-ids"));
    if (!Array.isArray(accepted) || !accepted.length || accepted.some(id=>typeof id !== "string" || !id) || new Set(accepted).size !== accepted.length) throw Error("explicit unique accepted intent IDs are required");
    const filename = requiredString(args,"file");
    const bytes = await readFile(filename);
    if (bytes.length > 128 * 1024 * 1024) throw Error("bundle exceeds 128 MiB");
    const form = new FormData(); form.set("file",new Blob([bytes],{type:"application/zip"}),basename(filename)); form.set("accepted_intent_ids",JSON.stringify(accepted));
    printJson(await apiRequest(args,"/api/imports",{method:"POST",body:form}));
  }
});
