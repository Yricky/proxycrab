#!/usr/bin/env node
import { apiRequest, assertAllowedArgs, optionalString, parseArgs, printHelp, printJson, requiredInteger, requiredString, run } from "./lib/common.mjs";
run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, ["session", "kind", "name", "clear"]);
  if (args.help) {
    printHelp("Usage: node share-redaction-set.mjs --session ID --kind page|har (--name SCRIPT | --clear true) [--base-url URL]\nBind or clear the redaction script of an already enabled share.");
    return;
  }
  const id = requiredInteger(args, "session", { min: 1 });
  const kind = requiredString(args, "kind");
  if (kind !== "page" && kind !== "har") throw new Error("--kind must be page or har");
  const name = optionalString(args, "name");
  const clear = optionalString(args, "clear");
  if ((name === undefined) === (clear === undefined) || (clear !== undefined && clear !== "true")) throw new Error("provide exactly one of --name or --clear true");
  const result = await apiRequest(args, `/api/${kind === "page" ? "session-shares" : "session-har-shares"}/${id}/redaction`, { method: "PUT", body: { redaction_script: name ?? null } });
  // Share tokens are credentials; binding changes never print them.
  const { token: _token, ...state } = result;
  printJson(state);
});
