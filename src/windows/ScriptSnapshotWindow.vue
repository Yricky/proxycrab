<script setup lang="ts">
import { ref } from "vue";
import { Io5Checkmark, Io5Copy } from "vue-icons-plus/io5";
import type { InterceptorExecution } from "../api/types";
import MonacoEditor from "../components/MonacoEditor.vue";
import { appStore, reportError } from "../stores/app";

const props = defineProps<{ execution: InterceptorExecution }>();
const copied = ref(false);
let copiedTimer: number | undefined;

async function copyHash(): Promise<void> {
  try {
    await navigator.clipboard.writeText(props.execution.script_hash);
    copied.value = true;
    if (copiedTimer !== undefined) window.clearTimeout(copiedTimer);
    copiedTimer = window.setTimeout(() => (copied.value = false), 1200);
    appStore.toast("Hash 已复制", "success");
  } catch (error) {
    reportError(error, "复制 Hash 失败");
  }
}
</script>

<template>
  <div class="snapshot-root">
    <div class="snapshot-toolbar">
      <span class="snapshot-name mono">{{ execution.name }}</span>
      <span class="badge">
        {{ execution.phase === "request" ? "历史请求拦截器" : "历史响应拦截器" }}
      </span>
      <span class="snapshot-spacer" />
      <button class="hash-button mono" title="复制 SHA-256" @click="copyHash">
        <span>{{ execution.script_hash }}</span>
        <Io5Checkmark v-if="copied" :size="13" class="text-success" />
        <Io5Copy v-else :size="13" />
      </button>
    </div>
    <MonacoEditor :model-value="execution.content" language="lua" readonly />
  </div>
</template>

<style scoped>
.snapshot-root {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.snapshot-toolbar {
  flex: none;
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: 8px 12px;
  border-bottom: 1px solid var(--border);
}
.snapshot-name {
  max-width: 220px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 600;
}
.snapshot-spacer {
  flex: 1;
}
.hash-button {
  min-width: 0;
  max-width: 360px;
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 7px;
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-faint);
  cursor: pointer;
  font-size: 10px;
}
.hash-button:hover {
  color: var(--text);
  background: var(--bg-hover);
}
.hash-button span {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
