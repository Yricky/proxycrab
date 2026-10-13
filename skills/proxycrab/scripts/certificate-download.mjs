#!/usr/bin/env node
import {
  apiDownloadRequest, assertAllowedArgs, parseArgs, printHelp, printJson,
  requiredString, run, writeBytes,
} from "./lib/common.mjs";

run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, ["out"]);
  if (args.help) {
    printHelp(`
Usage:
  node certificate-download.mjs --out PATH [--base-url URL]

Download the current public CA certificate from the management HTTP service.
The proxy does not need to be running or configured on the client.
`);
    return;
  }
  const out = requiredString(args, "out");
  const { bytes } = await apiDownloadRequest(args, "/ca.crt");
  const path = await writeBytes(out, bytes);
  printJson({ path, bytes: bytes.length });
});
