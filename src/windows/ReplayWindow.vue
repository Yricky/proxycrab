<script setup lang="ts">
import { reportError } from "../stores/app";
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { Io5ChevronDown, Io5Warning } from "vue-icons-plus/io5";
import { useBackend } from "../api";
import { t } from "../i18n";
import type { AssetMetadata } from "../api/generated/AssetMetadata";
import { proxyStore } from "../stores/proxy";
import { sessionsStore } from "../stores/sessions";
import CustomSelect from "../components/CustomSelect.vue";
import MonacoEditor from "../components/MonacoEditor.vue";
import EditableKvTable from "../components/EditableKvTable.vue";
import UrlInput from "../components/UrlInput.vue";
import AssetTreePicker from "../components/AssetTreePicker.vue";
import SessionListItem from "../components/SessionListItem.vue";
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
import { BsFillSendFill } from "vue-icons-plus/bs";

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

const methodOptions = REPLAY_METHODS.map((value) => ({ value, label: value }));
const methodDisplay = computed(() =>
  methodChoice.value === CUSTOM_METHOD ? customMethod.value || t("自定义…") : undefined,
);
const method = computed(() =>
  methodChoice.value === CUSTOM_METHOD
    ? customMethod.value.trim().toUpperCase()
    : methodChoice.value,
);

// ---------- 会话 ----------
// 会话列表统一走 sessionsStore，样式复用 SessionListItem（与侧边栏一致）。
const selectedSessionId = ref<number | null>(sessionsStore.activeSessionId);
const selectedSession = computed(
  () => sessionsStore.sessions.find((session) => session.id === selectedSessionId.value) ?? null,
);
const pickerOpen = ref(false);
const sessionSeg = ref<HTMLElement | null>(null);
const warnOpen = ref(false);
const warnWrap = ref<HTMLElement | null>(null);
const bodyWarnOpen = ref(false);
const bodyWarnWrap = ref<HTMLElement | null>(null);

function closePicker(): void {
  pickerOpen.value = false;
}

function togglePicker(): void {
  pickerOpen.value = !pickerOpen.value;
}

function chooseSession(sessionId: number): void {
  selectedSessionId.value = sessionId;
  closePicker();
}

// 会话列表可能在窗口打开后才就绪，未选中时补上当前活跃会话。
watch(
  () => sessionsStore.sessions.length,
  () => {
    if (selectedSession.value === null && sessionsStore.activeSessionId !== null)
      selectedSessionId.value = sessionsStore.activeSessionId;
  },
);

function onDocumentPointerDown(event: PointerEvent): void {
  const target = event.target as Node;
  if (!sessionSeg.value?.contains(target)) closePicker();
  if (!warnWrap.value?.contains(target)) warnOpen.value = false;
  if (!bodyWarnWrap.value?.contains(target)) bodyWarnOpen.value = false;
}

watch([pickerOpen, warnOpen, bodyWarnOpen], ([picker, warn, bodyWarn]) => {
  if (picker || warn || bodyWarn) document.addEventListener("pointerdown", onDocumentPointerDown);
  else document.removeEventListener("pointerdown", onDocumentPointerDown);
});
onBeforeUnmount(() => document.removeEventListener("pointerdown", onDocumentPointerDown));

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
async function onAssetUploaded(asset: AssetMetadata): Promise<void> {
  assets.value = [...assets.value.filter((item) => item.id !== asset.id), asset];
  try {
    assets.value = await backend.listAssets();
  } catch (error) {
    reportError(error, t("资源已上传，刷新列表失败"));
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
  if (bodyMode.value === "text") return new TextEncoder().encode(bodyText.value).length;
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
    (bodyMode.value === "body_ref" && bodyRefSession.value !== null && bodyRefLog.value !== null),
);
const lengthIssue = computed(() => contentLengthIssue(tuples.value, knownBodySize.value));
const missingContentType = computed(
  () => hasBody.value && headerValue(tuples.value, "content-type") === null,
);
type HeaderWarning = { key: string; hint: string; action: string; fix: () => void };
// GET / HEAD 按协议不应携带请求体：提示在请求体标题栏。
const bodylessMethod = computed(() =>
  (method.value === "GET" || method.value === "HEAD") && hasBody.value ? method.value : null,
);
const bodyWarnings = computed<HeaderWarning[]>(() =>
  bodylessMethod.value
    ? [
        {
          key: bodylessMethod.value,
          hint: t("{method} 请求不应携带请求体", { method: bodylessMethod.value }),
          action: t("清空 body"),
          fix: () => {
            bodyMode.value = "none";
          },
        },
      ]
    : [],
);
watch(bodyWarnings, (warnings) => {
  if (warnings.length === 0) bodyWarnOpen.value = false;
});
// 按 header key 维度聚合：一个 key 最多一条提示。
const headerWarnings = computed<HeaderWarning[]>(() => {
  const warnings: HeaderWarning[] = [];
  if (lengthIssue.value === "missing")
    warnings.push({
      key: "Content-Length",
      hint: t("存在 body 但未声明长度"),
      action: t("自动补上"),
      fix: fixContentLength,
    });
  else if (lengthIssue.value === "mismatch")
    warnings.push({
      key: "Content-Length",
      hint: t("声明值与实际 body 大小不符"),
      action: t("修正为 {size}", { size: knownBodySize.value }),
      fix: fixContentLength,
    });
  if (missingContentType.value)
    warnings.push({
      key: "Content-Type",
      hint: t("存在 body 但未声明类型"),
      action: t("按内容补齐"),
      fix: fixContentType,
    });
  return warnings;
});
watch(headerWarnings, (warnings) => {
  if (warnings.length === 0) warnOpen.value = false;
});
function applyWarning(warning: HeaderWarning): void {
  warning.fix();
  warnOpen.value = false;
  bodyWarnOpen.value = false;
}

function upsertHeader(name: string, value: string): void {
  const committed = rows.value.filter((row) => row.name !== "" || row.value !== "");
  const found = committed.find((row) => row.name.toLowerCase() === name.toLowerCase());
  if (found) found.value = value;
  else committed.push({ name, value });
  rows.value = committed;
}
function fixContentLength(): void {
  if (knownBodySize.value !== null) upsertHeader("content-length", String(knownBodySize.value));
}
function fixContentType(): void {
  if (bodyMode.value === "text") {
    upsertHeader("content-type", guessContentType(bodyText.value));
  } else if (bodyMode.value === "asset" && assetId.value) {
    const meta = assets.value.find((item) => item.id === assetId.value);
    upsertHeader("content-type", meta?.content_type || "application/octet-stream");
  } else {
    upsertHeader("content-type", "application/octet-stream");
  }
}

// ---------- 执行 ----------
const executing = ref(false);
const MAX_SEND_RECORDS = 20;
type SendRecord = {
  key: number;
  logId: number | null;
  sessionId: number;
  sessionName: string;
  method: string;
  url: string;
  at: string;
  error: string | null;
};
// 发送记录只存在于本窗口生命周期内，新记录排在最右，超出条数从左侧丢弃。
const sendRecords = ref<SendRecord[]>([]);
let recordKey = 0;

function pushRecord(record: Omit<SendRecord, "key">): void {
  recordKey += 1;
  sendRecords.value = [...sendRecords.value, { ...record, key: recordKey }].slice(
    -MAX_SEND_RECORDS,
  );
}

function recordTitle(record: SendRecord): string {
  const base = `${record.method} ${record.url}\n${record.sessionName} · ${record.at}`;
  return record.error ? `${base}\n${record.error}` : base;
}

function openRecord(record: SendRecord): void {
  if (record.logId !== null) openLogDetail(record.sessionId, record.logId);
}

function buildBody(): ReplayBodySpec | undefined {
  if (bodyMode.value === "text" && bodyText.value.length > 0)
    return { type: "text", text: bodyText.value, charset: "utf8" };
  if (bodyMode.value === "body_ref" && bodyRefSession.value !== null && bodyRefLog.value !== null)
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

const canCompose = computed(() => method.value !== "" && /^https?:\/\//.test(url.value));
const sendDisabled = computed(
  () =>
    executing.value || !canCompose.value || !proxyStore.running || selectedSession.value === null,
);
const sendTitle = computed(() =>
  executing.value
    ? t("发送中…")
    : !proxyStore.running
      ? t("代理未启动，无法发送")
      : !canCompose.value
        ? t("请填写合法的请求方法与 URL")
        : sessionsStore.sessions.length === 0
          ? t("没有可用的会话")
          : !selectedSession.value
            ? t("请选择会话")
            : t("发送到 {name}", { name: selectedSession.value.name }),
);

async function execute(): Promise<void> {
  const replay = backend.replay;
  if (executing.value || !replay || !selectedSession.value) return;
  closePicker();
  executing.value = true;
  const record: Omit<SendRecord, "key"> = {
    logId: null,
    sessionId: selectedSession.value.id,
    sessionName: selectedSession.value.name,
    method: method.value,
    url: url.value,
    at: new Date().toLocaleTimeString("zh-CN", { hour12: false }),
    error: null,
  };
  try {
    const result = await replay(record.sessionId, {
      method: method.value,
      url: url.value,
      headers: tuples.value,
      body: buildBody(),
    });
    record.logId = result.log_id;
  } catch (error) {
    record.error = error instanceof Error ? error.message : String(error);
  } finally {
    executing.value = false;
    pushRecord(record);
  }
}

const bodyModeLabels: Record<BodyMode, string> = {
  none: t("无"),
  text: t("文本"),
  body_ref: t("引用日志"),
  asset: t("资源"),
};
const bodyModes: BodyMode[] = ["none", "text", "body_ref", "asset"];
const bodyExtra = computed(() => {
  if (bodyMode.value === "asset" && assetId.value) return assetId.value;
  if (bodyMode.value === "body_ref" && bodyRefSession.value !== null && bodyRefLog.value !== null)
    return t("#{session}/#{log} {side} body", {
      session: bodyRefSession.value,
      log: bodyRefLog.value,
      side: bodyRefSide.value === "request" ? t("请求") : t("响应"),
    });
  if (bodyMode.value === "text" && knownBodySize.value !== null) return `${knownBodySize.value} B`;
  return null;
});
</script>

<template>
  <div class="replay-root">
    <header class="summary">
      <UrlInput v-model="url" class="url" />
    </header>

    <div class="send-row">
      <div class="record-strip">
        <template v-for="record in sendRecords" :key="record.key">
          <button
            v-if="record.logId !== null"
            type="button"
            class="record-chip"
            :title="recordTitle(record)"
            @click="openRecord(record)"
          >
            #{{ record.logId }}
          </button>
          <span v-else class="record-chip failed" :title="recordTitle(record)">{{
            t("失败")
          }}</span>
        </template>
      </div>
      <div class="send-group" @keydown.esc="closePicker">
        <CustomSelect
          class="method-select"
          :model-value="methodChoice"
          :options="methodOptions"
          :display-value="methodDisplay"
          :aria-label="t('请求方法')"
          @update:model-value="methodChoice = $event"
        >
          <template #footer="{ close }">
            <input
              v-model="customMethod"
              class="input method-custom-input mono"
              :placeholder="t('自定义 Method')"
              :aria-label="t('自定义请求方法')"
              spellcheck="false"
              @focus="methodChoice = CUSTOM_METHOD"
              @input="methodChoice = CUSTOM_METHOD"
              @keydown.enter.prevent="close(true)"
            />
          </template>
        </CustomSelect>
        <div ref="sessionSeg" class="session-seg">
          <button
            type="button"
            class="session-trigger"
            :class="{ open: pickerOpen }"
            :disabled="sessionsStore.sessions.length === 0"
            :title="
              selectedSession ? t('发送到 {name}', { name: selectedSession.name }) : t('选择会话')
            "
            :aria-expanded="pickerOpen"
            :aria-label="t('选择会话')"
            @click="togglePicker"
          >
            <span class="session-label" :class="{ placeholder: !selectedSession }">{{
              selectedSession?.name ?? t("选择会话")
            }}</span>
            <Io5ChevronDown :size="13" />
          </button>
          <div v-if="pickerOpen" class="session-picker" role="menu" :aria-label="t('选择会话')">
            <SessionListItem
              v-for="session in sessionsStore.sessions"
              :key="session.id"
              :session="session"
              :viewing="selectedSessionId === session.id"
              :active="sessionsStore.activeSessionId === session.id"
              :active-connections="proxyStore.activeNetlogCount(session.id)"
              class="picker-row"
              role="menuitem"
              tabindex="0"
              @click="chooseSession(session.id)"
              @keydown.enter.prevent="chooseSession(session.id)"
              @keydown.space.prevent="chooseSession(session.id)"
            />
          </div>
        </div>
        <button
          type="button"
          class="send-btn"
          :class="{ sending: executing }"
          :disabled="sendDisabled"
          :title="sendTitle"
          :aria-label="t('发送请求')"
          :aria-busy="executing"
          @click="execute"
        >
          <BsFillSendFill :size="13" />
        </button>
      </div>
    </div>

    <div class="tab-page">
      <section class="card headers-card">
        <div class="card-title">
          {{ t("请求头") }}
          <div
            v-if="headerWarnings.length > 0"
            ref="warnWrap"
            class="warn-wrap"
            @keydown.esc="warnOpen = false"
          >
            <button
              class="warn-trigger"
              :class="{ active: warnOpen }"
              :title="t('请求头有 {n} 项待补齐', headerWarnings.length)"
              :aria-label="t('请求头警告')"
              :aria-expanded="warnOpen"
              @click="warnOpen = !warnOpen"
            >
              <Io5Warning :size="12" />
            </button>
            <div v-if="warnOpen" class="warn-menu" role="menu">
              <div v-for="warning in headerWarnings" :key="warning.key" class="warn-item">
                <div class="warn-item-head">
                  <span class="warn-key mono">{{ warning.key }}</span>
                  <button class="btn notice-btn" @click="applyWarning(warning)">
                    {{ warning.action }}
                  </button>
                </div>
                <div class="warn-hint">{{ warning.hint }}</div>
              </div>
            </div>
          </div>
          <span class="count-badge">{{ tuples.length }}</span>
        </div>
        <div class="headers-scroll">
          <EditableKvTable v-model="rows" />
        </div>
      </section>

      <section class="card body-card">
        <div class="card-title">
          {{ t("请求体") }}
          <div
            v-if="bodyWarnings.length > 0"
            ref="bodyWarnWrap"
            class="warn-wrap"
            @keydown.esc="bodyWarnOpen = false"
          >
            <button
              class="warn-trigger"
              :class="{ active: bodyWarnOpen }"
              :title="t('请求体有 {n} 项待处理', bodyWarnings.length)"
              :aria-label="t('请求体警告')"
              :aria-expanded="bodyWarnOpen"
              @click="bodyWarnOpen = !bodyWarnOpen"
            >
              <Io5Warning :size="12" />
            </button>
            <div v-if="bodyWarnOpen" class="warn-menu" role="menu">
              <div v-for="warning in bodyWarnings" :key="warning.key" class="warn-item">
                <div class="warn-item-head">
                  <span class="warn-key mono">{{ warning.key }}</span>
                  <button class="btn notice-btn" @click="applyWarning(warning)">
                    {{ warning.action }}
                  </button>
                </div>
                <div class="warn-hint">{{ warning.hint }}</div>
              </div>
            </div>
          </div>
          <span class="card-title-extra mono">{{ bodyExtra ?? "" }}</span>
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
                { value: 'request', label: t('请求 body') },
                { value: 'response', label: t('响应 body') },
              ]"
              :aria-label="t('Body 方向')"
              @update:model-value="bodyRefSide = $event as 'request' | 'response'"
            />
          </div>
          <template v-else-if="bodyMode === 'asset'">
            <div v-if="assetsError" class="empty-hint text-danger">
              {{ t("资源加载失败：{error}") }}{{ assetsError }}
            </div>
            <AssetTreePicker
              v-else
              v-model:selected="assetId"
              :assets="assets"
              @uploaded="onAssetUploaded"
              class="body-assets"
            />
          </template>
          <div v-else class="empty-hint">{{ t("无 Body") }}</div>
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

.notice-btn {
  padding: 0 6px;
  font-size: 11px;
  line-height: 18px;
}

/* 发送行：左侧为发送记录（右对齐），右侧为方法 / 会话 / 发送三合一控件 */
.send-row {
  flex: none;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 8px;
}

.record-strip {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 4px;
  overflow: hidden;
}

.record-chip {
  flex: none;
  height: 18px;
  padding: 0 7px;
  border: 1px solid var(--border);
  border-radius: 9px;
  background: var(--bg-panel);
  color: var(--text-secondary);
  font: inherit;
  font-size: 11px;
  line-height: 16px;
  cursor: pointer;
}

.record-chip:hover {
  border-color: var(--accent);
  color: var(--accent);
}

.record-chip.failed {
  border-color: var(--danger);
  color: var(--danger);
  cursor: default;
}

.send-group {
  position: relative;
  flex: none;
  display: flex;
  align-items: stretch;
  height: 26px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--bg-panel);
}

.send-group > .method-select {
  width: 82px;
}

.send-group > .method-select :deep(.select-trigger) {
  height: 100%;
  min-height: 0;
  padding: 2px 6px 2px 9px;
  border: none;
  border-radius: 5px 0 0 5px;
  background: transparent;
  font-size: 11px;
}

.send-group > .method-select :deep(.select-trigger:hover:not(:disabled)) {
  background: var(--bg-hover);
}

.method-custom-input {
  width: 100%;
  min-width: 0;
  box-sizing: border-box;
  padding: 4px 7px;
  border-radius: var(--radius-sm);
  font-size: 11px;
}

.send-group :deep(button:focus-visible) {
  outline: 1px solid var(--accent);
  outline-offset: -2px;
}

.session-seg {
  display: flex;
  min-width: 120px;
  border-left: 1px solid var(--border);
}

.session-trigger {
  display: flex;
  flex: 1;
  align-items: center;
  gap: 8px;
  max-width: 200px;
  padding: 2px 6px 2px 9px;
  border: none;
  background: transparent;
  color: var(--text);
  font: inherit;
  font-size: 11px;
  text-align: left;
  cursor: pointer;
}

.session-trigger:hover:not(:disabled) {
  background: var(--bg-hover);
}

.session-trigger:disabled {
  opacity: 0.55;
  cursor: default;
}

.session-trigger > svg {
  flex: none;
  color: var(--text-secondary);
  transition: transform 0.12s;
}

.session-trigger.open > svg {
  transform: rotate(180deg);
}

.session-label {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.session-label.placeholder {
  color: var(--text-faint);
}

.send-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  padding: 0;
  border: none;
  border-left: 1px solid var(--border);
  border-radius: 0 5px 5px 0;
  background: transparent;
  color: var(--accent);
  cursor: pointer;
}

.send-btn:hover:not(:disabled) {
  background: var(--bg-hover);
}

.send-btn.sending,
.send-btn:disabled {
  color: var(--text-faint);
  cursor: default;
}

.session-picker {
  position: absolute;
  z-index: 100006;
  top: calc(100% + 4px);
  left: -1px;
  right: -1px;
  max-height: 280px;
  overflow-y: auto;
  padding: 4px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-md);
  background: var(--bg-elevated);
  box-shadow: var(--shadow-popup);
}

.picker-row:focus-visible {
  outline: 1px solid var(--accent);
  outline-offset: -1px;
}

/* 与详情页 tab-page / card 对齐 */
.tab-page {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 0px 8px 8px;
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
  /* 标题栏的警告菜单需要溢出卡片显示；圆角由标题栏自身负责 */
  overflow: visible;
}

.headers-card > .card-title {
  border-radius: var(--radius-md) var(--radius-md) 0 0;
}

.headers-card > .headers-scroll {
  border-radius: 0 0 var(--radius-md) var(--radius-md);
}

.warn-wrap {
  position: relative;
  display: flex;
  flex: none;
}

.warn-trigger {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 18px;
  padding: 0;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--warning);
  cursor: pointer;
}

.warn-trigger:hover,
.warn-trigger.active {
  background: var(--bg-hover);
}

.warn-menu {
  position: absolute;
  z-index: 100004;
  top: calc(100% + 4px);
  left: 0;
  width: 280px;
  padding: 4px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-md);
  background: var(--bg-elevated);
  box-shadow: var(--shadow-popup);
  font-weight: 400;
}

.warn-item {
  padding: 5px 7px;
  border-radius: var(--radius-sm);
}

.warn-item:hover {
  background: var(--bg-hover);
}

.warn-item-head {
  display: flex;
  align-items: center;
  gap: 8px;
}

.warn-key {
  flex: 1;
  font-size: 11px;
  color: var(--text);
}

.warn-hint {
  margin-top: 2px;
  font-size: 11px;
  color: var(--text-secondary);
}

.body-card {
  flex: 1;
  min-height: 0;
  /* 标题栏的警告菜单需要溢出卡片显示；圆角由标题栏与内容区自身负责 */
  overflow: visible;
}

.body-card > .card-title {
  border-radius: var(--radius-md) var(--radius-md) 0 0;
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
  font-size: 11px;
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
  border-radius: 0 0 var(--radius-md) var(--radius-md);
  overflow: hidden;
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
