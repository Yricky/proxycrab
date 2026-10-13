// monaco-editor 的 internal/common/workers.js 会副作用引入全部 editor contrib
// （codelens、diffEditorBreadcrumbs 等），这些 contrib 依赖未装配的服务
// （ICodeLensCache、IOutlineModelService 等），运行时产生大量噪音报错。
// JSON worker 链路（workerManager.js）只用其中的 createWebWorker，
// 这里提供等价的最小实现，通过 vite alias 替换原模块。
import { editor } from "monaco-editor/editor/editor.api";

interface WebWorkerOptions {
  label?: string;
  createWorker?: () => Worker;
  createData?: unknown;
  host?: unknown;
  keepIdleModels?: boolean;
}

export function createWebWorker(opts: WebWorkerOptions): unknown {
  const getWorker = globalThis.MonacoEnvironment?.getWorker;
  const raw = getWorker
    ? getWorker("workerMain.js", opts.label ?? "monaco-editor-worker")
    : opts.createWorker?.();
  if (!raw) throw new Error("MonacoEnvironment.getWorker 未定义");
  const worker = Promise.resolve(raw).then((w) => {
    w.postMessage("ignore");
    w.postMessage(opts.createData);
    return w;
  });
  return editor.createWebWorker({
    worker,
    host: opts.host as never,
    keepIdleModels: opts.keepIdleModels,
  } as Parameters<typeof editor.createWebWorker>[0]);
}
