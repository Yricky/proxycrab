#!/usr/bin/env node
import { isIPv4 } from "node:net";
import {
  apiRequest, assertAllowedArgs, optionalString, parseArgs, printHelp, printJson,
  requiredString, run, UsageError,
} from "./lib/common.mjs";

run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, ["action", "server"]);
  if (args.help) {
    printHelp(`
Usage:
  node clash-subscription.mjs --action status [--base-url URL]
  node clash-subscription.mjs --action enable --server IPV4 [--base-url URL]
  node clash-subscription.mjs --action disable [--base-url URL]

Manage the temporary Clash subscription without changing proxy lifecycle.
Output omits the download token; copy the link from certificate management > 2. 代理连接.
`);
    return;
  }
  const action = requiredString(args, "action");
  const server = optionalString(args, "server");
  if (!["status", "enable", "disable"].includes(action)) throw new UsageError("--action must be status, enable, or disable");
  if (action === "enable") {
    const first = Number(server?.split(".")[0]);
    if (!server || !isIPv4(server) || first === 0 || first === 127 || (first >= 224 && first <= 239) || server === "255.255.255.255") {
      throw new UsageError("--server must be a non-loopback unicast IPv4 address");
    }
  } else if (server !== undefined) {
    throw new UsageError("--server is only valid with --action enable");
  }
  const state = await apiRequest(args, "/api/clash-subscription", {
    method: { status: "GET", enable: "PUT", disable: "DELETE" }[action],
    ...(action === "enable" ? { body: { server } } : {}),
  });
  printJson({ enabled: state.enabled, ...(state.server ? { server: state.server } : {}) });
});
