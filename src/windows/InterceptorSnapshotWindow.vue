<script setup lang="ts">
import { computed } from "vue";
import type {
  HeaderItem,
  InterceptorExecution,
  Modification,
  RequestInterceptorSnapshot,
  ResponseInterceptorSnapshot,
} from "../api/types";
import BodyViewer from "../components/BodyViewer.vue";
import MonacoEditor from "../components/MonacoEditor.vue";

const props = defineProps<{
  sessionId: number;
  logId: number;
  execution: InterceptorExecution;
}>();

type SnapshotModification = Extract<Modification, { kind: "snapshot" }>;

const snapshot = computed<SnapshotModification | null>(
  () =>
    props.execution.modifications.find(
      (mod): mod is SnapshotModification => mod.kind === "snapshot",
    ) ?? null,
);
const request = computed<RequestInterceptorSnapshot | null>(
  () => snapshot.value?.request ?? null,
);
const response = computed<ResponseInterceptorSnapshot | null>(
  () => snapshot.value?.response ?? null,
);
const side = computed<"request" | "response">(() =>
  request.value ? "request" : "response",
);
const state = computed(() => request.value ?? response.value);
const headers = computed<HeaderItem[]>(() =>
  Object.entries(state.value?.headers ?? {}).flatMap(([name, values]) =>
    values.map((value) => ({ name, value })),
  ),
);
const body = computed(() => state.value?.body ?? null);
const contentType = computed(
  () =>
    headers.value.find((item) => item.name.toLowerCase() === "content-type")
      ?.value ?? "",
);
const language = computed(() => {
  const media = contentType.value.toLowerCase();
  if (media.includes("json")) return "json";
  if (media.includes("html")) return "html";
  if (media.includes("xml")) return "xml";
  if (media.includes("javascript")) return "javascript";
  if (media.includes("css")) return "css";
  return "plaintext";
});
</script>

<template>
  <div class="snapshot-window">
    <div v-if="!state || !body" class="empty">该执行记录没有完整快照</div>
    <template v-else>
      <section class="summary card">
        <template v-if="request">
          <span class="method">{{ request.method }}</span>
          <span class="mono primary">{{ request.uri }}</span>
          <span class="mono muted">{{ request.version }}</span>
        </template>
        <template v-else-if="response">
          <span class="status mono">{{ response.status }}</span>
          <span class="mono muted">{{ response.version }}</span>
        </template>
      </section>

      <section class="headers card">
        <h3>Headers</h3>
        <table v-if="headers.length">
          <tbody>
            <tr
              v-for="(header, index) in headers"
              :key="`${header.name}-${index}`"
            >
              <th class="mono">{{ header.name }}</th>
              <td class="mono">{{ header.value }}</td>
            </tr>
          </tbody>
        </table>
        <div v-else class="empty small">无 Headers</div>
      </section>

      <section class="body card">
        <h3>Body</h3>
        <div v-if="body.type === 'asset'" class="asset-id">
          <span>Asset ID</span><code>{{ body.asset_id }}</code>
        </div>
        <div v-else-if="body.type === 'string'" class="editor">
          <MonacoEditor
            :model-value="body.content"
            :language="language"
            :readonly="true"
          />
        </div>
        <div v-else-if="!execution.completed" class="empty">
          Body 尚未捕获完成
        </div>
        <BodyViewer
          v-else
          label="快照 Body"
          :body="{ type: 'binary', size: 1, path: null }"
          :headers="headers"
          :side="side"
          :target="{
            kind: 'interceptor',
            id: logId,
            sessionId,
            executionId: execution.execution_id,
          }"
        />
      </section>
    </template>
  </div>
</template>

<style scoped>
.snapshot-window {
  display: grid;
  grid-template-rows: auto minmax(120px, 0.7fr) minmax(180px, 1.3fr);
  gap: 10px;
  height: 100%;
  padding: 10px;
  overflow: hidden;
}

.card {
  min-width: 0;
  overflow: hidden;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--bg-panel);
}

.summary {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 9px 11px;
}

.method,
.status {
  flex: none;
  color: var(--accent);
  font-weight: 700;
}

.primary {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.muted {
  margin-left: auto;
  color: var(--text-secondary);
}

.headers,
.body {
  display: flex;
  min-height: 0;
  flex-direction: column;
}

h3 {
  flex: none;
  margin: 0;
  padding: 7px 10px;
  border-bottom: 1px solid var(--border);
  color: var(--text-secondary);
  font-size: 11px;
}

table {
  width: 100%;
  border-collapse: collapse;
  overflow: auto;
  font-size: 11px;
}

.headers > table {
  display: block;
  overflow: auto;
}

th,
td {
  padding: 5px 9px;
  border-bottom: 1px solid var(--border-subtle);
  text-align: left;
  vertical-align: top;
  word-break: break-all;
}

th {
  width: 180px;
  color: var(--text-secondary);
  font-weight: 500;
}

.editor {
  min-height: 0;
  flex: 1;
}

.asset-id {
  display: flex;
  align-items: baseline;
  gap: 10px;
  padding: 14px;
  color: var(--text-secondary);
}

.asset-id code {
  color: var(--text);
  word-break: break-all;
}

.empty {
  display: grid;
  place-items: center;
  min-height: 80px;
  color: var(--text-faint);
  font-size: 12px;
}

.empty.small {
  min-height: 50px;
}
</style>
