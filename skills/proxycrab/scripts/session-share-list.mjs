#!/usr/bin/env node
import { apiRequest, assertAllowedArgs, parseArgs, printHelp, printJson, run } from "./lib/common.mjs";
run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, []);
  if (args.help) {
    printHelp("Usage: node session-share-list.mjs [--base-url URL]\nList enabled, non-archived Session page shares without tokens.");
    return;
  }
  printJson(await apiRequest(args, "/api/session-shares"));
});
