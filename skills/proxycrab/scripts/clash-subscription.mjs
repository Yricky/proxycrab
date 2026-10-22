#!/usr/bin/env node
import { isIPv4 } from "node:net";
import {
  apiRequest, assertAllowedArgs, optionalString, parseArgs, printHelp, printJson,
  readText, requiredString, run, UsageError,
} from "./lib/common.mjs";

run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, ["action", "server", "file"]);
  if (args.help) {
    printHelp(`
Usage:
  node clash-subscription.mjs --action status [--base-url URL]
  node clash-subscription.mjs --action enable --server IPV4 [--base-url URL]
  node clash-subscription.mjs --action disable [--base-url URL]
  node clash-subscription.mjs --action config [--base-url URL]
  node clash-subscription.mjs --action save-config --file PATH [--base-url URL]
  node clash-subscription.mjs --action reset-config [--base-url URL]

Manage the temporary Clash subscription and its complete YAML config without changing proxy lifecycle.
The subscription is served at /clash.yaml without a download token while enabled. The config file
must contain {{server}} and {{port}} placeholders; downloads substitute the selected IP and current
proxy port. save-config validates and writes the file; reset-config restores the default.
`);
    return;
  }
  const action = requiredString(args, "action");
  const server = optionalString(args, "server");
  const file = optionalString(args, "file");
  if (!["status", "enable", "disable", "config", "save-config", "reset-config"].includes(action)) {
    throw new UsageError("--action must be status, enable, disable, config, save-config, or reset-config");
  }
  if (action === "enable") {
    const first = Number(server?.split(".")[0]);
    if (!server || !isIPv4(server) || first === 0 || first === 127 || (first >= 224 && first <= 239) || server === "255.255.255.255") {
      throw new UsageError("--server must be a non-loopback unicast IPv4 address");
    }
  } else if (server !== undefined) {
    throw new UsageError("--server is only valid with --action enable");
  }
  if (action === "save-config") {
    if (!file) throw new UsageError("--file is required with --action save-config");
  } else if (file !== undefined) {
    throw new UsageError("--file is only valid with --action save-config");
  }
  if (["status", "enable", "disable"].includes(action)) {
    const state = await apiRequest(args, "/api/clash-subscription", {
      method: { status: "GET", enable: "PUT", disable: "DELETE" }[action],
      ...(action === "enable" ? { body: { server } } : {}),
    });
    printJson({ enabled: state.enabled, ...(state.server ? { server: state.server } : {}) });
    return;
  }
  let yaml;
  if (action === "save-config") {
    try {
      yaml = await readText(file);
    } catch (error) {
      throw new UsageError(`cannot read --file ${file}: ${error.message}`);
    }
  }
  const config = await apiRequest(args, "/api/clash-config", {
    method: { config: "GET", "save-config": "PUT", "reset-config": "DELETE" }[action],
    ...(action === "save-config" ? { body: { yaml } } : {}),
  });
  printJson({
    path: config.path,
    yaml: config.yaml,
    ...(config.default_yaml ? { default_yaml: config.default_yaml } : {}),
  });
});
