<script setup lang="ts">
import { computed } from "vue";
import { splitSseBlocks } from "../utils/sse";
import MonacoEditor from "./MonacoEditor.vue";

const props = defineProps<{ text: string; aggregated: boolean }>();
const blocks = computed(() => (props.aggregated ? [] : splitSseBlocks(props.text)));
</script>

<template>
  <div v-if="aggregated" class="sse-editor">
    <MonacoEditor :model-value="text" language="plaintext" readonly />
  </div>
  <div v-else class="sse-blocks">
    <section v-for="(block, index) in blocks" :key="index" class="sse-block">
      <div class="sse-block-head">
        <span>#{{ index + 1 }}</span>
        <span v-if="!block.complete">未结束</span>
      </div>
      <pre>{{ block.raw }}</pre>
    </section>
  </div>
</template>

<style scoped>
.sse-editor {
  flex: 1;
  min-height: 0;
  display: flex;
}
.sse-blocks {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: 6px;
}
.sse-block {
  margin-bottom: 6px;
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  background: var(--bg-panel);
  content-visibility: auto;
  contain-intrinsic-size: auto 100px;
}
.sse-block-head {
  display: flex;
  justify-content: space-between;
  padding: 3px 8px;
  border-bottom: 1px solid var(--border);
  background: var(--bg-app);
  color: var(--text-faint);
  font: 10px var(--font-mono);
}
.sse-block pre {
  margin: 0;
  padding: 6px 8px;
  color: var(--text);
  font: 12px/1.5 var(--font-mono);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
</style>
