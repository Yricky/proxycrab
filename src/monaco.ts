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

export default monaco;
