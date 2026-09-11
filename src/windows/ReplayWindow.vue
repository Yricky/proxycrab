<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import {
  Io5ChevronDown,
  Io5OpenOutline,
  Io5Play,
  Io5RadioButtonOn,
  Io5Warning,
} from "vue-icons-plus/io5";
import { useBackend } from "../api";
import type { AssetMetadata, SessionMetadata } from "../api/types";
import { proxyStore } from "../stores/proxy";
import { openDropdownMenu } from "../stores/dialog";
import CustomSelect from "../components/CustomSelect.vue";
import MonacoEditor from "../components/MonacoEditor.vue";
import EditableKvTable from "../components/EditableKvTable.vue";
import UrlInput from "../components/UrlInput.vue";
import AssetTreePicker from "../components/AssetTreePicker.vue";
import {
  REPLAY_METHODS,
  contentLengthIssue,
  guessContentType,
  headerValue,
  kvRowsToTuples,
  type KvRow,
  type ReplayBodySpec,
  type ReplayDraft,
} from "../utils/replay";
import { openLogDetail } from "./launcher";

const props = defineProps<{ prefill?: ReplayDraft }>();
const backend = useBackend();

// ---------- 表单状态 ----------
const CUSTOM_METHOD = "__custom";
const methodChoice = ref<string>("GET");
const customMethod = ref("");
const url = ref("");
const rows = ref<KvRow[]>([]);
type BodyMode = "none" | "text" | "body_ref" | "asset";
const bodyMode = ref<BodyMode>("none");
const bodyText = ref("");
const bodyRefSession = ref<number | null>(null);
const bodyRefLog = ref<number | null>(null);
const bodyRefSide = ref<"request" | "response">("request");
const assetId = ref<string | null>(null);

if (props.prefill) {
  const p = props.prefill;
  if ((REPLAY_METHODS as readonly string[]).includes(p.method)) {
    methodChoice.value = p.method;
  } else {
    methodChoice.value = CUSTOM_METHOD;
    customMethod.value = p.method;
  }
  url.value = p.url;
  rows.value = p.headers.map(([name, value]) => ({ name, value }));
  if (p.body?.type === "text") {
    bodyMode.value = "text";
    bodyText.value = p.body.text;
  } else if (p.body?.type === "body_ref") {
    bodyMode.value = "body_ref";
    bodyRefSession.value = p.body.session_id;
    bodyRefLog.value = p.body.log_id;
    bodyRefSide.value = p.body.side;
  } else if (p.body?.type === "asset") {
    bodyMode.value = "asset";
    assetId.value = p.body.asset_id;
  }
}

const methodOptions = [
  ...REPLAY_METHODS.map((value) => ({ value, label: value })),
  { value: CUSTOM_METHOD, label: "自定义…" },
];
const method = computed(() =>
  methodChoice.value === CUSTOM_METHOD
    ? customMethod.value.trim().toUpperCase()
    : methodChoice.value,
);

// ---------- 会话 ----------
const sessions = ref<SessionMetadata[]>([]);
const activeSessionId = ref<number | null>(null);
onMounted(async () => {
  const [list, active] = await Promise.all([
    backend.listSessions(),
    backend.getActiveSession(),
  ]);
  sessions.value = list;
  activeSessionId.value =
    active.session_id !== null &&
    list.some((session) => session.id === active.session_id)
      ? active.session_id
      : null;
});

// ---------- 资源 ----------
const assets = ref<AssetMetadata[]>([]);
const assetsError = ref<string | null>(null);
let assetsLoaded = false;
async function ensureAssets(): Promise<void> {
  if (assetsLoaded) return;
  assetsLoaded = true;
  try {
    assets.value = await backend.listAssets();
  } catch (error) {
    assetsError.value = error instanceof Error ? error.message : String(error);
  }
}
function selectBodyMode(mode: BodyMode): void {
  bodyMode.value = mode;
  if (mode === "asset") void ensureAssets();
}
if (bodyMode.value === "asset") void ensureAssets();

// ---------- 警告 ----------
const tuples = computed(() => kvRowsToTuples(rows.value));
const knownBodySize = computed<number | null>(() => {
  if (bodyMode.value === "text")
    return new TextEncoder().encode(bodyText.value).length;
  if (bodyMode.value === "asset" && assetId.value) {
    const meta = assets.value.find((item) => item.id === assetId.value);
    return meta ? meta.size : null;
  }
  return null; // body_ref / none：长度未知或无 body
});
const hasBody = computed(
  () =>
    (bodyMode.value === "text" && bodyText.value.length > 0) ||
    (bodyMode.value === "asset" && assetId.value !== null) ||
    (bodyMode.value === "body_ref" &&
      bodyRefSession.value !== null &&
      bodyRefLog.value !== null),
);
const lengthIssue = computed(() =>
  contentLengthIssue(tuples.value, knownBodySize.value),
);
const missingContentType = computed(
  () => hasBody.value && headerValue(tuples.value, "content-type") === null,
);

function upsertHeader(name: string, value: string): void {
  const committed = rows.value.filter(
    (row) => row.name !== "" || row.value !== "",
  );
  const found = committed.find(
    (row) => row.name.toLowerCase() === name.toLowerCase(),
  );
  if (found) found.value = value;
  else committed.push({ name, value });
  rows.value = committed;
}
function fixContentLength(): void {
  if (knownBodySize.value !== null)
    upsertHeader("content-length", String(knownBodySize.value));
}
function fixContentType(): void {
  if (bodyMode.value === "text") {
    upsertHeader("content-type", guessContentType(bodyText.value));
  } else if (bodyMode.value === "asset" && assetId.value) {
    const meta = assets.value.find((item) => item.id === assetId.value);
    upsertHeader(
      "content-type",
      meta?.content_type || "application/octet-stream",
    );
  } else {
    upsertHeader("content-type", "application/octet-stream");
  }
}

// ---------- 执行 ----------
const executing = ref(false);
const resultLogId = ref<number | null>(null);
const resultSessionId = ref<number | null>(null);
const executeError = ref<string | null>(null);

function buildBody(): ReplayBodySpec | undefined {
  if (bodyMode.value === "text" && bodyText.value.length > 0)
    return { type: "text", text: bodyText.value, charset: "utf8" };
  if (
    bodyMode.value === "body_ref" &&
    bodyRefSession.value !== null &&
    bodyRefLog.value !== null
  )
    return {
      type: "body_ref",
      session_id: bodyRefSession.value,
      log_id: bodyRefLog.value,
      side: bodyRefSide.value,
    };
  if (bodyMode.value === "asset" && assetId.value)
    return { type: "asset", asset_id: assetId.value };
  return undefined;
}

const canCompose = computed(
  () => method.value !== "" && /^https?:\/\//.test(url.value),
);
const sendDisabled = computed(
  () => executing.value || !canCompose.value || !proxyStore.running,
);
const sendTitle = computed(() =>
  !proxyStore.running
    ? "代理未启动，无法发送"
    : !canCompose.value
      ? "请填写合法的请求方法与 URL"
      : "选择会话并发送",
);

function openSendMenu(event: MouseEvent): void {
  if (sendDisabled.value) return;
  const items = sessions.value.map((session) => ({
    label: `#${session.id} ${session.name || "未命名"}`,
    icon: session.id === activeSessionId.value ? Io5RadioButtonOn : undefined,
    action: () => void execute(session.id),
  }));
  openDropdownMenu(event.currentTarget as HTMLElement, items);
}

async function execute(targetSessionId: number): Promise<void> {
  if (executing.value) return;
  executing.value = true;
  resultLogId.value = null;
  resultSessionId.value = null;
  executeError.value = null;
  try {
    const result = await backend.replay(targetSessionId, {
      method: method.value,
      url: url.value,
      headers: tuples.value,
      body: buildBody(),
    });
    resultLogId.value = result.log_id;
    resultSessionId.value = targetSessionId;
  } catch (error) {
    executeError.value = error instanceof Error ? error.message : String(error);
  } finally {
    executing.value = false;
  }
}

function openResult(): void {
  if (resultLogId.value !== null && resultSessionId.value !== null) {
    openLogDetail(resultSessionId.value, resultLogId.value);
  }
}

const bodyModeLabels: Record<BodyMode, string> = {
  none: "无",
  text: "文本",
  body_ref: "引用日志",
  asset: "资源",
};
const bodyModes: BodyMode[] = ["none", "text", "body_ref", "asset"];
const bodyExtra = computed(() => {
  if (bodyMode.value === "asset" && assetId.value) return assetId.value;
  if (
    bodyMode.value === "body_ref" &&
    bodyRefSession.value !== null &&
    bodyRefLog.value !== null
  )
    return `#${bodyRefSession.value}/#${bodyRefLog.value} ${bodyRefSide.value === "request" ? "请求" : "响应"} body`;
  if (bodyMode.value === "text" && knownBodySize.value !== null)
    return `${knownBodySize.value} B`;
  return null;
});
</script>

<template>
  <div class="replay-root">
    <header class="summary">
      <UrlInput v-model="url" class="url" />
      <div
        v-if="lengthIssue || missingContentType || resultLogId !== null || executeError"
        class="notice-line"
      >
        <span v-if="lengthIssue" class="notice-item">
          <Io5Warning :size="12" class="warning-icon" />
          {{
            lengthIssue === "missing"
              ? "缺少 Content-Length"
              : "Content-Length 与实际 body 大小不符"
          }}
          <button class="btn notice-btn" @click="fixContentLength">
            {{
              lengthIssue === "missing" ? "自动补上" : `修正为 ${knownBodySize}`
            }}
          </button>
        </span>
        <span v-if="missingContentType" class="notice-item">
          <Io5Warning :size="12" class="warning-icon" />
          缺少 Content-Type
          <button class="btn notice-btn" @click="fixContentType">
            按内容补齐
          </button>
        </span>
        <span class="summary-spacer" />
        <span v-if="resultLogId !== null" class="notice-item result-ok">
          已发送，日志 #{{ resultLogId }}
          <button class="btn notice-btn" @click="openResult">
            <Io5OpenOutline :size="12" /> 查看详情
          </button>
        </span>
        <span v-else-if="executeError" class="notice-item text-danger">{{
          executeError
        }}</span>
      </div>
    </header>

    <nav class="tab-bar">
      <CustomSelect
        :model-value="methodChoice"
        :options="methodOptions"
        aria-label="请求方法"
        @update:model-value="methodChoice = $event"
      />
      <input
        v-if="methodChoice === CUSTOM_METHOD"
        v-model="customMethod"
        class="input mono custom-method"
        placeholder="METHOD"
        spellcheck="false"
      />
      <button class="tab-btn active">请求</button>
      <span class="summary-spacer" />
      <button
        class="btn primary send-btn"
        :disabled="sendDisabled"
        :title="sendTitle"
        @click="openSendMenu"
      >
        <Io5Play :size="13" />
        {{ executing ? "发送中…" : "发送" }}
        <Io5ChevronDown :size="11" />
      </button>
    </nav>

    <div class="tab-page">
      <section class="card headers-card">
        <div class="card-title">
          请求头
          <span class="count-badge">{{ tuples.length }}</span>
        </div>
        <div class="headers-scroll">
          <EditableKvTable v-model="rows" />
        </div>
      </section>

      <section class="card body-card">
        <div class="card-title">
          请求体
          <span v-if="bodyExtra" class="card-title-extra mono">{{
            bodyExtra
          }}</span>
          <span class="summary-spacer" />
          <span class="mode-tabs">
            <button
              v-for="mode in bodyModes"
              :key="mode"
              class="mode-tab"
              :class="{ active: bodyMode === mode }"
              @click="selectBodyMode(mode)"
            >
              {{ bodyModeLabels[mode] }}
            </button>
          </span>
        </div>
        <div class="body-content">
          <MonacoEditor
            v-if="bodyMode === 'text'"
            v-model="bodyText"
            language="plaintext"
            class="body-editor"
          />
          <div v-else-if="bodyMode === 'body_ref'" class="body-ref-form">
            <input
              v-model.number="bodyRefSession"
              class="input mono body-ref-input"
              type="number"
              min="1"
              placeholder="Session ID"
            />
            <input
              v-model.number="bodyRefLog"
              class="input mono body-ref-input"
              type="number"
              min="1"
              placeholder="Log ID"
            />
            <CustomSelect
              :model-value="bodyRefSide"
              :options="[
                { value: 'request', label: '请求 body' },
                { value: 'response', label: '响应 body' },
              ]"
              aria-label="Body 方向"
              @update:model-value="bodyRefSide = $event as 'request' | 'response'"
            />
          </div>
          <template v-else-if="bodyMode === 'asset'">
            <div v-if="assetsError" class="empty-hint text-danger">
              资源加载失败：{{ assetsError }}
            </div>
            <AssetTreePicker
              v-else
              v-model:selected="assetId"
              :assets="assets"
              class="body-assets"
            />
          </template>
          <div v-else class="empty-hint">无 Body</div>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.replay-root {
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
  background: var(--bg-app);
}

/* 与详情页 summary 头部对齐：URL 独占一行、无外框 */
.summary {
  flex: none;
  padding: 6px 12px;
  background: var(--bg-panel);
}

.summary-spacer {
  flex: 1;
}

.notice-line {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 4px;
  font-size: 11px;
  min-height: 20px;
}

.notice-item {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  color: var(--text-secondary);
}

.notice-btn {
  padding: 0 6px;
  font-size: 11px;
  line-height: 18px;
}

.warning-icon {
  color: var(--warning);
}

/* 与详情页 tab 栏对齐 */
.tab-bar {
  flex: none;
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0 8px;
  border-bottom: 1px solid var(--border);
  border-top: 1px solid var(--border);
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
  cursor: default;
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

.custom-method {
  width: 90px;
}

.send-btn {
  margin-left: auto;
}

/* 与详情页 tab-page / card 对齐 */
.tab-page {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 10px 12px 12px;
  overflow: hidden;
}

.card {
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--bg-panel);
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

.headers-card {
  flex: none;
  max-height: 45%;
}

.body-card {
  flex: 1;
  min-height: 0;
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

.card-title-extra {
  font-weight: 400;
  color: var(--text-faint);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
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

.mode-tabs {
  display: inline-flex;
  gap: 2px;
}

.mode-tab {
  border: none;
  background: transparent;
  color: var(--text-secondary);
  font-size: 11px;
  padding: 1px 8px;
  border-radius: 4px;
  cursor: pointer;
}

.mode-tab:hover {
  color: var(--text);
  background: var(--bg-hover);
}

.mode-tab.active {
  color: var(--accent);
  font-weight: 600;
  background: var(--bg-selected);
}

.body-content {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.body-editor {
  flex: 1;
  min-height: 0;
}

.body-ref-form {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 10px;
}

.body-ref-input {
  width: 110px;
}

.body-assets {
  flex: 1;
  min-height: 0;
}

.empty-hint {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-faint);
  font-size: 12px;
}
</style>
