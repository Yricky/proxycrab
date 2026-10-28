#!/usr/bin/env node
import { apiRequest, assertAllowedArgs, optionalInteger, parseArgs, printHelp, printJson, requiredString, run, UsageError } from "./lib/common.mjs";

run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, ["session-id", "log-id", "side", "after-id", "limit"]);
  if (args.help) {
    printHelp(`Usage: node websocket-messages.mjs --log-id ID --side request|response [--session-id ID] [--after-id ID] [--limit N]
Read WebSocket message metadata only. request=C2S, response=S2C. IDs are per direction.
Use next_after_id for subsequent pages and polling; has_more only describes the current snapshot.
Interception metadata includes sequence, status, root/parent {side,id}, producer/dropped_by and errors.
Use root for the network source and parent for the direct trigger; producer identifies the source
execution_id, callback invocation_id and send_index. Read source snapshots from the handshake log
with interceptor-get.mjs --log-id HANDSHAKE_ID --execution-id EXECUTION_ID.
Read content with body-get.mjs --log-id ID --message-id ID --side request|response --output FILE.`);
    return;
  }
  const id = optionalInteger(args, "log-id", { min: 1 });
  if (id === undefined) throw new UsageError("--log-id is required");
  const side = requiredString(args, "side");
  if (!["request", "response"].includes(side)) throw new UsageError("--side must be request or response");
  const params = new URLSearchParams({side});
  for (const [arg, key, min, max] of [["session-id", "session_id", 1, Number.MAX_SAFE_INTEGER], ["after-id", "after_id", 0, 4294967295], ["limit", "limit", 1, 2000]]) {
    const value = optionalInteger(args, arg, { min, max });
    if (value !== undefined) params.set(key, String(value));
  }
  printJson(await apiRequest(args, `/api/logs/${id}/websocket/messages?${params}`));
});
