#!/usr/bin/env node
import { apiRequest, assertAllowedArgs, optionalString, parseArgs, printHelp, printJson, requiredInteger, run } from "./lib/common.mjs";
run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, ["session", "kind", "name", "clear"]);
  if (args.help) {
    printHelp("Usage: node share-redaction-set.mjs --session ID [--kind page] (--name SCRIPT | --clear true) [--base-url URL]\nBind or clear the redaction script of an already enabled Session share.");
    return;
  }
  const id = requiredInteger(args, "session", { min: 1 });
  if (args.kind !== undefined && args.kind !== "page") throw new Error("--kind only supports page; HAR link sharing has been removed");
  const name = optionalString(args, "name");
  const clear = optionalString(args, "clear");
  if ((name === undefined) === (clear === undefined) || (clear !== undefined && clear !== "true")) throw new Error("provide exactly one of --name or --clear true");
  const current = await apiRequest(args, `/api/session-shares/${id}`);
  if (!current.enabled) throw new Error("share is not enabled");
  const result = await apiRequest(args, "/api/session-shares", { method: "POST", body: { session_id: id, session_share_config: { redaction_script: name ?? null, persistent: current.persistent ?? false } } });
  const { token: _token, ...state } = result;
  printJson(state);
});
