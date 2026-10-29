import { strFromU8, unzipSync } from "fflate";
import type { BundleIntent } from "../api/types";

export const MAX_BUNDLE_BYTES = 128 * 1024 * 1024;
export function resourceMatches(rules: string[], ids: string[]): string[] {
  return ids.filter((id) =>
    rules.some((rule) => (rule.endsWith("/") ? id.startsWith(rule) : id === rule)),
  );
}
export function parseBundle(bytes: Uint8Array): { intents: (BundleIntent & { name: string })[] } {
  if (bytes.length > MAX_BUNDLE_BYTES) throw new Error("分享包超过 128 MiB");
  let total = 0,
    count = 0;
  const files = unzipSync(bytes, {
    filter: (file) => {
      if (++count > 4096) throw new Error("分享包文件过多");
      total += file.originalSize;
      if (total > MAX_BUNDLE_BYTES) throw new Error("分享包解压后超过 128 MiB");
      if (
        file.name.includes("\\") ||
        file.name
          .replace(/\/$/, "")
          .split("/")
          .some((part) => !part || part === "." || part === "..")
      )
        throw new Error("分享包包含非法路径");
      return file.name === "intents.json" || /^sessions\/[^/]+\/metadata\.json$/.test(file.name);
    },
  });
  const raw = files["intents.json"];
  if (!raw) throw new Error("分享包缺少 intents.json");
  const manifest = JSON.parse(strFromU8(raw)) as { version: number; intents: BundleIntent[] };
  if (manifest.version !== 1 || !Array.isArray(manifest.intents) || !manifest.intents.length)
    throw new Error("分享包意图格式不受支持");
  const ids = new Set<string>();
  return {
    intents: manifest.intents.map((intent) => {
      if (
        !intent ||
        typeof intent.id !== "string" ||
        !intent.id ||
        ids.has(intent.id) ||
        typeof intent.type !== "string" ||
        typeof intent.path !== "string" ||
        (intent.assets !== undefined &&
          (!Array.isArray(intent.assets) || intent.assets.some((id) => typeof id !== "string")))
      )
        throw new Error("分享包意图格式错误或 ID 重复");
      ids.add(intent.id);
      let name = intent.path;
      if (intent.type === "session") {
        if (!/^sessions\/\d+$/.test(intent.path)) throw new Error("Session 意图路径错误");
        const metadata = files[`${intent.path}/metadata.json`];
        if (!metadata) throw new Error(`缺少 ${intent.path}/metadata.json`);
        name = (JSON.parse(strFromU8(metadata)) as { name: string }).name;
        if (typeof name !== "string") throw new Error("Session 名称格式错误");
      }
      return { ...intent, assets: intent.assets ?? [], name };
    }),
  };
}
