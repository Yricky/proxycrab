<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import type { UrlSegment } from "../utils/url-segments";

const props = defineProps<{
  segments: UrlSegment[];
  title?: string;
}>();

const root = ref<HTMLElement | null>(null);
const expanded = ref(false);
const overflows = ref(false);

// 折叠状态下内容高度超过两行即视为溢出；展开时保持原判定，保证“收起”可见
function measure(): void {
  const el = root.value;
  if (!el || expanded.value) return;
  overflows.value = el.scrollHeight > el.clientHeight + 1;
}

let observer: ResizeObserver | undefined;

onMounted(() => {
  void nextTick(measure);
  if (root.value) {
    observer = new ResizeObserver(measure);
    observer.observe(root.value);
  }
});

watch(
  () => props.segments,
  () => {
    expanded.value = false;
    void nextTick(measure);
  },
);

onBeforeUnmount(() => observer?.disconnect());
</script>

<template>
  <div ref="root" class="seg-url mono" :class="{ expanded }" :title="title">
    <span v-for="(seg, i) in segments" :key="i" :class="seg.cls">{{
      seg.text
    }}</span>
    <button
      v-if="overflows"
      class="seg-toggle"
      :class="{ floating: !expanded }"
      @click="expanded = !expanded"
    >
      {{ expanded ? "收起" : "更多" }}
    </button>
  </div>
</template>

<style scoped>
.seg-url {
  position: relative;
  font-size: 12px;
  line-height: 1.4;
  word-break: break-all;
}

.seg-url:not(.expanded) {
  max-height: calc(2 * 1.4em);
  overflow: hidden;
}

.seg-toggle {
  height: 16px;
  padding: 0 8px;
  border: 1px solid var(--border);
  border-radius: 999px;
  background: var(--bg-app);
  color: var(--accent);
  font-family: var(--font-ui);
  font-size: 10px;
  line-height: 1;
  cursor: pointer;
}

.seg-toggle:hover {
  border-color: var(--accent);
  /* --bg-selected 为半透明色，叠在实心底色上避免透出下层 URL 文字 */
  background: linear-gradient(var(--bg-selected), var(--bg-selected)),
    var(--bg-app);
}

.seg-url.expanded .seg-toggle {
  float: right;
  margin-left: 8px;
}

.seg-toggle.floating {
  position: absolute;
  right: 0;
  bottom: 0;
}

.url-scheme {
  color: var(--text-faint);
}

.url-host {
  color: var(--accent);
  font-weight: 600;
}

.url-path {
  color: var(--text);
}

.url-query {
  color: var(--warning);
}

.url-query-key {
  color: var(--warning);
}

.url-query-eq {
  color: var(--text-faint);
}

.url-query-value {
  color: var(--accent);
}

.url-query-sep {
  color: var(--text-faint);
}
</style>
