<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useBackend } from "../api";
import type { BodyPayload, HeaderItem, LogDetail, Modification } from "../api/types";
import { appStore, reportError } from "../stores/app";
import { formatBytes } from "../utils/format";
import { bodyLanguage } from "../utils/body-language";
import MonacoEditor from "../components/MonacoEditor.vue";
import { Io5Checkmark, Io5Copy, Io5Warning } from "vue-icons-plus/io5";

const props = defineProps<{ sessionId: number; logId: number }>();

const backend = useBackend();

const detail = ref<LogDetail | null>(null);
const loading = ref(true);
const loadError = ref<string | null>(null);

let autoRefreshTimer: number | undefined;

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

// 请求仍在进行时自动轮询，完成后停止
watch(
  () => detail.value?.outcome,
  (outcome) => {
    if (autoRefreshTimer !== undefined) {
      window.clearTimeout(autoRefreshTimer);
      autoRefreshTimer = undefined;
    }
    if (outcome === "in_progress") {
      autoRefreshTimer = window.setTimeout(() => void load(), 2000);
    }
  },
);

onMounted(load);
onBeforeUnmount(() => {
  if (autoRefreshTimer !== undefined) window.clearTimeout(autoRefreshTimer);
  if (copiedTimer !== undefined) window.clearTimeout(copiedTimer);
});

// ---------- tabs ----------

type TabKey = "request" | "response" | "modifications";
const activeTab = ref<TabKey>("request");

const modificationCount = computed(
  () =>
    (detail.value?.req_modifications.length ?? 0) +
    (detail.value?.resp_modifications.length ?? 0),
);

const tabs = computed(() => [
  { key: "request" as TabKey, label: "请求" },
  {
    key: "response" as TabKey,
    label: "响应",
    disabled: !detail.value?.response,
  },
  {
    key: "modifications" as TabKey,
    label: "修改记录",
    count: modificationCount.value,
    disabled: modificationCount.value === 0,
  },
]);

// ---------- 信息区 / Body 分隔条 ----------

const topRatio = ref(0.45);

function startSplit(event: PointerEvent): void {
  const page = (event.currentTarget as HTMLElement).parentElement;
  if (!page) return;
  const rect = page.getBoundingClientRect();
  const onMove = (e: PointerEvent) => {
    const ratio = (e.clientY - rect.top) / rect.height;
    topRatio.value = Math.min(0.85, Math.max(0.15, ratio));
  };
  const onUp = () => {
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUp);
  };
  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", onUp);
  event.preventDefault();
}

// ---------- summary ----------

const methodClass = computed(() => {
  const method = detail.value?.request.method.toUpperCase();
  switch (method) {
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
  if (status >= 200 && status < 300) return "s-2xx";
  if (status >= 300 && status < 400) return "s-3xx";
  if (status >= 400 && status < 500) return "s-4xx";
  if (status >= 500) return "s-5xx";
  return "s-other";
}

// URL 分段着色
interface UrlSegment {
  text: string;
  cls: string;
}

const urlSegments = computed<UrlSegment[]>(() => {
  const uri = detail.value?.request.uri;
  if (!uri) return [];
  try {
    const url = new URL(uri);
    const segments: UrlSegment[] = [
      { text: `${url.protocol}//`, cls: "url-scheme" },
      { text: url.host, cls: "url-host" },
    ];
    const path = url.pathname;
    if (path && path !== "/") segments.push({ text: path, cls: "url-path" });
    else if (path) segments.push({ text: path, cls: "url-scheme" });
    if (url.search) segments.push({ text: url.search, cls: "url-query" });
    if (url.hash) segments.push({ text: url.hash, cls: "url-query" });
    return segments;
  } catch {
    return [{ text: uri, cls: "url-path" }];
  }
});

const bodySizeLabel = computed(() => {
  const body = detail.value?.request.body;
  return body ? bodySize(body) : "";
});

function bodySize(body: BodyPayload): string {
  switch (body.type) {
    case "empty":
      return "0 B";
    case "text":
      return formatBytes(new TextEncoder().encode(body.content).length);
    case "json":
      return formatBytes(new TextEncoder().encode(JSON.stringify(body.content)).length);
    case "binary":
    case "large":
      return formatBytes(body.size);
  }
}

// ---------- copy ----------

const copiedKey = ref<string | null>(null);
let copiedTimer: number | undefined;

async function copyText(key: string, text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
    copiedKey.value = key;
    if (copiedTimer !== undefined) window.clearTimeout(copiedTimer);
    copiedTimer = window.setTimeout(() => (copiedKey.value = null), 1200);
  } catch {
    appStore.toast("复制失败", "error");
  }
}

// ---------- query params ----------

interface QueryParam {
  name: string;
  value: string;
}

const queryParams = computed<QueryParam[]>(() => {
  const uri = detail.value?.request.uri;
  if (!uri) return [];
  try {
    const url = new URL(uri);
    const params: QueryParam[] = [];
    url.searchParams.forEach((value, name) => params.push({ name, value }));
    return params;
  } catch {
    return [];
  }
});

// ---------- body viewing ----------

/** 可格式化（pretty/raw 切换）仅适用于“看起来是 JSON 的文本”。 */
function bodyFormattable(body: BodyPayload, headers: HeaderItem[]): boolean {
  return body.type === "text" && bodyLanguage(body, headers) === "json";
}

const prettyMode = ref<Record<string, boolean>>({});

function isPretty(which: "req" | "resp", body: BodyPayload): boolean {
  if (body.type === "json") return true;
  return prettyMode.value[which] ?? true;
}

function bodyEditorValue(
  which: "req" | "resp",
  body: BodyPayload,
  headers: HeaderItem[],
): string {
  if (body.type === "json") return JSON.stringify(body.content, null, 2);
  if (body.type === "text") {
    if (isPretty(which, body) && bodyLanguage(body, headers) === "json") {
      try {
        return JSON.stringify(JSON.parse(body.content), null, 2);
      } catch {
        return body.content;
      }
    }
    return body.content;
  }
  return "";
}

function bodyRawText(body: BodyPayload): string {
  if (body.type === "json") return JSON.stringify(body.content, null, 2);
  if (body.type === "text") return body.content;
  return "";
}

// ---------- modifications ----------

const modificationKindLabels: Record<string, string> = {
  header_append: "追加头",
  header_set: "设置头",
  header_remove: "删除头",
  body_replace_string: "替换 Body 为字符串",
  body_replace_file: "替换 Body 为文件",
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
      return mod.values.length > 0
        ? `${mod.name}（原值：${mod.values.join(", ")}）`
        : mod.name;
    case "body_replace_string":
      return mod.content;
    case "body_replace_file":
      return mod.path;
  }
}

function headerCount(headers: HeaderItem[]): string {
  return headers.length > 0 ? String(headers.length) : "";
}
</script>

<template>
  <div class="log-detail">
    <!-- loading / error states -->
    <div v-if="loading && !detail" class="state-hint">加载中…</div>
    <div v-else-if="loadError && !detail" class="state-hint state-error">
      <Io5Warning :size="22" />
      <p>加载失败：{{ loadError }}</p>
      <button class="btn" @click="load()">重试</button>
    </div>

    <template v-else-if="detail">
      <!-- 固定摘要栏 -->
      <header class="summary">
        <div class="summary-line">
          <span class="method-chip mono" :class="methodClass">{{ detail.request.method }}</span>
          <span
            v-if="detail.response"
            class="status-chip mono"
            :class="statusClass(detail.response.status)"
          >
            {{ detail.response.status }} {{ detail.response.status_text }}
          </span>
          <span class="outcome-chip" :class="'o-' + detail.outcome">
            <span class="o-dot" />{{ outcomeLabel(detail.outcome) }}
          </span>
          <span v-if="loading" class="refreshing text-faint">刷新中…</span>
          <span class="summary-spacer" />
          <button class="btn icon" title="复制 URL" @click="copyText('url', detail.request.uri)">
            <Io5Checkmark v-if="copiedKey === 'url'" :size="14" class="text-success" />
            <Io5Copy v-else :size="14" />
          </button>
        </div>
        <div class="url mono" :title="detail.request.uri">
          <span v-for="(seg, i) in urlSegments" :key="i" :class="seg.cls">{{ seg.text }}</span>
        </div>
        <div class="meta-strip">
          <span class="meta-item">
            <span class="meta-label">来源</span>{{ detail.source_addr ?? "未知" }}
          </span>
          <span class="meta-item">
            <span class="meta-label">阶段</span>{{ detail.stage }}
          </span>
          <span class="meta-item">
            <span class="meta-label">HTTP</span>{{ detail.request.version }}
          </span>
          <span v-if="bodySizeLabel" class="meta-item">
            <span class="meta-label">请求体</span>{{ bodySizeLabel }}
          </span>
        </div>
      </header>

      <!-- 错误横幅 -->
      <div v-if="detail.error" class="error-banner">
        <Io5Warning :size="14" class="error-icon" />
        <div class="error-body">
          <div class="error-head mono">{{ detail.error.stage }} · {{ detail.error.kind }}</div>
          <div class="error-message mono">{{ detail.error.message }}</div>
        </div>
      </div>

      <!-- Tab 栏 -->
      <nav class="tab-bar">
        <button
          v-for="tab in tabs"
          :key="tab.key"
          class="tab-btn"
          :class="{ active: activeTab === tab.key }"
          :disabled="tab.disabled"
          @click="activeTab = tab.key"
        >
          {{ tab.label }}
          <span v-if="tab.count" class="tab-count">{{ tab.count }}</span>
        </button>
      </nav>

      <!-- 请求 -->
      <div v-show="activeTab === 'request'" class="tab-page">
        <div class="pane-top" :style="{ height: `calc(${topRatio * 100}% - 3px)` }">
          <section v-if="queryParams.length > 0" class="card">
            <div class="card-title">
              Query 参数 <span class="count-badge">{{ queryParams.length }}</span>
            </div>
            <table class="kv-table">
              <tbody>
                <tr v-for="(p, i) in queryParams" :key="i">
                  <td class="mono kv-name">{{ p.name }}</td>
                  <td class="mono kv-value">{{ p.value }}</td>
                </tr>
              </tbody>
            </table>
          </section>

          <section class="card">
            <div class="card-title">
              请求头 <span class="count-badge">{{ headerCount(detail.request.headers) }}</span>
            </div>
            <table v-if="detail.request.headers.length > 0" class="kv-table">
              <tbody>
                <tr v-for="(h, i) in detail.request.headers" :key="i">
                  <td class="mono kv-name">{{ h.name }}</td>
                  <td class="mono kv-value">{{ h.value }}</td>
                </tr>
              </tbody>
            </table>
            <div v-else class="empty-hint">无请求头</div>
          </section>
        </div>

        <div class="splitter" title="拖动调整区域大小" @pointerdown="startSplit" />

        <section class="card body-card">
          <div class="card-title body-title">
            <span>请求体</span>
            <span class="body-tools">
              <template v-if="bodyFormattable(detail.request.body, detail.request.headers)">
                <button
                  class="tool-btn"
                  :class="{ on: isPretty('req', detail.request.body) }"
                  @click="prettyMode = { ...prettyMode, req: true }"
                >
                  格式化
                </button>
                <button
                  class="tool-btn"
                  :class="{ on: !isPretty('req', detail.request.body) }"
                  @click="prettyMode = { ...prettyMode, req: false }"
                >
                  原文
                </button>
              </template>
              <button
                v-if="detail.request.body.type === 'text' || detail.request.body.type === 'json'"
                class="btn icon"
                title="复制请求体"
                @click="copyText('req-body', bodyRawText(detail.request.body))"
              >
                <Io5Checkmark v-if="copiedKey === 'req-body'" :size="13" class="text-success" />
                <Io5Copy v-else :size="13" />
              </button>
            </span>
          </div>
          <div v-if="detail.request.body.type === 'empty'" class="empty-hint">无请求体</div>
          <div
            v-else-if="detail.request.body.type === 'binary' || detail.request.body.type === 'large'"
            class="empty-hint"
          >
            二进制数据（{{ bodySize(detail.request.body) }}），暂不支持预览
          </div>
          <div v-else class="body-editor">
            <MonacoEditor
              :model-value="
                bodyEditorValue('req', detail.request.body, detail.request.headers)
              "
              :language="bodyLanguage(detail.request.body, detail.request.headers)"
              readonly
            />
          </div>
        </section>
      </div>

      <!-- 响应 -->
      <div v-show="activeTab === 'response'" class="tab-page">
        <template v-if="detail.response">
          <div class="pane-top" :style="{ height: `calc(${topRatio * 100}% - 3px)` }">
            <section class="card">
              <div class="card-title">
                响应头 <span class="count-badge">{{ headerCount(detail.response.headers) }}</span>
                <span class="card-title-extra mono">{{ detail.response.version }}</span>
              </div>
              <table v-if="detail.response.headers.length > 0" class="kv-table">
                <tbody>
                  <tr v-for="(h, i) in detail.response.headers" :key="i">
                    <td class="mono kv-name">{{ h.name }}</td>
                    <td class="mono kv-value">{{ h.value }}</td>
                  </tr>
                </tbody>
              </table>
              <div v-else class="empty-hint">无响应头</div>
            </section>
          </div>

          <div class="splitter" title="拖动调整区域大小" @pointerdown="startSplit" />

          <section class="card body-card">
            <div class="card-title body-title">
              <span>响应体</span>
              <span class="body-tools">
                <template v-if="bodyFormattable(detail.response.body, detail.response.headers)">
                  <button
                    class="tool-btn"
                    :class="{ on: isPretty('resp', detail.response.body) }"
                    @click="prettyMode = { ...prettyMode, resp: true }"
                  >
                    格式化
                  </button>
                  <button
                    class="tool-btn"
                    :class="{ on: !isPretty('resp', detail.response.body) }"
                    @click="prettyMode = { ...prettyMode, resp: false }"
                  >
                    原文
                  </button>
                </template>
                <button
                  v-if="detail.response.body.type === 'text' || detail.response.body.type === 'json'"
                  class="btn icon"
                  title="复制响应体"
                  @click="copyText('resp-body', bodyRawText(detail.response.body))"
                >
                  <Io5Checkmark v-if="copiedKey === 'resp-body'" :size="13" class="text-success" />
                  <Io5Copy v-else :size="13" />
                </button>
              </span>
            </div>
            <div v-if="detail.response.body.type === 'empty'" class="empty-hint">无响应体</div>
            <div
              v-else-if="detail.response.body.type === 'binary' || detail.response.body.type === 'large'"
              class="empty-hint"
            >
              二进制数据（{{ bodySize(detail.response.body) }}），暂不支持预览
            </div>
            <div v-else class="body-editor">
              <MonacoEditor
                :model-value="
                  bodyEditorValue('resp', detail.response.body, detail.response.headers)
                "
                :language="bodyLanguage(detail.response.body, detail.response.headers)"
                readonly
              />
            </div>
          </section>
        </template>
      </div>

      <!-- 修改记录 -->
      <div v-show="activeTab === 'modifications'" class="tab-page">
        <div class="pane-top full">
          <section v-if="detail.req_modifications.length > 0" class="card">
            <div class="card-title">请求修改</div>
            <ul class="mod-list">
              <li v-for="(mod, i) in detail.req_modifications" :key="i" class="mod-item">
                <span class="mod-badge">{{ modificationLabel(mod) }}</span>
                <span class="mod-detail mono">{{ modificationDetail(mod) }}</span>
              </li>
            </ul>
          </section>
          <section v-if="detail.resp_modifications.length > 0" class="card">
            <div class="card-title">响应修改</div>
            <ul class="mod-list">
              <li v-for="(mod, i) in detail.resp_modifications" :key="i" class="mod-item">
                <span class="mod-badge">{{ modificationLabel(mod) }}</span>
                <span class="mod-detail mono">{{ modificationDetail(mod) }}</span>
              </li>
            </ul>
          </section>
        </div>
      </div>
    </template>
  </div>
</template>

<style scoped>
.log-detail {
  flex: 1;
  min-height: 0;
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
}
.state-error {
  color: var(--danger);
}
.state-error p {
  margin: 0;
  color: var(--text-secondary);
  font-size: 12px;
  word-break: break-all;
  padding: 0 16px;
}

/* ---------- 摘要栏 ---------- */

.summary {
  flex: none;
  padding: 12px 14px 10px;
  border-bottom: 1px solid var(--border);
  background: var(--bg-panel);
}
.summary-line {
  display: flex;
  align-items: center;
  gap: 8px;
}
.summary-spacer {
  flex: 1;
}
.refreshing {
  font-size: 11px;
}

.method-chip {
  padding: 2px 8px;
  border-radius: 6px;
  font-size: 11px;
  font-weight: 700;
  color: #fff;
}
.m-get { background: var(--success); }
.m-post { background: var(--accent); }
.m-put { background: var(--warning); }
.m-patch { background: #8b5cf6; }
.m-delete { background: var(--danger); }
.m-other { background: var(--text-faint); }

.status-chip {
  font-size: 11px;
  font-weight: 700;
}
.s-2xx { color: var(--success); }
.s-3xx { color: var(--accent); }
.s-4xx { color: var(--warning); }
.s-5xx { color: var(--danger); }
.s-other { color: var(--text-secondary); }

.outcome-chip {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 10px;
  background: var(--bg-active);
  color: var(--text-secondary);
}
.o-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--text-faint);
}
.o-in_progress { color: var(--warning); }
.o-in_progress .o-dot { background: var(--warning); }
.o-success { color: var(--success); }
.o-success .o-dot { background: var(--success); }
.o-failed { color: var(--danger); }
.o-failed .o-dot { background: var(--danger); }

/* URL 分段着色 */
.url {
  margin-top: 8px;
  font-size: 12px;
  line-height: 1.4;
  word-break: break-all;
}
.url-scheme { color: var(--text-faint); }
.url-host { color: var(--accent); font-weight: 600; }
.url-path { color: var(--text); }
.url-query { color: var(--warning); }

.meta-strip {
  display: flex;
  flex-wrap: wrap;
  gap: 6px 16px;
  margin-top: 8px;
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

/* ---------- 错误横幅 ---------- */

.error-banner {
  flex: none;
  display: flex;
  gap: 8px;
  margin: 10px 12px 0;
  padding: 8px 10px;
  border: 1px solid var(--danger);
  border-radius: var(--radius-md);
  background: rgba(227, 77, 89, 0.07);
}
.error-icon {
  color: var(--danger);
  flex: none;
  margin-top: 2px;
}
.error-body {
  min-width: 0;
}
.error-head {
  font-size: 11px;
  font-weight: 600;
  color: var(--danger);
}
.error-message {
  font-size: 11px;
  word-break: break-all;
  color: var(--text);
  margin-top: 2px;
}

/* ---------- Tab 栏 ---------- */

.tab-bar {
  flex: none;
  display: flex;
  gap: 2px;
  padding: 8px 12px 0;
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
.tab-btn:hover:not(:disabled):not(.active) {
  color: var(--text);
  background: var(--bg-hover);
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
.tab-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}
.tab-count {
  font-size: 10px;
  padding: 0 5px;
  border-radius: 8px;
  background: var(--bg-active);
}

/* ---------- Tab 页：信息区 + 分隔条 + Body ---------- */

.tab-page {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding: 10px 12px 12px;
  overflow: hidden;
}
.pane-top {
  flex: none;
  min-height: 60px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding-right: 2px;
}
.pane-top.full {
  flex: 1;
  height: auto !important;
}

.splitter {
  flex: none;
  height: 7px;
  margin: 0 -2px;
  cursor: row-resize;
  position: relative;
}
.splitter::after {
  content: "";
  position: absolute;
  left: 40%;
  right: 40%;
  top: 3px;
  height: 2px;
  border-radius: 1px;
  background: var(--border-strong);
  transition: background 0.12s;
}
.splitter:hover::after,
.splitter:active::after {
  background: var(--accent);
}

.card {
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--bg-panel);
  overflow: hidden;
  flex: none;
}
.card-title {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 10px;
  font-size: 11px;
  font-weight: 600;
  color: var(--text-secondary);
  border-bottom: 1px solid var(--border);
  background: var(--bg-app);
  position: sticky;
  top: 0;
  z-index: 1;
}
.card-title-extra {
  margin-left: auto;
  font-weight: 400;
  color: var(--text-faint);
}
.count-badge {
  font-size: 10px;
  font-weight: 500;
  padding: 0 5px;
  border-radius: 8px;
  background: var(--bg-active);
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

/* ---------- Body 查看器 ---------- */

.body-card {
  flex: 1;
  min-height: 80px;
  display: flex;
  flex-direction: column;
  margin-top: 3px;
}
.body-title {
  flex: none;
}
.body-tools {
  margin-left: auto;
  display: inline-flex;
  align-items: center;
  gap: 2px;
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
.tool-btn:first-child {
  border-radius: 6px 0 0 6px;
}
.tool-btn:nth-child(2) {
  border-radius: 0 6px 6px 0;
  border-left: none;
}
.tool-btn.on {
  background: var(--accent);
  border-color: var(--accent);
  color: var(--accent-text);
}
.body-editor {
  flex: 1;
  min-height: 0;
  display: flex;
}
.body-card .empty-hint {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
}

/* ---------- 修改记录 ---------- */

.mod-list {
  list-style: none;
  margin: 0;
  padding: 6px 10px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.mod-item {
  display: flex;
  align-items: baseline;
  gap: 8px;
  font-size: 11px;
}
.mod-badge {
  flex: none;
  font-size: 10px;
  padding: 1px 7px;
  border-radius: 8px;
  background: rgba(51, 112, 255, 0.12);
  color: var(--accent);
}
.mod-detail {
  word-break: break-all;
  color: var(--text-secondary);
}
</style>
