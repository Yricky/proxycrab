<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useBackend } from "../api";
import type { BodyPayload, LogDetail, Modification } from "../api/types";
import { reportError } from "../stores/app";
import { formatBytes } from "../utils/format";
import MonacoEditor from "../components/MonacoEditor.vue";
import {
  Io5ChevronDown,
  Io5ChevronForward,
  Io5Refresh,
  Io5Warning,
} from "vue-icons-plus/io5";

const props = defineProps<{ sessionId: number; logId: number }>();

const backend = useBackend();

const detail = ref<LogDetail | null>(null);
const loading = ref(true);
const loadError = ref<string | null>(null);
const requestOpen = ref(true);
const responseOpen = ref(true);

async function load(): Promise<void> {
  loading.value = true;
  loadError.value = null;
  try {
    detail.value = await backend.getLog(props.sessionId, props.logId);
  } catch (error) {
    loadError.value = reportError(error, "加载日志详情失败");
  } finally {
    loading.value = false;
  }
}

onMounted(load);

// ---------- overview ----------

const methodClass = computed(() => {
  const method = detail.value?.request.method.toUpperCase();
  if (method === "POST") return "method-post";
  if (method === "GET") return "method-get";
  return "method-other";
});

const outcomeLabels: Record<string, string> = {
  in_progress: "进行中",
  success: "成功",
  failed: "失败",
  tunneled: "隧道",
};

function outcomeLabel(outcome: string): string {
  return outcomeLabels[outcome] ?? outcome;
}

function statusClass(status: number): string {
  if (status >= 200 && status < 300) return "status-2xx";
  if (status >= 300 && status < 400) return "status-3xx";
  if (status >= 400 && status < 500) return "status-4xx";
  if (status >= 500) return "status-5xx";
  return "status-other";
}

// ---------- body rendering ----------

function guessTextLanguage(content: string): string {
  const trimmed = content.trimStart();
  return trimmed.startsWith("{") || trimmed.startsWith("[") ? "json" : "plaintext";
}

// Only called for text/json bodies; empty/binary/large are rendered separately.
function bodyEditorValue(body: BodyPayload): string {
  if (body.type === "json") return JSON.stringify(body.content, null, 2);
  if (body.type === "text") return body.content;
  return "";
}

function bodyEditorLanguage(body: BodyPayload): string {
  if (body.type === "json") return "json";
  if (body.type === "text") return guessTextLanguage(body.content);
  return "plaintext";
}

// ---------- modifications ----------

const modificationKindLabels: Record<string, string> = {
  header_append: "追加头",
  header_set: "设置头",
  header_remove: "删除头",
  body_replace_string: "替换Body为字符串",
  body_replace_file: "替换Body为文件",
};

function modificationLabel(mod: Modification): string {
  return modificationKindLabels[mod.kind] ?? mod.kind;
}

function modificationDetail(mod: Modification): string {
  switch (mod.kind) {
    case "header_append":
    case "header_set":
      return `${mod.name}: ${mod.value}`;
    case "header_remove":
      return mod.values.length > 0 ? `${mod.name}（原值：${mod.values.join(", ")}）` : mod.name;
    case "body_replace_string":
      return mod.content;
    case "body_replace_file":
      return mod.path;
  }
}
</script>

<template>
  <div class="log-detail">
    <div v-if="loading && !detail" class="empty-hint">加载中…</div>
    <div v-else-if="loadError" class="detail-error-state">
      <Io5Warning :size="20" />
      <p class="text-secondary">加载失败：{{ loadError }}</p>
      <button class="btn" @click="load">重试</button>
    </div>
    <template v-else-if="detail">
      <!-- 概览 -->
      <div class="overview">
        <div class="overview-head">
          <span class="badge method-badge mono" :class="methodClass">{{
            detail.request.method
          }}</span>
          <span class="mono url">{{ detail.request.uri }}</span>
          <button
            class="btn icon refresh-btn"
            :class="{ spinning: loading }"
            title="刷新"
            :disabled="loading"
            @click="load"
          >
            <Io5Refresh :size="14" />
          </button>
        </div>
        <div class="overview-meta text-secondary">
          <span v-if="detail.source_addr">来源 {{ detail.source_addr }}</span>
          <span v-else>来源未知</span>
          <span>阶段 {{ detail.stage }}</span>
          <span
            >结果
            <span class="badge" :class="'outcome-' + detail.outcome">{{
              outcomeLabel(detail.outcome)
            }}</span></span
          >
        </div>
      </div>

      <!-- 错误 -->
      <div v-if="detail.error" class="error-card">
        <div class="error-title"><Io5Warning :size="13" /> 错误</div>
        <div class="mono error-line">阶段：{{ detail.error.stage }}</div>
        <div class="mono error-line">类型：{{ detail.error.kind }}</div>
        <div class="mono error-line">{{ detail.error.message }}</div>
      </div>

      <!-- 请求 -->
      <section class="section">
        <div class="section-head" @click="requestOpen = !requestOpen">
          <Io5ChevronDown v-if="requestOpen" :size="13" />
          <Io5ChevronForward v-else :size="13" />
          <span class="section-title">请求</span>
        </div>
        <template v-if="requestOpen">
          <div class="sub-title text-faint">请求头</div>
          <table v-if="detail.request.headers.length > 0" class="header-table">
            <tbody>
              <tr v-for="(h, i) in detail.request.headers" :key="i">
                <td class="mono header-name">{{ h.name }}</td>
                <td class="mono header-value">{{ h.value }}</td>
              </tr>
            </tbody>
          </table>
          <div v-else class="empty-hint">无请求头</div>

          <div class="sub-title text-faint">请求体</div>
          <template v-if="detail.request.body.type === 'empty'">
            <div class="empty-hint">无请求体</div>
          </template>
          <div v-else-if="detail.request.body.type === 'binary' || detail.request.body.type === 'large'" class="empty-hint">
            二进制数据（{{ formatBytes(detail.request.body.size) }}），暂不支持预览
          </div>
          <div v-else class="body-editor">
            <MonacoEditor
              :model-value="bodyEditorValue(detail.request.body)"
              :language="bodyEditorLanguage(detail.request.body)"
              readonly
            />
          </div>

          <template v-if="detail.req_modifications.length > 0">
            <div class="sub-title text-faint">请求修改记录</div>
            <ul class="mod-list">
              <li v-for="(mod, i) in detail.req_modifications" :key="i" class="mod-item mono">
                <span class="badge mod-badge">{{ modificationLabel(mod) }}</span>
                <span class="mod-detail">{{ modificationDetail(mod) }}</span>
              </li>
            </ul>
          </template>
        </template>
      </section>

      <!-- 响应 -->
      <section v-if="detail.response" class="section">
        <div class="section-head" @click="responseOpen = !responseOpen">
          <Io5ChevronDown v-if="responseOpen" :size="13" />
          <Io5ChevronForward v-else :size="13" />
          <span class="section-title">响应</span>
          <span class="mono status-line" :class="statusClass(detail.response.status)">
            {{ detail.response.status }} {{ detail.response.status_text }}
          </span>
        </div>
        <template v-if="responseOpen">
          <div class="sub-title text-faint">响应头</div>
          <table v-if="detail.response.headers.length > 0" class="header-table">
            <tbody>
              <tr v-for="(h, i) in detail.response.headers" :key="i">
                <td class="mono header-name">{{ h.name }}</td>
                <td class="mono header-value">{{ h.value }}</td>
              </tr>
            </tbody>
          </table>
          <div v-else class="empty-hint">无响应头</div>

          <div class="sub-title text-faint">响应体</div>
          <template v-if="detail.response.body.type === 'empty'">
            <div class="empty-hint">无响应体</div>
          </template>
          <div v-else-if="detail.response.body.type === 'binary' || detail.response.body.type === 'large'" class="empty-hint">
            二进制数据（{{ formatBytes(detail.response.body.size) }}），暂不支持预览
          </div>
          <div v-else class="body-editor">
            <MonacoEditor
              :model-value="bodyEditorValue(detail.response.body)"
              :language="bodyEditorLanguage(detail.response.body)"
              readonly
            />
          </div>

          <template v-if="detail.resp_modifications.length > 0">
            <div class="sub-title text-faint">响应修改记录</div>
            <ul class="mod-list">
              <li v-for="(mod, i) in detail.resp_modifications" :key="i" class="mod-item mono">
                <span class="badge mod-badge">{{ modificationLabel(mod) }}</span>
                <span class="mod-detail">{{ modificationDetail(mod) }}</span>
              </li>
            </ul>
          </template>
        </template>
      </section>
    </template>
  </div>
</template>

<style scoped>
.log-detail {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  overflow-y: auto;
  padding: 12px;
  user-select: text;
}

.detail-error-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-4);
  color: var(--danger);
}
.detail-error-state p {
  margin: 0;
  color: var(--text-secondary);
}

/* ---------- overview ---------- */

.overview-head {
  display: flex;
  align-items: flex-start;
  gap: var(--space-2);
}
.url {
  flex: 1;
  min-width: 0;
  word-break: break-all;
  user-select: text;
}
.refresh-btn {
  flex: none;
}
.refresh-btn.spinning {
  opacity: 0.6;
}

.method-badge {
  flex: none;
  color: #fff;
}
.method-post {
  background: var(--accent);
  color: var(--accent-text);
}
.method-get {
  background: var(--success);
}
.method-other {
  background: var(--text-faint);
}

.overview-meta {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-3);
  font-size: 12px;
  margin-top: 6px;
}

.outcome-in_progress {
  background: rgba(242, 153, 74, 0.18);
  color: var(--warning);
}
.outcome-success {
  background: rgba(52, 168, 83, 0.16);
  color: var(--success);
}
.outcome-failed {
  background: rgba(227, 77, 89, 0.15);
  color: var(--danger);
}
.outcome-tunneled {
  background: var(--bg-active);
  color: var(--text-secondary);
}

/* ---------- error ---------- */

.error-card {
  border: 1px solid var(--danger);
  border-radius: var(--radius-md);
  padding: var(--space-2) var(--space-3);
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.error-title {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
  font-weight: 600;
  color: var(--danger);
  margin-bottom: 2px;
}
.error-line {
  word-break: break-all;
  color: var(--text);
}

/* ---------- sections ---------- */

.section {
  border-top: 1px solid var(--border);
  padding-top: var(--space-2);
}
.section-head {
  display: flex;
  align-items: center;
  gap: 6px;
  cursor: pointer;
  color: var(--text-secondary);
}
.section-head .section-title {
  margin: 0;
}
.status-line {
  font-weight: 600;
}
.status-2xx {
  color: var(--success);
}
.status-3xx {
  color: var(--accent);
}
.status-4xx {
  color: var(--warning);
}
.status-5xx {
  color: var(--danger);
}
.status-other {
  color: var(--text-secondary);
}

.sub-title {
  font-size: 11px;
  margin: var(--space-2) 0 4px;
}

.header-table {
  width: 100%;
  border-collapse: collapse;
}
.header-table td {
  padding: 2px 6px;
  vertical-align: top;
}
.header-name {
  color: var(--text-secondary);
  white-space: nowrap;
  width: 1%;
  padding-right: var(--space-3);
}
.header-value {
  word-break: break-all;
}

.body-editor {
  height: 200px;
  display: flex;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  overflow: hidden;
}

/* ---------- modifications ---------- */

.mod-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.mod-item {
  display: flex;
  align-items: baseline;
  gap: 6px;
  font-size: 12px;
}
.mod-badge {
  flex: none;
}
.mod-detail {
  word-break: break-all;
  color: var(--text-secondary);
}
</style>
