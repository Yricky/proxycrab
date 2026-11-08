// 残留检查门禁：src 下 .ts/.mts/.vue 不允许出现硬编码中文 UI 文案。
// 跳过：注释行、src/i18n/locales/**、t() 调用的 key 本身、白名单（Lua 脚本模板等）。
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

const CN = /[\u4e00-\u9fff]/;
const CNG = /[\u4e00-\u9fff]+/g;

// 设计性豁免：中文是 i18n key 或有意不翻译的内容
// - permission-meta / 各选项表：表内存中文 key，消费处统一 t() 包裹
// - createPlaceholder：死配置（接口声明但从未被消费）
// - 简体中文：语言名专有名词按惯例不翻译
const ALLOW = [
  ["src/utils/permission-meta.ts", ""],
  ["src/components/PermissionEditor.vue", 'label: "'],
  ["src/components/PermissionEditor.vue", 'description: "'],
  ["src/components/ThemeToggle.vue", 'label: "'],
  ["src/components/HexViewer.vue", 'label: "'],
  ["src/components/CustomSelect.vue", 'placeholder: "'],
  ["src/components/CustomSelect.vue", 'ariaLabel: "'],
  ["src/windows/ColumnManagerWindow.vue", "createPlaceholder"],
  ["src/windows/FilterManagerWindow.vue", "createPlaceholder"],
  ["src/windows/RedactionManagerWindow.vue", "createPlaceholder"],
  ["src/windows/RoutingManagerWindow.vue", "createPlaceholder"],
  ["src/windows/SettingsWindow.vue", "简体中文"],
];

let failures = 0;
function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) {
      // locale 文件目录豁免
      if (path.endsWith(join("i18n", "locales"))) continue;
      walk(path, out);
    } else if (/\.(ts|mts|vue)$/.test(name) && !/\.test\.mts$/.test(name)) {
      out.push(path);
    }
  }
  return out;
}

for (const path of walk("src")) {
  const content = readFileSync(path, "utf8");
  // 去掉 t("…") / t('…') 调用的 key（多行安全：全文件级正则）
  const stripped = content
    .replace(/\bt\(\s*(['"])((?:[^'"\\\n]|\\.)*?)\1/g, (m) => m.replace(CNG, ""))
    // 去掉注释（行注释与块注释；对 URL 字符串误伤可接受——仅本检查用）
    .replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(CNG, ""))
    .replace(/(^|[^:])\/\/[^\n]*/g, (m) => m.replace(CNG, ""))
    .replace(/<!--[\s\S]*?-->/g, (m) => m.replace(CNG, ""));

  const lines = stripped.split("\n");
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (!CN.test(line)) continue;
    // 字符串字面量含中文，或 vue 模板文本节点（> 中文 <）含中文
    const inLiteral = /['"`][^'"`\n]*[\u4e00-\u9fff][^'"`\n]*['"`]/.test(line);
    const templateText = />[^<>]*[\u4e00-\u9fff][^<>]*</.test(line) && !/\{\{/.test(line.trim());
    if (inLiteral || templateText) {
      const allowed = ALLOW.some(
        ([allowPath, fragment]) => path === allowPath && line.includes(fragment),
      );
      if (allowed) continue;
      console.error(`${path}:${i + 1}: ${line.trim().slice(0, 100)}`);
      failures++;
    }
  }
}

process.exit(failures ? 1 : 0);
