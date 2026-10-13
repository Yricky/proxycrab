<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import { appStore } from "../stores/app";

const props = withDefaults(
  defineProps<{
    original: string;
    modified: string;
    language?: string;
    /** renderSideBySide / ignoreTrimWhitespace 等 diff 编辑器选项。 */
    options?: Record<string, unknown>;
  }>(),
  { language: "plaintext", options: () => ({}) },
);

const container = ref<HTMLElement | null>(null);

// diff 编辑器的 layout() 不带参数时测量的是自身根节点（隐藏期被钳到 5px 后无法恢复），
// 必须显式传入外层容器尺寸。
function layout(): void {
  const el = container.value;
  if (!editor || !el) return;
  const width = el.clientWidth;
  const height = el.clientHeight;
  // 隐藏（display:none）时跳过，避免写入退化的 0 尺寸
  if (width > 0 && height > 0) editor.layout({ width, height });
}

defineExpose({ layout });

// Monaco 懒加载，与 MonacoEditor.vue 保持一致。
let editor: import("monaco-editor/editor/editor.api").editor.IStandaloneDiffEditor | null = null;
let monacoRef: typeof import("monaco-editor/editor/editor.api") | null = null;
let originalModel: import("monaco-editor/editor/editor.api").editor.ITextModel | null = null;
let modifiedModel: import("monaco-editor/editor/editor.api").editor.ITextModel | null = null;
let resizeObserver: ResizeObserver | null = null;
let themeObserver: MutationObserver | null = null;

function currentTheme(): string {
  return document.documentElement.dataset.theme === "dark" ? "proxycrab-dark" : "proxycrab-light";
}

onMounted(async () => {
  const monaco = (await import("../monaco")).default;
  monacoRef = monaco;
  originalModel = monaco.editor.createModel(props.original, props.language);
  modifiedModel = monaco.editor.createModel(props.modified, props.language);
  editor = monaco.editor.createDiffEditor(container.value!, {
    // 比对视图只读，编辑在独立的文本页签中进行。
    readOnly: true,
    originalEditable: false,
    theme: currentTheme(),
    automaticLayout: false,
    minimap: { enabled: false },
    fontSize: 12,
    scrollBeyondLastLine: false,
    wordWrap: "on",
    folding: true,
    renderSideBySide: true,
    ...props.options,
  });
  editor.setModel({ original: originalModel, modified: modifiedModel });
  resizeObserver = new ResizeObserver(() => layout());
  resizeObserver.observe(container.value!);
  themeObserver = new MutationObserver(() => {
    monaco.editor.setTheme(currentTheme());
  });
  themeObserver.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["data-theme"],
  });
});

onBeforeUnmount(() => {
  resizeObserver?.disconnect();
  themeObserver?.disconnect();
  editor?.dispose();
  originalModel?.dispose();
  modifiedModel?.dispose();
  editor = null;
  originalModel = null;
  modifiedModel = null;
});

watch(
  () => props.original,
  (value) => {
    if (originalModel && originalModel.getValue() !== value) originalModel.setValue(value);
  },
);

watch(
  () => props.modified,
  (value) => {
    if (modifiedModel && modifiedModel.getValue() !== value) modifiedModel.setValue(value);
  },
);

watch(
  () => props.language,
  (language) => {
    if (!monacoRef) return;
    if (originalModel) monacoRef.editor.setModelLanguage(originalModel, language);
    if (modifiedModel) monacoRef.editor.setModelLanguage(modifiedModel, language);
  },
);

watch(
  () => props.options,
  (options) => {
    editor?.updateOptions({ ...options });
  },
);

watch(
  () => appStore.theme,
  () => {
    monacoRef?.editor.setTheme(currentTheme());
  },
);
</script>

<template>
  <div ref="container" class="monaco-diff-host" />
</template>

<style scoped>
.monaco-diff-host {
  flex: 1;
  min-height: 0;
  width: 100%;
}
</style>
