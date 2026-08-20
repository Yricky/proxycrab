// Monaco singleton with Vite worker wiring. Import this module instead of
// "monaco-editor" directly so the environment is always configured first.
import * as monaco from "monaco-editor";
// monaco-editor 的 exports map 将 "./*" 映射到 "./esm/vs/*.js"，
// 因此这里的子路径不带 esm/vs 前缀。
import EditorWorker from "monaco-editor/editor/editor.worker?worker";
import CssWorker from "monaco-editor/language/css/css.worker?worker";
import HtmlWorker from "monaco-editor/language/html/html.worker?worker";
import JsonWorker from "monaco-editor/language/json/json.worker?worker";
import TypeScriptWorker from "monaco-editor/language/typescript/ts.worker?worker";

self.MonacoEnvironment = {
  getWorker(_workerId: string, label: string) {
    if (label === "json") return new JsonWorker();
    if (label === "css" || label === "scss" || label === "less") return new CssWorker();
    if (label === "html" || label === "handlebars" || label === "razor") {
      return new HtmlWorker();
    }
    if (label === "typescript" || label === "javascript") return new TypeScriptWorker();
    return new EditorWorker();
  },
};

if (!monaco.languages.getLanguages().some((language) => language.id === "lua")) {
  monaco.languages.register({
    id: "lua",
    extensions: [".lua"],
    aliases: ["Lua", "lua"],
  });
  monaco.languages.setLanguageConfiguration("lua", {
    comments: {
      lineComment: "--",
      blockComment: ["--[[", "]]"],
    },
    brackets: [
      ["{", "}"],
      ["[", "]"],
      ["(", ")"],
    ],
    autoClosingPairs: [
      { open: "{", close: "}" },
      { open: "[", close: "]" },
      { open: "(", close: ")" },
      { open: '"', close: '"' },
      { open: "'", close: "'" },
    ],
    surroundingPairs: [
      { open: "{", close: "}" },
      { open: "[", close: "]" },
      { open: "(", close: ")" },
      { open: '"', close: '"' },
      { open: "'", close: "'" },
    ],
  });
  monaco.languages.setMonarchTokensProvider("lua", {
    defaultToken: "",
    tokenPostfix: ".lua",
    keywords: [
      "and",
      "break",
      "do",
      "else",
      "elseif",
      "end",
      "false",
      "for",
      "function",
      "goto",
      "if",
      "in",
      "local",
      "nil",
      "not",
      "or",
      "repeat",
      "return",
      "then",
      "true",
      "until",
      "while",
    ],
    builtins: [
      "_G",
      "_VERSION",
      "assert",
      "collectgarbage",
      "error",
      "getmetatable",
      "ipairs",
      "next",
      "pairs",
      "pcall",
      "print",
      "rawequal",
      "rawget",
      "rawlen",
      "rawset",
      "select",
      "setmetatable",
      "tonumber",
      "tostring",
      "type",
      "warn",
      "xpcall",
      "coroutine",
      "math",
      "string",
      "table",
      "utf8",
    ],
    operators: [
      "+",
      "-",
      "*",
      "/",
      "%",
      "^",
      "#",
      "&",
      "~",
      "|",
      "<<",
      ">>",
      "//",
      "==",
      "~=",
      "<=",
      ">=",
      "<",
      ">",
      "=",
      "(",
      ")",
      "{",
      "}",
      "[",
      "]",
      "::",
      ";",
      ":",
      ",",
      ".",
      "..",
      "...",
    ],
    symbols: /[=><!~?:&|+\-*/^%#]+/,
    tokenizer: {
      root: [
        [/[a-zA-Z_]\w*/, { cases: { "@keywords": "keyword", "@builtins": "type.identifier" } }],
        { include: "@whitespace" },
        [/\d*\.\d+([eE][-+]?\d+)?/, "number.float"],
        [/0[xX][0-9a-fA-F]+/, "number.hex"],
        [/\d+/, "number"],
        [/"/, "string", "@doubleQuotedString"],
        [/'/, "string", "@singleQuotedString"],
        [/\[(=*)\[/, "string", "@longString.$1"],
        [/[{}()[\]]/, "@brackets"],
        [/@symbols/, { cases: { "@operators": "operator", "@default": "" } }],
        [/[;,.]/, "delimiter"],
      ],
      whitespace: [
        [/[ \t\r\n]+/, ""],
        [/--\[(=*)\[/, "comment", "@longComment.$1"],
        [/--.*$/, "comment"],
      ],
      doubleQuotedString: [
        [/[^\\"]+/, "string"],
        [/\\./, "string.escape"],
        [/"/, "string", "@pop"],
      ],
      singleQuotedString: [
        [/[^\\']+/, "string"],
        [/\\./, "string.escape"],
        [/'/, "string", "@pop"],
      ],
      longString: [
        [/\](=*)\]/, { cases: { "$1==$S2": { token: "string", next: "@pop" }, "@default": "string" } }],
        [/./, "string"],
      ],
      longComment: [
        [/\](=*)\]/, { cases: { "$1==$S2": { token: "comment", next: "@pop" }, "@default": "comment" } }],
        [/./, "comment"],
      ],
    },
  });
}

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

monaco.editor.defineTheme("proxycrab-light", {
  base: "vs",
  inherit: true,
  rules: [],
  colors: {
    "editor.background": "#f5f6f8", // --bg-app (light)
    "editor.lineHighlightBackground": "#e9edf3", // 活动行高亮与背景区分
  },
});
monaco.editor.defineTheme("proxycrab-dark", {
  base: "vs-dark",
  inherit: true,
  rules: [],
  colors: {
    "editor.background": "#1a1b1e", // --bg-app (dark)
  },
});

export default monaco;
