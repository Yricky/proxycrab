<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref } from "vue";
import { Io5Checkmark, Io5EllipsisHorizontal } from "vue-icons-plus/io5";
import CustomSelect from "../components/CustomSelect.vue";
import MonacoDiffEditor from "../components/MonacoDiffEditor.vue";
import MonacoEditor from "../components/MonacoEditor.vue";

type TabKey = "text1" | "text2" | "side" | "inline";

const tabs: { key: TabKey; label: string }[] = [
  { key: "text1", label: "文本1" },
  { key: "text2", label: "文本2" },
  { key: "side", label: "左右视图" },
  { key: "inline", label: "内联比对" },
];

const activeTab = ref<TabKey>("text1");
// diff 编辑器按需创建，首次切到对应页签时才挂载。
const visited = ref<Set<TabKey>>(new Set(["text1"]));
const sideDiff = ref<InstanceType<typeof MonacoDiffEditor> | null>(null);
const inlineDiff = ref<InstanceType<typeof MonacoDiffEditor> | null>(null);

const text1 = ref("");
const text2 = ref("");
const language = ref("plaintext");

const languageOptions = [
  { value: "plaintext", label: "纯文本" },
  { value: "json", label: "JSON" },
  { value: "javascript", label: "JavaScript" },
  { value: "typescript", label: "TypeScript" },
  { value: "css", label: "CSS" },
  { value: "html", label: "HTML" },
  { value: "xml", label: "XML" },
  { value: "yaml", label: "YAML" },
  { value: "markdown", label: "Markdown" },
  { value: "lua", label: "Lua" },
  { value: "less", label: "Less" },
  { value: "scss", label: "SCSS" },
];

const formatJson = ref(false);
const ignoreWhitespace = ref(false);
const menuOpen = ref(false);
const menuRoot = ref<HTMLElement | null>(null);

const isJson = computed(() => language.value === "json");

function selectTab(key: TabKey): void {
  activeTab.value = key;
  if (!visited.value.has(key)) {
    visited.value = new Set(visited.value).add(key);
  }
  // v-show 重新显示后容器尺寸恢复，主动触发一次布局
  if (key === "side" || key === "inline") {
    const target = key === "side" ? sideDiff : inlineDiff;
    void nextTick(() => target.value?.layout());
  }
}

function prettyJson(text: string): string {
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return text;
  }
}

// 勾选「JSON 格式化」时，比对前对两侧做 pretty-print，不回写编辑区原文。
const diffOriginal = computed(() =>
  isJson.value && formatJson.value ? prettyJson(text1.value) : text1.value,
);
const diffModified = computed(() =>
  isJson.value && formatJson.value ? prettyJson(text2.value) : text2.value,
);

function onDocumentClick(event: MouseEvent): void {
  if (!menuRoot.value?.contains(event.target as Node)) closeMenu();
}

function closeMenu(): void {
  menuOpen.value = false;
  document.removeEventListener("click", onDocumentClick, true);
}

function toggleMenu(): void {
  if (menuOpen.value) {
    closeMenu();
    return;
  }
  menuOpen.value = true;
  document.addEventListener("click", onDocumentClick, true);
}

function toggleFormatJson(): void {
  if (!isJson.value) return;
  formatJson.value = !formatJson.value;
}

function toggleIgnoreWhitespace(): void {
  ignoreWhitespace.value = !ignoreWhitespace.value;
}

onBeforeUnmount(closeMenu);
</script>

<template>
  <div class="diff-root">
    <nav class="tab-bar">
      <button
        v-for="tab in tabs"
        :key="tab.key"
        class="tab-btn"
        :class="{ active: activeTab === tab.key }"
        @click="selectTab(tab.key)"
      >
        {{ tab.label }}
      </button>
      <span class="tab-spacer" />
      <CustomSelect
        v-model="language"
        class="lang-select"
        :options="languageOptions"
        aria-label="语言"
      />
      <div ref="menuRoot" class="menu-wrap">
        <button
          class="menu-trigger"
          :class="{ active: menuOpen }"
          aria-label="比对选项"
          @click="toggleMenu"
        >
          <Io5EllipsisHorizontal :size="14" />
        </button>
        <div v-if="menuOpen" class="menu">
          <button
            class="menu-item"
            :disabled="!isJson"
            title="仅 JSON 语言时可用"
            @click="toggleFormatJson"
          >
            <Io5Checkmark :size="14" :class="{ invisible: !formatJson }" />
            <span>JSON 格式化</span>
          </button>
          <button class="menu-item" @click="toggleIgnoreWhitespace">
            <Io5Checkmark :size="14" :class="{ invisible: !ignoreWhitespace }" />
            <span>忽略空白</span>
          </button>
        </div>
      </div>
    </nav>

    <div v-show="activeTab === 'text1'" class="tab-page">
      <MonacoEditor v-model="text1" :language="language" />
    </div>
    <div v-show="activeTab === 'text2'" class="tab-page">
      <MonacoEditor v-if="visited.has('text2')" v-model="text2" :language="language" />
    </div>
    <div v-show="activeTab === 'side'" class="tab-page">
      <MonacoDiffEditor
        v-if="visited.has('side')"
        ref="sideDiff"
        :original="diffOriginal"
        :modified="diffModified"
        :language="language"
        :options="{ renderSideBySide: true, ignoreTrimWhitespace: ignoreWhitespace }"
      />
    </div>
    <div v-show="activeTab === 'inline'" class="tab-page">
      <MonacoDiffEditor
        v-if="visited.has('inline')"
        ref="inlineDiff"
        :original="diffOriginal"
        :modified="diffModified"
        :language="language"
        :options="{ renderSideBySide: false, ignoreTrimWhitespace: ignoreWhitespace }"
      />
    </div>
  </div>
</template>

<style scoped>
.diff-root {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}

.tab-bar {
  flex: none;
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 0 8px;
  border-bottom: 1px solid var(--border);
}

.tab-btn {
  position: relative;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 6px 14px 8px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  font: inherit;
  font-size: 12px;
  cursor: pointer;
  border-radius: 6px 6px 0 0;
}
.tab-btn:hover {
  color: var(--text);
}
.tab-btn.active {
  color: var(--accent);
  font-weight: 600;
}
.tab-btn.active::after {
  content: "";
  position: absolute;
  left: 8px;
  right: 8px;
  bottom: -1px;
  height: 2px;
  border-radius: 1px;
  background: var(--accent);
}

.tab-spacer {
  flex: 1;
}

.lang-select {
  flex: none;
  width: 120px;
}

.menu-wrap {
  position: relative;
  flex: none;
}

.menu-trigger {
  height: 24px;
  width: 26px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
}
.menu-trigger:hover,
.menu-trigger.active {
  background: var(--bg-hover);
  color: var(--text);
}

.menu {
  position: absolute;
  z-index: 100003;
  top: calc(100% + 2px);
  right: 0;
  min-width: 148px;
  padding: 5px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-popup);
}

.menu-item {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 9px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text);
  font: inherit;
  font-size: 12px;
  text-align: left;
  cursor: pointer;
}
.menu-item:hover:not(:disabled) {
  background: var(--bg-hover);
}
.menu-item:disabled {
  color: var(--text-faint);
  cursor: default;
}
.menu-item .invisible {
  visibility: hidden;
}

.tab-page {
  flex: 1;
  min-height: 0;
  display: flex;
}
</style>
