<script setup lang="ts">
import type { InterceptorExecution } from "../api/types";
import MonacoEditor from "../components/MonacoEditor.vue";

const props = defineProps<{ execution: InterceptorExecution }>();

</script>

<template>
  <div class="snapshot-root">
    <div class="snapshot-toolbar">
      <span class="snapshot-name mono">{{ execution.name }}</span>
      <span class="badge">
        {{ execution.phase === "request" ? "请求拦截器" : "响应拦截器" }}
      </span>
      <span class="badge" :class="{ temporary: execution.origin === 'temporary' }">
        {{ execution.origin === "temporary" ? "临时执行" : "已保存脚本" }}
      </span>
      <span v-if="!execution.completed" class="badge waiting">
        暂停中
      </span>
      <span class="snapshot-spacer" />
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
.badge.temporary {
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 12%, transparent);
}
.badge.waiting {
  color: var(--warning);
  background: color-mix(in srgb, var(--warning) 12%, transparent);
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
