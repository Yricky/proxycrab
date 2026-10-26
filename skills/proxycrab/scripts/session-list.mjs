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

List all ProxyCrab Sessions, their configured table column names, and active_session_id.
Outputs { sessions: [...], active_session_id: number | null }.
`);
    return;
  }
  const { sessions, active_session_id } = await apiRequest(args, "/api/sessions");
  const sessionsWithColumns = await Promise.all(
    sessions.map(async (session) => {
      const view = await apiRequest(args, `/api/session-view${sessionQuery(session.id)}`);
      return {
        ...session,
        columns: view.columns.map((column) => column.script_name ?? column.kind),
      };
    }),
  );
  printJson({ sessions: sessionsWithColumns, active_session_id });
});
