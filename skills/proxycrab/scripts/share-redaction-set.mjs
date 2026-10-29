#!/usr/bin/env node
import { apiRequest, assertAllowedArgs, optionalString, parseArgs, printHelp, printJson, requiredInteger, run } from "./lib/common.mjs";
run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, ["session", "kind", "name", "clear", "inherit"]);
  if (args.help) {
    printHelp("Usage: node share-redaction-set.mjs --session ID [--kind page] (--name SCRIPT | --clear true | --inherit true) [--base-url URL]\nBind a script, explicitly disable redaction, or inherit local redaction of an already enabled Session share.");
    return;
  }
  const id = requiredInteger(args, "session", { min: 1 });
  if (args.kind !== undefined && args.kind !== "page") throw new Error("--kind only supports page; HAR link sharing has been removed");
  const name = optionalString(args, "name");
  const clear = optionalString(args, "clear");
  const inherit = optionalString(args, "inherit");
  if ([name, clear, inherit].filter(value => value !== undefined).length !== 1 || (clear !== undefined && clear !== "true") || (inherit !== undefined && inherit !== "true")) throw new Error("provide exactly one of --name, --clear true, or --inherit true");
  const current = await apiRequest(args, `/api/session-shares/${id}`);
  if (!current.enabled) throw new Error("share is not enabled");
  const result = await apiRequest(args, "/api/session-shares", { method: "POST", body: { session_id: id, session_share_config: { redaction_script: name ?? (clear ? "" : null), persistent: current.persistent ?? false } } });
  const { token: _token, ...state } = result;
  printJson(state);
});
