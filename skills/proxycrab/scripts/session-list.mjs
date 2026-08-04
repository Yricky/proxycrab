#!/usr/bin/env node
import {
  apiRequest,
  assertAllowedArgs,
  parseArgs,
  printHelp,
  printJson,
  run,
  sessionQuery,
} from "./lib/common.mjs";

run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, []);
  if (args.help) {
    printHelp(`
Usage: node session-list.mjs [--base-url URL]

List all ProxyCrab Sessions and their configured table column names.
`);
    return;
  }
  const sessions = await apiRequest(args, "/api/sessions");
  const sessionsWithColumns = await Promise.all(
    sessions.map(async (session) => {
      const view = await apiRequest(args, `/api/session-view${sessionQuery(session.id)}`);
      return {
        ...session,
        columns: view.columns.map((column) => column.script_name ?? column.kind),
      };
    }),
  );
  printJson(sessionsWithColumns);
});
