<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useBackend } from "../api";
import type {
  BodyPayload,
  BreakpointSummary,
  HeaderItem,
  InterceptorExecution,
  LogDetail,
  Modification,
} from "../api/types";
import { BackendError } from "../api/tauri-backend";
import { appStore, reportError } from "../stores/app";
import { formatBytes } from "../utils/format";
import { bodyLanguage } from "../utils/body-language";
import MonacoEditor from "../components/MonacoEditor.vue";
import { Io5Checkmark, Io5Copy, Io5Warning } from "vue-icons-plus/io5";
import { openScriptSnapshot } from "./launcher";

const props = defineProps<{
  sessionId?: number;
  logId?: number;
  breakpointId?: number;
}>();

const backend = useBackend();

const detail = ref<LogDetail | null>(null);
const loading = ref(true);
const loadError = ref<string | null>(null);
const breakpoint = ref<BreakpointSummary | null>(null);
const temporaryScript = ref("");
const scriptPanelOpen = ref(false);
const executingScript = ref(false);
const releasing = ref(false);
const extending = ref(false);
const extensionSeconds = ref(60);

let autoRefreshTimer: number | undefined;
let breakpointRefreshTimer: number | undefined;
let loadInFlight = false;

async function load(silent = false): Promise<void> {
  if (loadInFlight) return;
  loadInFlight = true;
  if (!silent) loading.value = true;
  loadError.value = null;
  try {
    if (props.breakpointId !== undefined && breakpoint.value !== null) {
      const payload = await backend.getBreakpoint(props.breakpointId);
      breakpoint.value = payload.breakpoint;
      detail.value = payload.log;
    } else if (props.breakpointId !== undefined && detail.value === null) {
      const payload = await backend.getBreakpoint(props.breakpointId);
      breakpoint.value = payload.breakpoint;
      detail.value = payload.log;
      activeTab.value = payload.breakpoint.phase;
    } else if (props.sessionId !== undefined && props.logId !== undefined) {
      detail.value = await backend.getLog(props.sessionId, props.logId);
    } else if (detail.value) {
      detail.value = await backend.getLog(detail.value.session_id, detail.value.id);
    }
  } catch (error) {
    if (
      silent &&
      error instanceof BackendError &&
      error.code === "not_found" &&
      detail.value
    ) {
      breakpoint.value = null;
      try {
        detail.value = await backend.getLog(detail.value.session_id, detail.value.id);
      } catch {
        // Keep the last live snapshot while the resumed request finishes persisting.
      }
    } else if (!silent) {
      loadError.value = reportError(error, "加载日志详情失败");
    }
  } finally {
    loading.value = false;
    loadInFlight = false;
  }
}

async function extendBreakpoint(): Promise<void> {
  if (!breakpoint.value || !Number.isFinite(extensionSeconds.value)) return;
  extending.value = true;
  try {
    breakpoint.value = await backend.extendBreakpoint(breakpoint.value.id, {
      timeout_ms: Math.max(0, Math.floor(extensionSeconds.value * 1000)),
    });
  } catch (error) {
    reportError(error, "延长断点失败");
  } finally {
    extending.value = false;
  }
}

async function releaseBreakpoint(): Promise<void> {
  if (!breakpoint.value) return;
  releasing.value = true;
  try {
    await backend.releaseBreakpoint(breakpoint.value.id);
    breakpoint.value = null;
    appStore.toast("断点已放行", "success");
    window.setTimeout(() => void load(true), 100);
  } catch (error) {
    reportError(error, "放行断点失败");
  } finally {
    releasing.value = false;
  }
}

async function executeTemporaryScript(): Promise<void> {
  if (!breakpoint.value || executingScript.value) return;
  executingScript.value = true;
  try {
    const result = await backend.executeBreakpointScript(breakpoint.value.id, {
      content: temporaryScript.value,
    });
    breakpoint.value = result.breakpoint;
    await load(true);
    if (result.execution.error) {
      appStore.toast(`临时脚本报错：${result.execution.error}`, "error", 5000);
    } else {
      appStore.toast("临时脚本修改已应用，断点仍保持", "success");
    }
  } catch (error) {
    reportError(error, "执行临时脚本失败");
  } finally {
    executingScript.value = false;
  }
}

function remainingLabel(): string {
  return `${Math.max(0, Math.ceil((breakpoint.value?.remaining_ms ?? 0) / 1000))} 秒`;
}

// 请求仍在进行时自动轮询，完成后停止
watch(
  () => detail.value?.outcome,
  (outcome) => {
    if (autoRefreshTimer !== undefined) {
      window.clearTimeout(autoRefreshTimer);
      autoRefreshTimer = undefined;
    }
    if (props.breakpointId === undefined && outcome === "in_progress") {
      autoRefreshTimer = window.setTimeout(() => void load(), 2000);
    }
  },
);

onMounted(() => {
  void load();
  if (props.breakpointId !== undefined) {
    breakpointRefreshTimer = window.setInterval(() => {
      if (breakpoint.value || detail.value?.outcome === "in_progress") void load(true);
    }, 500);
  }
});
onBeforeUnmount(() => {
  if (autoRefreshTimer !== undefined) window.clearTimeout(autoRefreshTimer);
  if (copiedTimer !== undefined) window.clearTimeout(copiedTimer);
  if (breakpointRefreshTimer !== undefined) window.clearInterval(breakpointRefreshTimer);
});

// ---------- tabs ----------

type TabKey = "request" | "response" | "modifications";
const activeTab = ref<TabKey>("request");

const interceptorCount = computed(
  () =>
    (detail.value?.request_interceptors.length ?? 0) +
    (detail.value?.response_interceptors.length ?? 0),
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
    label: "拦截器",
    count: interceptorCount.value,
    disabled: interceptorCount.value === 0,
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
  snapshot: "执行快照",
  method_set: "设置 Method",
  uri_set: "设置 URI",
  status_set: "设置状态码",
  header_append: "追加头",
  header_set: "设置头",
  header_remove: "删除头",
  body_replace_string: "替换 Body 为字符串",
  body_replace_file: "替换 Body 为文件",
  tag_set: "设置 Tag",
};

function modificationLabel(mod: Modification): string {
  return modificationKindLabels[mod.kind] ?? mod.kind;
}

function modificationDetail(mod: Modification): string {
  switch (mod.kind) {
    case "method_set":
      return mod.method;
    case "uri_set":
      return mod.uri;
    case "status_set":
      return String(mod.status);
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
    case "tag_set":
      return `${mod.key}: ${mod.value}`;
    case "snapshot":
      return `${Object.keys(mod.headers).length} 个请求头`;
  }
}

function visibleModifications(execution: InterceptorExecution): Modification[] {
  return execution.modifications.filter((mod) => mod.kind !== "snapshot");
}

function openExecution(execution: InterceptorExecution): void {
  if (!detail.value) return;
  openScriptSnapshot(detail.value.session_id, detail.value.id, execution);
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

      <section v-if="breakpoint" class="breakpoint-panel">
        <div class="breakpoint-actions">
          <span class="breakpoint-state">
            断点等待中 · {{ breakpoint.interceptor_name }} · 剩余 {{ remainingLabel() }}
          </span>
          <span class="summary-spacer" />
          <input
            v-model.number="extensionSeconds"
            class="input extension-input mono"
            type="number"
            min="0"
            step="1"
            aria-label="延长秒数"
          />
          <span class="extension-unit">秒</span>
          <button class="btn" :disabled="extending" @click="extendBreakpoint">
            {{ extending ? "延长中…" : "延长" }}
          </button>
          <button class="btn" @click="scriptPanelOpen = !scriptPanelOpen">
            {{ scriptPanelOpen ? "收起临时脚本" : "执行临时脚本" }}
          </button>
          <button class="btn primary" :disabled="releasing" @click="releaseBreakpoint">
            {{ releasing ? "放行中…" : "放行" }}
          </button>
        </div>
        <div v-if="scriptPanelOpen" class="temporary-script">
          <div class="temporary-editor">
            <MonacoEditor v-model="temporaryScript" language="lua" />
          </div>
          <div class="temporary-footer">
            <span>能力与当前阶段一致；临时脚本中不可再次调用 breakpoint()</span>
            <button
              class="btn primary"
              :disabled="executingScript"
              @click="executeTemporaryScript"
            >
              {{ executingScript ? "执行中…" : "应用修改（保持断点）" }}
            </button>
          </div>
        </div>
      </section>

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

      <!-- 拦截器执行记录 -->
      <div v-show="activeTab === 'modifications'" class="tab-page">
        <div class="pane-top full">
          <section v-if="detail.request_interceptors.length > 0" class="execution-section">
            <div class="execution-section-title">请求拦截器</div>
            <article
              v-for="execution in detail.request_interceptors"
              :key="execution.execution_id"
              class="card execution-card"
            >
              <button class="execution-head" @click="openExecution(execution)">
                <span class="execution-order">#{{ execution.position + 1 }}</span>
                <span class="execution-name mono">{{ execution.name }}</span>
                <span v-if="execution.origin === 'temporary'" class="execution-origin">临时</span>
                <span v-else-if="!execution.completed" class="execution-origin waiting">暂停中</span>
                <span class="execution-hash mono">{{ execution.script_hash.slice(0, 12) }}</span>
                <span class="execution-open">查看历史脚本</span>
              </button>
              <div v-if="execution.error" class="execution-error mono">
                {{ execution.error }}
              </div>
              <div
                v-if="visibleModifications(execution).length === 0"
                class="execution-no-change"
              >
                已执行，未产生修改
              </div>
              <ul v-else class="mod-list">
                <li
                  v-for="(mod, i) in visibleModifications(execution)"
                  :key="i"
                  class="mod-item"
                >
                  <span class="mod-badge">{{ modificationLabel(mod) }}</span>
                  <span class="mod-detail mono">{{ modificationDetail(mod) }}</span>
                </li>
              </ul>
            </article>
          </section>
          <section v-if="detail.response_interceptors.length > 0" class="execution-section">
            <div class="execution-section-title">响应拦截器</div>
            <article
              v-for="execution in detail.response_interceptors"
              :key="execution.execution_id"
              class="card execution-card"
            >
              <button class="execution-head" @click="openExecution(execution)">
                <span class="execution-order">#{{ execution.position + 1 }}</span>
                <span class="execution-name mono">{{ execution.name }}</span>
                <span v-if="execution.origin === 'temporary'" class="execution-origin">临时</span>
                <span v-else-if="!execution.completed" class="execution-origin waiting">暂停中</span>
                <span class="execution-hash mono">{{ execution.script_hash.slice(0, 12) }}</span>
                <span class="execution-open">查看历史脚本</span>
              </button>
              <div v-if="execution.error" class="execution-error mono">
                {{ execution.error }}
              </div>
              <div
                v-if="visibleModifications(execution).length === 0"
                class="execution-no-change"
              >
                已执行，未产生修改
              </div>
              <ul v-else class="mod-list">
                <li
                  v-for="(mod, i) in visibleModifications(execution)"
                  :key="i"
                  class="mod-item"
                >
                  <span class="mod-badge">{{ modificationLabel(mod) }}</span>
                  <span class="mod-detail mono">{{ modificationDetail(mod) }}</span>
                </li>
              </ul>
            </article>
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

.breakpoint-panel {
  flex: none;
  border-bottom: 1px solid color-mix(in srgb, var(--warning) 45%, var(--border));
  background: color-mix(in srgb, var(--warning) 8%, var(--bg-panel));
}
.breakpoint-actions {
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 8px 12px;
}
.breakpoint-state {
  color: var(--warning);
  font-size: 11px;
  font-weight: 600;
}
.extension-input { width: 68px; padding: 4px 7px; }
.extension-unit, .temporary-footer { color: var(--text-faint); font-size: 10px; }
.temporary-script { height: 210px; display: flex; flex-direction: column; border-top: 1px solid var(--border); background: var(--bg-panel); }
.temporary-editor { flex: 1; min-height: 0; display: flex; }
.temporary-footer { flex: none; display: flex; align-items: center; gap: 8px; padding: 6px 8px; border-top: 1px solid var(--border); }
.temporary-footer span { flex: 1; }

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

/* ---------- 拦截器执行记录 ---------- */

.execution-section {
  display: flex;
  flex-direction: column;
  gap: 7px;
}
.execution-section-title {
  color: var(--text-secondary);
  font-size: 11px;
  font-weight: 600;
}
.execution-card {
  overflow: visible;
}
.execution-head {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 9px;
  border: 0;
  border-bottom: 1px solid var(--border);
  border-radius: var(--radius-md) var(--radius-md) 0 0;
  background: var(--bg-app);
  color: var(--text);
  cursor: pointer;
  text-align: left;
}
.execution-head:hover {
  background: var(--bg-hover);
}
.execution-order {
  color: var(--accent);
  font-size: 10px;
  font-weight: 700;
}
.execution-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 600;
}
.execution-hash {
  flex: none;
  color: var(--text-faint);
  font-size: 10px;
}
.execution-origin {
  flex: none;
  border-radius: 7px;
  padding: 1px 6px;
  background: color-mix(in srgb, var(--accent) 12%, transparent);
  color: var(--accent);
  font-size: 9px;
}
.execution-origin.waiting {
  background: color-mix(in srgb, var(--warning) 12%, transparent);
  color: var(--warning);
}
.execution-open {
  margin-left: auto;
  color: var(--accent);
  font-size: 10px;
}
.execution-error {
  padding: 7px 10px;
  border-bottom: 1px solid color-mix(in srgb, var(--danger) 28%, var(--border));
  background: color-mix(in srgb, var(--danger) 7%, transparent);
  color: var(--danger);
  font-size: 10px;
  word-break: break-all;
}
.execution-no-change {
  padding: 9px 10px;
  color: var(--text-faint);
  font-size: 11px;
}

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
