import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import process from "node:process";
const host = process.env.TAURI_DEV_HOST;
const projectRoot = process.cwd();
const sourceRoot = `${projectRoot}/src`;

// vue-icons-plus packs are single files exporting thousands of icon components
// via `useGenIcon(...)` factory calls. Rolldown cannot prove these calls pure once
// the shared factory module is split into a common chunk (multi-entry builds), so
// every icon would be kept. Annotate the calls as pure to restore tree-shaking.
const pureIconsPlugin = {
  name: "pure-icons",
  transform(code: string, id: string) {
    if (id.includes("vue-icons-plus") && id.endsWith(".mjs")) {
      return code.replaceAll("= useGenIcon(", "= /*@__PURE__*/ useGenIcon(");
    }
  },
};

const monacoWorkersShimPlugin = {
  name: "monaco-workers-shim",
  enforce: "pre" as const,
  resolveId(id: string, importer?: string) {
    if (
      id.endsWith("internal/common/workers.js") &&
      importer?.includes("monaco-editor")
    ) {
      return `${projectRoot}/src/monaco-workers-shim.ts`;
    }
  },
};

// https://vite.dev/config/
export default defineConfig({
  root: sourceRoot,
  plugins: [vue(), pureIconsPlugin, monacoWorkersShimPlugin],
  optimizeDeps: {
    rolldownOptions: {
      // dev 预打包不走上面的插件管线，需单独注入同一个 shim
      plugins: [monacoWorkersShimPlugin],
    },
  },
  resolve: {
    alias: [
      // monaco-editor 的 exports map 未暴露 css 子路径，通过别名直取
      { find: "monaco-editor/codicon.css", replacement: `${projectRoot}/node_modules/monaco-editor/esm/vs/base/browser/ui/codicons/codicon/codicon.css` },
      { find: "monaco-editor/codicon-modifiers.css", replacement: `${projectRoot}/node_modules/monaco-editor/esm/vs/base/browser/ui/codicons/codicon/codicon-modifiers.css` },
      // workers.js 会副作用引入全部 editor contrib，替换为最小实现（见 shim 注释）。
      // 该引用是 monaco 包内的相对路径，alias 无法正确处理，需用 resolveId 钩子。
      // （见 monacoWorkersShimPlugin）
    ],
  },
  build: {
    outDir: `${projectRoot}/dist`,
    emptyOutDir: true,
    rollupOptions: {
      input: {
        app: `${sourceRoot}/index.html`,
        session: `${sourceRoot}/session.html`,
      },
    },
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
});
