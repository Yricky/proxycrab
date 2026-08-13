<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import { appStore } from "../stores/app";

const props = withDefaults(
  defineProps<{
    modelValue: string;
    language?: string;
    readonly?: boolean;
    /** Show a minimap / line numbers etc. kept minimal by default. */
    options?: Record<string, unknown>;
  }>(),
  { language: "plaintext", readonly: false, options: () => ({}) },
);

const emit = defineEmits<{ "update:modelValue": [value: string] }>();

const container = ref<HTMLElement | null>(null);

// Monaco is heavy; load it lazily so the main bundle stays lean.
let editor: import("monaco-editor").editor.IStandaloneCodeEditor | null = null;
let monacoRef: typeof import("monaco-editor") | null = null;
let suppressChange = false;
let resizeObserver: ResizeObserver | null = null;
let themeObserver: MutationObserver | null = null;

function currentTheme(): string {
  return document.documentElement.dataset.theme === "dark" ? "proxycrab-dark" : "proxycrab-light";
}

function focus(): void {
  editor?.focus();
}

defineExpose({ focus });

onMounted(async () => {
  const monaco = (await import("../monaco")).default;
  monacoRef = monaco;
  editor = monaco.editor.create(container.value!, {
    value: props.modelValue,
    language: props.language,
    readOnly: props.readonly,
    theme: currentTheme(),
    automaticLayout: false,
    minimap: { enabled: false },
    fontSize: 12,
    scrollBeyondLastLine: false,
    wordWrap: "on",
    renderLineHighlight: props.readonly ? "none" : "line",
    folding: true,
    ...props.options,
  });
  editor.onDidChangeModelContent(() => {
    if (suppressChange || !editor) return;
    emit("update:modelValue", editor.getValue());
  });
  resizeObserver = new ResizeObserver(() => editor?.layout());
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
  editor = null;
});

watch(
  () => props.modelValue,
  (value) => {
    if (editor && editor.getValue() !== value) {
      suppressChange = true;
      editor.setValue(value);
      suppressChange = false;
    }
  },
);

watch(
  () => props.language,
  (language) => {
    const model = editor?.getModel();
    if (model && monacoRef) monacoRef.editor.setModelLanguage(model, language);
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
  <div ref="container" class="monaco-host" />
</template>

<style scoped>
.monaco-host {
  flex: 1;
  min-height: 0;
  width: 100%;
}
</style>
