// Monaco singleton with Vite worker wiring. Import this module instead of
// "monaco-editor" directly so the environment is always configured first.
import * as monaco from "monaco-editor/editor/editor.api";
// monaco-editor 的 exports map 将 "./*" 映射到 "./esm/vs/*.js"，
// 因此这里的子路径不带 esm/vs 前缀。
// 仅引入实际使用的编辑器特性与语言，避免 editor.main 全量引入（含 monaco-lsp-client、
// css/html/ts 语言服务与全部 90+ 语言定义）。
import "monaco-editor/codicon.css";
import "monaco-editor/codicon-modifiers.css";
import "monaco-editor/editor/browser/coreCommands";
import "monaco-editor/editor/contrib/bracketMatching/browser/bracketMatching";
import "monaco-editor/editor/contrib/caretOperations/browser/caretOperations";
import "monaco-editor/editor/contrib/clipboard/browser/clipboard";
import "monaco-editor/editor/contrib/comment/browser/comment";
import "monaco-editor/editor/contrib/contextmenu/browser/contextmenu";
import "monaco-editor/editor/contrib/cursorUndo/browser/cursorUndo";
import "monaco-editor/editor/contrib/dnd/browser/dnd";
import "monaco-editor/editor/contrib/find/browser/findController";
import "monaco-editor/editor/contrib/folding/browser/folding";
import "monaco-editor/editor/contrib/hover/browser/hoverContribution";
import "monaco-editor/editor/contrib/indentation/browser/indentation";
import "monaco-editor/editor/contrib/inPlaceReplace/browser/inPlaceReplace";
import "monaco-editor/editor/contrib/linesOperations/browser/linesOperations";
import "monaco-editor/editor/contrib/links/browser/links";
import "monaco-editor/editor/contrib/multicursor/browser/multicursor";
import "monaco-editor/editor/contrib/smartSelect/browser/smartSelect";
import "monaco-editor/editor/contrib/suggest/browser/suggestController";
import "monaco-editor/editor/contrib/unusualLineTerminators/browser/unusualLineTerminators";
import "monaco-editor/editor/contrib/wordHighlighter/browser/wordHighlighter";
import "monaco-editor/editor/contrib/wordOperations/browser/wordOperations";
import "monaco-editor/editor/contrib/wordPartOperations/browser/wordPartOperations";
// Diff 编辑器（文本比对工具）
import "monaco-editor/editor/browser/widget/diffEditor/diffEditor.contribution";
// 语法高亮（tokenizer 惰性加载）
import "monaco-editor/languages/definitions/css/register";
import "monaco-editor/languages/definitions/html/register";
import "monaco-editor/languages/definitions/javascript/register";
import "monaco-editor/languages/definitions/python/register";

// import "monaco-editor/languages/definitions/less/register";
import "monaco-editor/languages/definitions/markdown/register";
// import "monaco-editor/languages/definitions/scss/register";
import "monaco-editor/languages/definitions/typescript/register";
import "monaco-editor/languages/definitions/xml/register";
import "monaco-editor/languages/definitions/yaml/register";
import "monaco-editor/languages/definitions/shell/register";
import "monaco-editor/languages/definitions/lua/register";
// JSON 完整语言服务（诊断/补全），其余语言仅高亮
import "monaco-editor/languages/features/json/register";
import EditorWorker from "monaco-editor/editor/editor.worker?worker";
import JsonWorker from "monaco-editor/language/json/json.worker?worker";

self.MonacoEnvironment = {
  getWorker(_workerId: string, label: string) {
    if (label === "json") return new JsonWorker();
    return new EditorWorker();
  },
};


// 应用自定义主题：编辑器背景对齐 base.css 的 --bg-app 色板，
// 避免浅色模式下 Monaco 默认纯白背景与 --bg-panel 白色面板融为一体而无法区分。
// 颜色值需与 src/styles/base.css 中的 --bg-app 保持同步。
// Monaco 默认通过 window.open 打开链接，在 Tauri webview 中是 no-op。
// registerLinkOpener 会插入到 opener 链最前面，优先于内置的 window.open 兜底逻辑，
// 拦截 http/https 链接的打开行为，转到内置浏览器窗口。
monaco.editor.registerLinkOpener({
  async open(resource) {
    const url = resource.toString();
    if (/^https?:\/\//i.test(url)) {
      const { openBrowser } = await import("./windows/launcher");
      openBrowser(url);
      return true;
    }
    return false;
  },
});

// JWT 按前两个点分段；编辑和换行后的高亮状态由 Monaco 统一维护。
monaco.languages.register({ id: "jwt" });
monaco.languages.setMonarchTokensProvider("jwt", {
  tokenizer: {
    root: [
      [/[^.]+/, "header"],
      [/\./, "delimiter", "@payload"],
    ],
    payload: [
      [/[^.]+/, "payload"],
      [/\./, "delimiter", "@signature"],
    ],
    signature: [[/.+/, "signature"]],
  },
});

monaco.editor.defineTheme("proxycrab-light", {
  base: "vs",
  inherit: true,
  rules: [
    { token: "header.jwt", foreground: "d6004e" },
    { token: "payload.jwt", foreground: "b829e8" },
    { token: "signature.jwt", foreground: "0090c8" },
    { token: "delimiter.jwt", foreground: "8f959e" },
  ],
  colors: {
    "editor.background": "#f5f6f8", // --bg-app (light)
    "editor.lineHighlightBackground": "#e9edf3", // 活动行高亮与背景区分
  },
});
monaco.editor.defineTheme("proxycrab-dark", {
  base: "vs-dark",
  inherit: true,
  rules: [
    { token: "header.jwt", foreground: "fb4d85" },
    { token: "payload.jwt", foreground: "d63aff" },
    { token: "signature.jwt", foreground: "00b9f1" },
    { token: "delimiter.jwt", foreground: "757b85" },
  ],
  colors: {
    "editor.background": "#1a1b1e", // --bg-app (dark)
  },
});

export default monaco;
