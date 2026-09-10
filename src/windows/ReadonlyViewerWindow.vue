<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import MonacoEditor from "../components/MonacoEditor.vue";
import CustomSelect, { type CustomSelectOption } from "../components/CustomSelect.vue";

const props = withDefaults(
  defineProps<{
    content: string;
    /** 期望的高亮格式（monaco language id），用户可在状态栏自行更改。 */
    language?: string;
  }>(),
  { language: "plaintext" },
);

const language = ref(props.language);
const wordWrap = ref(true);
const pretty = ref(true);

const languageOptions = ref<CustomSelectOption[]>([
  { value: props.language, label: props.language },
]);

onMounted(async () => {
  // Monaco 懒加载；语言列表同样异步拉取。
  const monaco = (await import("../monaco")).default;
  const options = monaco.languages
    .getLanguages()
    .map((lang) => ({ value: lang.id, label: lang.aliases?.[0] ?? lang.id }))
    .sort((a, b) => a.label.localeCompare(b.label));
  if (!options.some((option) => option.value === language.value)) {
    options.unshift({ value: language.value, label: language.value });
  }
  languageOptions.value = options;
});

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

// 格式化器与高亮格式匹配：目前仅 json 有格式化器，失败回退原文。
const prettyContent = computed(() => {
  const raw = props.content ?? "";
  if (language.value !== "json") return raw;
  try {
    return JSON.stringify(JSON.parse(raw), null, 2);
  } catch {
    return raw;
  }
});

// 与 pretty 开关解耦，否则切到「原文」后按钮会消失。
const formattable = computed(() => prettyContent.value !== (props.content ?? ""));

const displayContent = computed(() =>
  pretty.value ? prettyContent.value : (props.content ?? ""),
);

const sizeLabel = computed(() =>
  formatBytes(new TextEncoder().encode(displayContent.value).length),
);
const lineCount = computed(() =>
  displayContent.value.length === 0 ? 0 : displayContent.value.split("\n").length,
);
</script>

<template>
  <div class="viewer-root">
    <MonacoEditor
      :model-value="displayContent"
      :language="language"
      readonly
      :options="{ wordWrap: wordWrap ? 'on' : 'off' }"
    />
    <footer class="viewer-statusbar">
      <span class="status-item mono">{{ sizeLabel }}</span>
      <span class="status-item">{{ lineCount }} 行</span>
      <span class="status-spacer" />
      <template v-if="formattable">
        <button class="tool-btn" :class="{ on: pretty }" @click="pretty = true">格式化</button>
        <button class="tool-btn" :class="{ on: !pretty }" @click="pretty = false">原文</button>
      </template>
      <button
        class="status-toggle"
        :class="{ active: wordWrap }"
        title="切换自动换行"
        @click="wordWrap = !wordWrap"
      >
        自动换行
      </button>
      <CustomSelect
        v-model="language"
        class="status-language"
        :options="languageOptions"
        placement="up"
        aria-label="高亮格式"
      />
    </footer>
  </div>
</template>

<style scoped>
.viewer-root {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.viewer-statusbar {
  flex: none;
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: 4px 12px;
  border-top: 1px solid var(--border);
  background: var(--bg-panel);
  color: var(--text-faint);
  font-size: 11px;
}

.status-item {
  white-space: nowrap;
}

.status-spacer {
  flex: 1;
}

.status-toggle {
  padding: 1px 7px;
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-faint);
  font-size: 11px;
  cursor: pointer;
}

.status-toggle:hover {
  background: var(--bg-hover);
  color: var(--text);
}

.status-toggle.active {
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 12%, transparent);
}

.status-language {
  width: 140px;
  flex: none;
}

.tool-btn {
  border: 1px solid var(--border);
  background: var(--bg-panel);
  color: var(--text-secondary);
  font: inherit;
  font-size: 10px;
  padding: 1px 8px;
  cursor: pointer;
}

.tool-btn.on {
  background: var(--accent);
  border-color: var(--accent);
  color: var(--accent-text);
}

.status-language :deep(.select-trigger) {
  min-height: 24px;
  padding: 2px 6px 2px 8px;
  font-size: 11px;
}
</style>
