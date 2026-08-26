#!/usr/bin/env node
import {
  apiRequest,
  assertAllowedArgs,
  buildRemoteFilter,
  matchesBuiltInCriteria,
  optionalInteger,
  parseArgs,
  printHelp,
  printJson,
  run,
  sessionQuery,
} from "./lib/common.mjs";

const sleep = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, [
    "session-id",
    "timeout-ms",
    "interval-ms",
    "uri",
    "method",
    "status",
    "source",
    "stage",
    "column-script",
    "filter-script",
    "input",
    "include-in-progress",
  ]);
  if (args.help) {
    printHelp(`
Usage: node log-wait.mjs [options]

Snapshot existing captures, then wait for a newer match and print its complete detail.

Timing:
  --timeout-ms N         Default 30000
  --interval-ms N        Default 500
  --include-in-progress  Return before the capture finishes

Matching (may be combined):
  --session-id ID
  --uri TEXT
  --method TEXT
  --status TEXT
  --source TEXT
  --stage TEXT
  --column-script NAME [--input TEXT]
  --filter-script NAME [--input TEXT]

One condition is evaluated remotely without changing the Session's persisted filter. Additional
built-in conditions are checked locally against full capture details.
`);
    return;
  }

  const sessionId = optionalInteger(args, "session-id", { min: 1 });
  const timeoutMs = optionalInteger(args, "timeout-ms", { min: 1, max: 3_600_000 }) ?? 30_000;
  const intervalMs = optionalInteger(args, "interval-ms", { min: 50, max: 60_000 }) ?? 500;
  const remoteFilter = buildRemoteFilter(args, { allowMultiple: true }) ?? {
    option: null,
    input: "",
  };

  const baseline = await apiRequest(args, "/api/logs/ids", {
    method: "POST",
    body: {
      ...(sessionId === undefined ? {} : { session_id: sessionId }),
      filter: { option: null, input: "" },
      limit: 10_000,
    },
  });
  const baselineMax =
    baseline.matched_ids.length === 0 ? 0 : Math.max(...baseline.matched_ids);
  const candidates = new Set();
  const deadline = Date.now() + timeoutMs;

  while (Date.now() <= deadline) {
    const result = await apiRequest(args, "/api/logs/ids", {
      method: "POST",
      body: {
        ...(sessionId === undefined ? {} : { session_id: sessionId }),
        filter: remoteFilter,
        min_id: baselineMax,
        limit: 10_000,
      },
    });
    for (const id of [...result.matched_ids, ...result.in_progress_ids]) {
      if (id > baselineMax) candidates.add(id);
    }

    const matched = new Set();
    const inProgress = new Set();
    const candidateIds = [...candidates];
    for (let index = 0; index < candidateIds.length; index += 10_000) {
      const batch = candidateIds.slice(index, index + 10_000);
      const membership = await apiRequest(args, "/api/logs/ids", {
        method: "POST",
        body: {
          ...(sessionId === undefined ? {} : { session_id: sessionId }),
          filter: remoteFilter,
          ids: batch,
        },
      });
      for (const id of membership.matched_ids) matched.add(id);
      for (const id of membership.in_progress_ids) inProgress.add(id);
    }

    for (const id of [...candidates].sort((left, right) => left - right)) {
      if (!matched.has(id) && !inProgress.has(id)) {
        candidates.delete(id);
        continue;
      }
      const detail = await apiRequest(args, `/api/logs/${id}${sessionQuery(sessionId)}`);
      const matches = matchesBuiltInCriteria(detail, args);
      const complete = detail.outcome !== "in_progress";
      let remoteMatch = matched.has(id);
      let remotelyInProgress = inProgress.has(id);
      if (complete && remotelyInProgress) {
        const finalMembership = await apiRequest(args, "/api/logs/ids", {
          method: "POST",
          body: {
            ...(sessionId === undefined ? {} : { session_id: sessionId }),
            filter: remoteFilter,
            ids: [id],
          },
        });
        remoteMatch = finalMembership.matched_ids.includes(id);
        remotelyInProgress = finalMembership.in_progress_ids.includes(id);
      }
      if (
        remoteMatch &&
        matches &&
        ((complete && !remotelyInProgress) ||
          (!complete && args["include-in-progress"] === true))
      ) {
        printJson(detail);
        return;
      }
      if (complete && !remotelyInProgress) candidates.delete(id);
    }
    await sleep(Math.min(intervalMs, Math.max(0, deadline - Date.now())));
  }

  throw new Error(`no new matching ProxyCrab capture within ${timeoutMs} ms`);
});
