<script setup lang="ts">
import { computed } from "vue";
import { parseUrlSegments, type UrlSegment } from "../utils/url-segments";

const model = defineModel<string>({ required: true });

const segments = computed<UrlSegment[]>(
  () => parseUrlSegments(model.value) ?? [{ text: model.value, cls: "" }],
);

function onInput(event: Event): void {
  const el = event.target as HTMLTextAreaElement;
  if (el.value.includes("\n")) model.value = el.value.replace(/\r?\n/g, "");
}
</script>

<template>
  <div class="url-input">
    <div class="url-input-highlight" aria-hidden="true">
      <span v-for="(seg, i) in segments" :key="i" :class="seg.cls">{{
        seg.text
      }}</span>
    </div>
    <textarea
      v-model="model"
      class="url-input-field"
      rows="1"
      spellcheck="false"
      placeholder="https://example.com/path?query=1"
      @input="onInput"
      @keydown.enter.prevent
    />
  </div>
</template>

<style scoped>
.url-input {
  position: relative;
  flex: 1;
  min-width: 0;
  font-size: 12px;
  /* 与详情页 SegmentedUrl 一致：默认收起为 2 行 */
  max-height: calc(2 * 1.4em + 4px);
  overflow: hidden;
}

/* 编辑态完整展示 */
.url-input:focus-within {
  max-height: none;
}

/* 两层字体/换行参数与详情页 SegmentedUrl 完全一致 */
.url-input-highlight,
.url-input-field {
  font-family: var(--font-mono);
  font-size: 12px;
  line-height: 1.4;
  padding: 2px 0;
  white-space: pre-wrap;
  word-break: break-all;
}

.url-input-highlight {
  min-height: calc(1.4em + 4px);
  pointer-events: none;
  color: var(--text);
}

.url-input-field {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  border: none;
  outline: none;
  resize: none;
  overflow: hidden;
  background: transparent;
  color: transparent;
  caret-color: var(--text);
}

.url-input-field::placeholder {
  color: var(--text-faint);
}

/* 分段着色与 SegmentedUrl 保持一致 */
.url-input-highlight .url-scheme {
  color: var(--text-faint);
}

.url-input-highlight .url-host {
  color: var(--accent);
  font-weight: 600;
}

.url-input-highlight .url-path {
  color: var(--text);
}

.url-input-highlight .url-query,
.url-input-highlight .url-query-key {
  color: var(--warning);
}

.url-input-highlight .url-query-value {
  color: var(--accent);
}

.url-input-highlight .url-query-eq,
.url-input-highlight .url-query-sep {
  color: var(--text-faint);
}
</style>
