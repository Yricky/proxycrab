// Monaco singleton with Vite worker wiring. Import this module instead of
// "monaco-editor" directly so the environment is always configured first.
import * as monaco from "monaco-editor";
// monaco-editor 的 exports map 将 "./*" 映射到 "./esm/vs/*.js"，
// 因此这里的子路径不带 esm/vs 前缀。
import EditorWorker from "monaco-editor/editor/editor.worker?worker";
import JsonWorker from "monaco-editor/language/json/json.worker?worker";

self.MonacoEnvironment = {
  getWorker(_workerId: string, label: string) {
    if (label === "json") return new JsonWorker();
    return new EditorWorker();
  },
};

export default monaco;
