<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref } from "vue";
import { useBackend } from "../api";
import { t } from "../i18n";
import type { FileWriter } from "../api/save-file";
import type { LogsStore } from "../stores/logs";
import { appendHar, errorMessage, type ExportLog, type ExportProgress } from "../utils/har-export";

const props = defineProps<{ logs: LogsStore }>();
const backend = useBackend();
const running = ref(false);
const status = ref(t("等待导出"));
const progress = ref<ExportProgress>({
  processed: 0,
  total: 0,
  exported: 0,
  skipped: 0,
  warnings: 0,
});
const messages = ref<{ id: number; time: string; level: string; message: string }[]>([]);
const logElement = ref<HTMLElement>();
const percent = computed(() =>
  progress.value.total ? Math.round((progress.value.processed / progress.value.total) * 100) : 0,
);
let sequence = 0;
let controller: AbortController | undefined;
let writer: FileWriter | null = null;
let disposed = false;

const log: ExportLog = (level, message) => {
  messages.value.push({ id: ++sequence, time: new Date().toLocaleTimeString(), level, message });
  if (messages.value.length > 1000) messages.value.splice(0, messages.value.length - 1000);
  void nextTick(() => {
    if (logElement.value) logElement.value.scrollTop = logElement.value.scrollHeight;
  });
};
async function start() {
  if (running.value || props.logs.loading || props.logs.sessionId === null) return;
  const sessionId = props.logs.sessionId;
  const ids = [...props.logs.sortedIds];
  controller = new AbortController();
  const signal = controller.signal;
  running.value = true;
  messages.value = [];
  progress.value = { processed: 0, total: ids.length, exported: 0, skipped: 0, warnings: 0 };
  status.value = t("选择保存位置");
  log("info", t("Session {id}，固定当前列表 {n} 条记录及排序", { id: sessionId, n: ids.length }));
  try {
    writer = await backend.saveFile({ filename: `proxycrab-session-${sessionId}.har` });
    if (!writer) {
      status.value = t("已取消保存");
      log("info", status.value);
      return;
    }
    signal.throwIfAborted();
    status.value = t("正在导出");
    await appendHar(
      backend,
      writer,
      sessionId,
      ids,
      signal,
      (value) => {
        progress.value = value;
      },
      log,
    );
    signal.throwIfAborted();
    status.value = t("正在保存文件");
    await writer.markAsFinish();
    status.value = t("导出完成");
    log(
      "info",
      t("导出完成：{exported} 条，跳过 {skipped} 条，警告 {warnings} 次", {
        exported: progress.value.exported,
        skipped: progress.value.skipped,
        warnings: progress.value.warnings,
      }),
    );
  } catch (error) {
    status.value = signal.aborted ? t("已取消导出") : t("导出失败");
    log(signal.aborted ? "info" : "error", `${status.value}：${errorMessage(error)}`);
    await writer
      ?.markAsError()
      .catch((cleanupError) =>
        log("error", t("清理失败：{error}", { error: errorMessage(cleanupError) })),
      );
  } finally {
    writer = null;
    running.value = false;
    if (disposed) messages.value = [];
  }
}
function cancel() {
  controller?.abort();
  if (running.value) status.value = t("正在取消导出");
  void writer
    ?.markAsError()
    .catch((error) => log("error", t("清理失败：{error}", { error: errorMessage(error) })));
}
onBeforeUnmount(() => {
  disposed = true;
  cancel();
});
</script>

<template>
  <main class="export-root">
    <div class="export-toolbar">
      <span>{{ t("当前列表 · {n} 条", logs.sortedIds.length) }}</span>
      <span class="export-actions">
        <button v-if="running" class="btn" @click="cancel">{{ t("取消导出") }}</button>
        <button
          class="btn primary"
          :disabled="running || logs.loading || logs.sessionId === null"
          @click="start"
        >
          {{ t("导出 HAR") }}
        </button>
      </span>
    </div>
    <p class="export-hint">
      {{ t("按启动时的筛选结果和顺序导出。Body 读取失败将写入失败说明并继续。") }}
    </p>
    <div class="export-status" aria-live="polite">
      <strong>{{ status }}</strong>
      <span>{{
        t("{processed}/{total} · 已写入 {exported} · 跳过 {skipped} · 警告 {warnings}", {
          processed: progress.processed,
          total: progress.total,
          exported: progress.exported,
          skipped: progress.skipped,
          warnings: progress.warnings,
        })
      }}</span>
    </div>
    <progress
      :value="progress.processed"
      :max="progress.total || 1"
      :aria-label="t('导出进度 {percent}%', { percent })"
    />
    <div ref="logElement" class="export-log mono" role="log" :aria-label="t('导出日志')">
      <div v-if="!messages.length" class="export-hint">
        {{ t("点击「导出 HAR」开始，进度和日志将在这里显示。") }}
      </div>
      <div v-for="item in messages" :key="item.id" class="log-row" :class="item.level">
        <time>{{ item.time }}</time
        ><span>{{ item.message }}</span>
      </div>
    </div>
    <div class="export-hint">{{ t("日志保留最近 1000 条；关闭窗口会取消未完成的导出。") }}</div>
  </main>
</template>

<style scoped>
.export-root {
  display: flex;
  flex: 1;
  min-height: 0;
  flex-direction: column;
  gap: 10px;
  padding: 14px;
  font-size: 12px;
}
.export-toolbar,
.export-status,
.export-actions {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}
.export-hint {
  margin: 0;
  color: var(--text-secondary);
  font-size: 11px;
}
.export-status {
  flex-wrap: wrap;
}
.export-status span {
  color: var(--text-secondary);
}
progress {
  width: 100%;
  height: 6px;
  accent-color: var(--accent);
}
.export-log {
  flex: 1;
  min-height: 0;
  overflow: auto;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--bg-app);
  padding: 8px;
}
.log-row {
  display: flex;
  gap: 10px;
  padding: 3px 0;
  overflow-wrap: anywhere;
}
.log-row time {
  flex: none;
  color: var(--text-faint);
}
.warning {
  color: var(--warning);
}
.error {
  color: var(--danger);
}
</style>
