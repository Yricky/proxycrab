<script setup lang="ts">
import { computed, ref } from "vue";
import {
  Io5AlertCircleOutline,
  Io5ArchiveOutline,
  Io5CheckmarkCircleOutline,
  Io5LayersOutline,
  Io5OpenOutline,
} from "vue-icons-plus/io5";
import { useBackend } from "../api";
import type { BundleFileStatus, BundleImportResult, BundleResolution } from "../api/types";
import { sessionsStore } from "../stores/sessions";
import { HTTP_API_CHANGE_EVENT } from "../stores/http-api-sync";
import { windowsStore } from "../stores/windows";
import { formatBytes } from "../utils/format";
import { parseBundle } from "../utils/bundle";

const props = defineProps<{
  file: File;
  preview: ReturnType<typeof parseBundle>;
  /** 窗口 id：取消、返回与完成都在窗口内自行关闭。 */
  windowId: string;
}>();
const backend = useBackend();
const selected = ref<string[]>([]);
const result = ref<BundleImportResult | null>(null);
const choices = ref<Record<string, BundleResolution>>({});
const busy = ref(false);
const error = ref("");

const MODES = [
  { value: "keep", label: "复用本地", title: "复用本地：保留工作区现有文件，不写入分享包中的版本" },
  { value: "overwrite", label: "覆盖本地", title: "覆盖本地：用分享包中的文件替换工作区现有文件" },
  { value: "rename", label: "另存", title: "另存新名称：脚本另存为新名字，保留工作区现有文件" },
] as const;

const importable = computed(() => props.preview.intents.filter((intent) => intent.type === "session"));
const conflicts = computed(() => result.value?.files.filter((f) => f.status === "conflict") ?? []);
const counts = computed(() => ({
  same: result.value?.files.filter((f) => f.status === "same").length ?? 0,
  added: result.value?.files.filter((f) => f.status === "new").length ?? 0,
}));
const canSubmit = computed(() => {
  if (busy.value) return false;
  if (!result.value?.import_id) return selected.value.length > 0;
  return conflicts.value.every((file) => {
    const choice = choices.value[file.path];
    return choice?.action && (choice.action !== "rename" || choice.name?.trim());
  });
});
const footerHint = computed(() => {
  if (!result.value)
    return importable.value.length
      ? `已选 ${selected.value.length}/${importable.value.length}`
      : "没有可导入的项";
  if (result.value.import_id) return "确认后整批导入，失败时回滚";
  return "导入完成";
});

async function submit(): Promise<void> {
  if (!canSubmit.value) return;
  busy.value = true;
  error.value = "";
  try {
    const next = result.value?.import_id
      ? await backend.resolveBundle!(result.value.import_id, Object.values(choices.value))
      : await backend.importBundle!(props.file, selected.value);
    result.value = next;
    choices.value = {};
    for (const file of next.files.filter((f) => f.status === "conflict"))
      choices.value[file.path] = {
        path: file.path,
        fingerprint: file.fingerprint,
        action: "keep",
      };
    if (!next.import_id) {
      await sessionsStore.refresh();
      window.dispatchEvent(
        new CustomEvent(HTTP_API_CHANGE_EVENT, { detail: { resources: ["all"] } }),
      );
    }
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    busy.value = false;
  }
}

function back(): void {
  result.value = null;
  choices.value = {};
  error.value = "";
}

function modesFor(file: BundleFileStatus): (typeof MODES)[number][] {
  return MODES.filter((mode) => mode.value !== "rename" || file.can_rename);
}

function chooseAll(action: "keep" | "overwrite"): void {
  for (const file of conflicts.value) if (choices.value[file.path]) choices.value[file.path].action = action;
}

function close(): void {
  void windowsStore.close(props.windowId);
}

function openSession(sessionId: number): void {
  window.dispatchEvent(new CustomEvent("proxycrab-open-session", { detail: sessionId }));
  close();
}
</script>

<template>
  <main class="bundle-import">
    <header class="bundle-file">
      <Io5ArchiveOutline :size="18" class="bundle-file-icon" />
      <div class="bundle-file-text">
        <strong :title="file.name">{{ file.name }}</strong>
        <span
          >{{ formatBytes(file.size) }} · {{ preview.intents.length }} 个意图 ·
          {{ importable.length }} 个可导入</span
        >
      </div>
    </header>

    <template v-if="!result">
      <h3 class="section-title intent-heading">选择要导入的内容</h3>
      <label
        v-for="intent in preview.intents"
        :key="intent.id"
        class="intent-row"
        :class="{ unsupported: intent.type !== 'session', selected: selected.includes(intent.id) }"
      >
        <input
          v-model="selected"
          type="checkbox"
          :value="intent.id"
          :disabled="busy || intent.type !== 'session'"
        />
        <Io5LayersOutline :size="15" class="intent-icon" />
        <span class="intent-body">
          <span class="intent-title">
            <strong>{{ intent.name }}</strong>
            <span class="badge">{{ intent.type === "session" ? "Session" : intent.type }}</span>
            <span v-if="intent.type !== 'session'" class="badge muted">当前版本不支持</span>
          </span>
          <span class="intent-meta mono">{{ intent.id }}</span>
          <span v-if="intent.assets.length" class="intent-meta">
            随附资源 <span class="mono">{{ intent.assets.join("、") }}</span>
          </span>
        </span>
      </label>
    </template>

    <template v-else-if="result.import_id">
      <div class="conflict-summary">
        <span
          >相同 <strong>{{ counts.same }}</strong> · 新增 <strong>{{ counts.added }}</strong> · 冲突
          <strong class="text-danger">{{ conflicts.length }}</strong></span
        >
        <span class="conflict-bulk">
          <button class="btn" :disabled="busy" @click="chooseAll('keep')">全部复用</button>
          <button class="btn" :disabled="busy" @click="chooseAll('overwrite')">全部覆盖</button>
        </span>
      </div>
      <div class="conflict-table">
        <div class="conflict-head">
          <span>冲突文件</span>
          <span>处理方式</span>
        </div>
        <template v-for="fileStatus in conflicts" :key="fileStatus.path">
          <div class="conflict-row">
            <span class="conflict-path mono">{{ fileStatus.path }}</span>
            <div class="mode-group" role="group" :aria-label="`${fileStatus.path} 的处理方式`">
              <button
                v-for="mode in modesFor(fileStatus)"
                :key="mode.value"
                type="button"
                class="mode-btn"
                :class="[`m-${mode.value}`, { active: choices[fileStatus.path]?.action === mode.value }]"
                :title="mode.title"
                :aria-pressed="choices[fileStatus.path]?.action === mode.value"
                :disabled="busy"
                @click="choices[fileStatus.path]!.action = mode.value"
              >
                {{ mode.label }}
              </button>
            </div>
          </div>
          <div v-if="choices[fileStatus.path]?.action === 'rename'" class="conflict-name-row">
            <input
              v-model="choices[fileStatus.path]!.name"
              class="input"
              placeholder="新脚本名称（不含 .lua）"
              :disabled="busy"
              :aria-label="`${fileStatus.path} 的新名称`"
            />
          </div>
        </template>
      </div>
    </template>

    <template v-else>
      <div class="import-done">
        <Io5CheckmarkCircleOutline :size="17" />
        <strong>已导入 {{ result.sessions.length }} 个 Session</strong>
      </div>
      <div v-for="session in result.sessions" :key="session.id" class="session-row">
        <Io5LayersOutline :size="15" class="intent-icon" />
        <span class="session-name">{{ session.name }}</span>
        <span class="session-id mono">#{{ session.id }}</span>
        <button class="btn" @click="openSession(session.id)">
          <Io5OpenOutline :size="13" />打开
        </button>
      </div>
      <p class="empty-hint import-note">配置、脚本与随附资源已写入工作区。</p>
    </template>

    <p v-if="error" class="error">
      <Io5AlertCircleOutline :size="14" /><span>{{ error }}</span>
    </p>

    <footer class="bundle-footer">
      <span class="footer-hint">{{ footerHint }}</span>
      <template v-if="!result">
        <button class="btn" :disabled="busy" @click="close">取消</button>
        <button class="btn primary" :disabled="!canSubmit" @click="submit">
          {{ busy ? "正在导入…" : "导入所选" }}
        </button>
      </template>
      <template v-else-if="result.import_id">
        <button class="btn" :disabled="busy" @click="back">返回</button>
        <button class="btn primary" :disabled="!canSubmit" @click="submit">
          {{ busy ? "正在导入…" : "解决冲突并导入" }}
        </button>
      </template>
      <button v-else class="btn primary" @click="close">完成</button>
    </footer>
  </main>
</template>

<style scoped>
.bundle-import {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 12px 14px 0;
  overflow: auto;
  font-size: 13px;
}
.bundle-file {
  flex: none;
  display: flex;
  align-items: center;
  gap: 8px;
}
.bundle-file-icon {
  flex: none;
  color: var(--text-secondary);
}
.bundle-file-text {
  display: flex;
  flex-direction: column;
  min-width: 0;
}
.bundle-file-text strong {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.bundle-file-text span {
  color: var(--text-secondary);
  font-size: 11px;
}
.intent-heading {
  margin: 2px 0 0;
}
.intent-row {
  flex: none;
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 8px 10px;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--bg-panel);
  cursor: pointer;
}
.intent-row:hover {
  border-color: var(--border-strong);
}
.intent-row.selected {
  border-color: var(--accent);
  background: var(--bg-selected);
}
.intent-row.unsupported {
  cursor: default;
  opacity: 0.65;
}
.intent-row input {
  flex: none;
  margin-top: 3px;
}
.intent-icon {
  flex: none;
  margin-top: 1px;
  color: var(--text-secondary);
}
.intent-body {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.intent-title {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}
.intent-title strong {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.badge.muted {
  color: var(--text-faint);
}
.intent-meta {
  color: var(--text-secondary);
  font-size: 11px;
  overflow-wrap: anywhere;
}
.conflict-summary {
  flex: none;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  font-size: 12px;
  color: var(--text-secondary);
}
.conflict-summary strong {
  color: var(--text);
}
.conflict-bulk {
  display: flex;
  gap: 6px;
}
.conflict-bulk .btn {
  padding: 3px 8px;
  font-size: 12px;
}
.conflict-table {
  flex: none;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  overflow: hidden;
}
.conflict-head,
.conflict-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 176px;
  align-items: center;
  gap: 8px;
  padding: 0 10px;
}
.conflict-head > span:last-child {
  padding-left: 4px;
}
.conflict-head {
  height: 26px;
  background: var(--bg-app);
  border-bottom: 1px solid var(--border);
  color: var(--text-faint);
  font-size: 11px;
}
.conflict-row {
  min-height: 32px;
  border-bottom: 1px solid var(--border);
}
.conflict-table > .conflict-row:last-child {
  border-bottom: none;
}
.conflict-path {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
}
.conflict-name-row {
  display: flex;
  justify-content: flex-end;
  padding: 0 10px 8px;
  border-bottom: 1px solid var(--border);
}
.conflict-name-row .input {
  width: 168px;
  font-size: 12px;
}
/* 与「管理接口权限」页一致的胶囊分组 */
.mode-group {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  padding: 2px;
  border: 1px solid var(--border);
  border-radius: 999px;
  background: var(--bg-panel);
}
.mode-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  height: 18px;
  padding: 0 8px;
  border: 0;
  border-radius: 999px;
  background: transparent;
  color: var(--text-faint);
  font: inherit;
  font-size: 11px;
  cursor: pointer;
}
.mode-btn:hover:not(:disabled):not(.active) {
  color: var(--text);
  background: var(--bg-hover);
}
.mode-btn.active.m-keep {
  background: color-mix(in srgb, var(--success) 18%, transparent);
  color: var(--success);
}
.mode-btn.active.m-overwrite {
  background: color-mix(in srgb, var(--danger) 18%, transparent);
  color: var(--danger);
}
.mode-btn.active.m-rename {
  background: color-mix(in srgb, var(--warning) 18%, transparent);
  color: var(--warning);
}
.mode-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}
.import-done {
  flex: none;
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--success);
}
.session-row {
  flex: none;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
}
.session-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.session-id {
  flex: none;
  color: var(--text-faint);
  font-size: 12px;
}
.import-note {
  padding: 0;
  text-align: left;
  font-size: 12px;
}
.error {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  margin: 0;
  color: var(--danger);
  font-size: 12px;
  white-space: pre-wrap;
}
.bundle-footer {
  flex: none;
  position: sticky;
  bottom: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  margin: auto -14px 0;
  padding: 8px 14px;
  border-top: 1px solid var(--border);
  background: var(--bg-elevated);
}
.footer-hint {
  flex: 1;
  color: var(--text-faint);
  font-size: 11px;
}
</style>
