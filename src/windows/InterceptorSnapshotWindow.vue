<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useBackend } from "../api";
import type { BodyTarget } from "../api/body";
import type {
  BodyPayload,
  HeaderItem,
  InterceptorExecution,
  InterceptorSnapshotPayload,
} from "../api/types";
import BodyViewer from "../components/BodyViewer.vue";
import SegmentedUrl from "../components/SegmentedUrl.vue";
import { urlSegments as buildUrlSegments } from "../utils/url-segments";
import { prefillFromSnapshot } from "../utils/replay";
import { openReplay } from "./launcher";
import { Io5SendOutline } from "vue-icons-plus/io5";

function replaySnapshot(): void {
  if (!snapshot.value) return;
  const draft = prefillFromSnapshot(
    props.sessionId,
    props.logId,
    snapshot.value,
  );
  if (draft) openReplay(draft);
}

const props = defineProps<{
  sessionId: number;
  logId: number;
  execution: InterceptorExecution;
}>();

const backend = useBackend();
const snapshot = ref<InterceptorSnapshotPayload | null>(null);
const loading = ref(true);
const loadError = ref<string | null>(null);

async function load(): Promise<void> {
  loading.value = true;
  loadError.value = null;
  try {
    snapshot.value = await backend.getInterceptorSnapshot(
      props.sessionId,
      props.logId,
      props.execution.execution_id,
    );
  } catch (error) {
    loadError.value = error instanceof Error ? error.message : String(error);
  } finally {
    loading.value = false;
  }
}

onMounted(load);

const request = computed(() => snapshot.value?.request ?? null);
const response = computed(() => snapshot.value?.response ?? null);
const state = computed(() => request.value ?? response.value);
const side = computed<"request" | "response">(() =>
  request.value ? "request" : "response",
);
const headers = computed<HeaderItem[]>(() =>
  Object.entries(state.value?.headers ?? {}).flatMap(([name, values]) =>
    values.map((value) => ({ name, value })),
  ),
);
const bodySource = computed(() => state.value?.body ?? null);
const assetId = computed(() =>
  bodySource.value?.type === "asset" ? bodySource.value.asset_id : null,
);
// 快照 Body 一律通过 Body 接口按 execution_id 读取（原始文件 / 字符串 / 资产
// 均由后端解析），体积在加载后按实际结果显示。
const snapshotBody: BodyPayload = { type: "binary", size: 0, path: null };
const bodyTarget = computed<BodyTarget>(() => ({
  kind: "interceptor",
  id: props.logId,
  sessionId: props.sessionId,
  executionId: props.execution.execution_id,
}));
const bodyPending = computed(
  () => bodySource.value?.type === "original" && !props.execution.completed,
);

const methodClass = computed(() => {
  switch (request.value?.method.toUpperCase()) {
    case "GET":
      return "m-get";
    case "POST":
      return "m-post";
    case "PUT":
      return "m-put";
    case "PATCH":
      return "m-patch";
    case "DELETE":
      return "m-delete";
    default:
      return "m-other";
  }
});

function statusClass(status: number): string {
  if (status >= 200 && status < 300) return "s-2xx";
  if (status >= 300 && status < 400) return "s-3xx";
  if (status >= 400 && status < 500) return "s-4xx";
  if (status >= 500) return "s-5xx";
  return "s-other";
}

const urlSegments = computed(() =>
  request.value ? buildUrlSegments(request.value.uri) : [],
);
</script>

<template>
  <div class="snapshot-window">
    <div v-if="loading" class="state-hint">加载中…</div>
    <div v-else-if="loadError" class="state-hint state-error">
      <p>加载失败：{{ loadError }}</p>
    </div>
    <div v-else-if="!state" class="state-hint">该执行记录没有完整快照</div>
    <template v-else>
      <header class="summary">
        <div class="summary-line">
          <span v-if="request" class="method-chip" :class="methodClass">
            {{ request.method }}
          </span>
          <span
            v-else-if="response"
            class="status-chip mono"
            :class="statusClass(response.status)"
          >
            {{ response.status }}
          </span>
          <span class="meta-item mono">{{ state.version }}</span>
          <span v-if="assetId" class="meta-item asset-meta" :title="assetId">
            <span class="meta-label">Body 资产</span>{{ assetId }}
          </span>
          <span class="summary-spacer" />
          <button
            v-if="request"
            class="btn icon"
            title="重放此快照请求"
            @click="replaySnapshot"
          >
            <Io5SendOutline :size="14" />
          </button>
        </div>
        <SegmentedUrl
          v-if="request"
          class="url"
          :segments="urlSegments"
          :title="request.uri"
        />
      </header>

      <div class="content">
        <section class="card headers-card">
          <div class="card-title">
            Headers
            <span v-if="headers.length" class="count-badge">{{
              headers.length
            }}</span>
          </div>
          <div class="headers-scroll">
            <table v-if="headers.length" class="kv-table">
              <tbody>
                <tr v-for="(header, index) in headers" :key="index">
                  <td class="mono kv-name">{{ header.name }}</td>
                  <td class="mono kv-value">{{ header.value }}</td>
                </tr>
              </tbody>
            </table>
            <div v-else class="empty-hint">无 Headers</div>
          </div>
        </section>

        <div v-if="bodyPending" class="card body-card state-hint">
          Body 尚未捕获完成
        </div>
        <BodyViewer
          v-else
          class="body-panel"
          label="快照 Body"
          :body="snapshotBody"
          :headers="headers"
          :side="side"
          :target="bodyTarget"
          unknown-size
        />
      </div>
    </template>
  </div>
</template>

<style scoped>
.snapshot-window {
  height: 100%;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  user-select: text;
}

.state-hint {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--space-2);
  color: var(--text-faint);
  font-size: 12px;
}

.state-error p {
  margin: 0;
  color: var(--danger);
  word-break: break-all;
  padding: 0 16px;
}

/* ---------- 摘要栏（与请求详情一致） ---------- */

.summary {
  flex: none;
  padding: 6px 12px;
  background: var(--bg-panel);
  border-bottom: 1px solid var(--border);
}

.summary-line {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.summary-spacer {
  flex: 1;
}

.method-chip {
  flex: none;
  padding: 0 6px;
  border-radius: 8px;
  font-size: 10px;
  font-weight: 700;
  height: 16px;
  color: #fff;
}

.m-get {
  background: var(--success);
}

.m-post {
  background: var(--accent);
}

.m-put {
  background: var(--warning);
}

.m-patch {
  background: #8b5cf6;
}

.m-delete {
  background: var(--danger);
}

.m-other {
  background: var(--text-faint);
}

.status-chip {
  flex: none;
  font-size: 10px;
  font-weight: 700;
}

.s-2xx {
  color: var(--success);
}

.s-3xx {
  color: var(--accent);
}

.s-4xx {
  color: var(--warning);
}

.s-5xx {
  color: var(--danger);
}

.s-other {
  color: var(--text-secondary);
}

.meta-item {
  font-size: 11px;
  color: var(--text);
  font-family: var(--font-mono);
}

.meta-label {
  color: var(--text-faint);
  font-family: var(--font-ui);
  margin-right: 6px;
}

.asset-meta {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.url {
  margin-top: 4px;
}

/* ---------- 内容区：Headers + Body ---------- */

.content {
  flex: 1;
  min-height: 0;
  display: flex;
  gap: 10px;
  padding: 10px 12px 12px;
  overflow: hidden;
}

.card {
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--bg-panel);
  overflow: hidden;
}

.headers-card {
  flex: none;
  width: min(320px, 40%);
  display: flex;
  flex-direction: column;
}

.card-title {
  flex: none;
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 10px;
  font-size: 11px;
  font-weight: 600;
  color: var(--text-secondary);
  border-bottom: 1px solid var(--border);
  background: var(--bg-app);
}

.count-badge {
  font-size: 10px;
  font-weight: 500;
  padding: 0 5px;
  border-radius: 8px;
  background: var(--bg-active);
}

.headers-scroll {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
}

.kv-table {
  width: 100%;
  border-collapse: collapse;
}

.kv-table tr + tr {
  border-top: 1px solid var(--border);
}

.kv-table tr:hover {
  background: var(--bg-hover);
}

.kv-table td {
  padding: 4px 10px;
  vertical-align: top;
  font-size: 11px;
}

.kv-name {
  color: var(--text);
  white-space: nowrap;
  width: 1%;
  padding-right: 16px;
}

.kv-value {
  color: var(--success);
  word-break: break-all;
}

.body-card {
  flex: 1;
  min-width: 0;
}

.body-panel {
  flex: 1;
  min-width: 0;
}
</style>
