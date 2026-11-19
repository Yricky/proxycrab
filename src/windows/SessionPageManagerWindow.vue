<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { Io5DocumentOutline, Io5Download, Io5TrashOutline } from "vue-icons-plus/io5";
import { useBackend } from "../api";
import { selectFile } from "../api/select-file";
import { BackendError } from "../api/backend-error";
import { saveBytesFile } from "../api/save-file";
import type { SessionPage } from "../api/generated/SessionPage";
import type { HttpApiChange } from "../api/generated/HttpApiChange";
import { resourceKinds, useBackendChanges } from "../utils/api-changes";
import { appStore, reportError } from "../stores/app";
import { confirmDialog } from "../stores/dialog";
import { formatBytes } from "../utils/format";
import { t } from "../i18n";

const backend = useBackend();
const pages = ref<SessionPage[]>([]);
const selected = ref<string | null>(null);
const current = computed(() => pages.value.find((page) => page.id === selected.value));
const busy = ref(false);
const loading = ref(false);
let disposed = false;

async function refresh(): Promise<void> {
  if (disposed) return;
  loading.value = true;
  try {
    const nextPages = await backend.listSessionPages!();
    if (disposed) return;
    pages.value = nextPages;
    if (selected.value && !pages.value.some((p) => p.id === selected.value)) selected.value = null;
    if (!selected.value) selected.value = pages.value[0]?.id ?? null;
  } catch (e) {
    reportError(e, t("加载会话页面失败"));
  } finally {
    loading.value = false;
  }
}
async function importPage(): Promise<void> {
  const file = await selectFile(".zip,application/zip");
  if (!file) return;
  if (file.size > 128 * 1024 * 1024) {
    reportError(t("分享包超过 128 MiB"));
    return;
  }
  busy.value = true;
  try {
    let result: SessionPage;
    try {
      result = await backend.importSessionPage!(file);
    } catch (e) {
      if (!(e instanceof BackendError) || e.code !== "conflict") throw e;
      const replace = await confirmDialog({
        title: t("导入会话页面"),
        message: t("同 ID 的会话页面已存在，是否替换整个页面包？已分享的链接将使用新版本。"),
        confirmText: t("替换"),
        danger: true,
      });
      if (!replace) return;
      result = await backend.importSessionPage!(file, true);
    }
    await refresh();
    selected.value = result.id;
    appStore.toast(t("会话页面已导入"), "success");
  } catch (e) {
    reportError(e, t("导入会话页面失败"));
  } finally {
    busy.value = false;
  }
}
async function exportPage(): Promise<void> {
  if (!current.value) return;
  busy.value = true;
  try {
    await saveBytesFile(
      backend,
      `${current.value.id}.zip`,
      await backend.exportSessionPage!(current.value.id),
    );
  } catch (e) {
    reportError(e, t("导出会话页面失败"));
  } finally {
    busy.value = false;
  }
}
async function deletePage(): Promise<void> {
  if (!current.value) return;
  const ok = await confirmDialog({
    title: t("删除会话页面"),
    message: t("删除此会话页面？已分享的页面将失效。"),
    confirmText: t("删除"),
    danger: true,
  });
  if (!ok) return;
  busy.value = true;
  try {
    await backend.deleteSessionPage!(current.value.id);
    await refresh();
  } catch (e) {
    reportError(e, t("删除会话页面失败"));
  } finally {
    busy.value = false;
  }
}
function changed(change: HttpApiChange): void {
  const resources = resourceKinds(change);
  if (resources.includes("all") || resources.includes("session_pages")) void refresh();
}
useBackendChanges(backend, changed);

onMounted(() => {
  void refresh();
});
onBeforeUnmount(() => {
  disposed = true;
});
</script>

<template>
  <main class="pages-manager">
    <aside class="pages-list">
      <header>
        <strong>{{ t("会话页面") }}</strong>
        <span class="muted mono">{{ pages.length }}</span>
        <span class="spacer" />
        <button v-if="backend.importSessionPage" class="btn" :disabled="busy" @click="importPage">
          {{ t("导入") }}
        </button>
      </header>
      <div class="page-items">
        <button
          v-for="page in pages"
          :key="page.id"
          class="page-item"
          :class="{ selected: selected === page.id }"
          @click="selected = page.id"
        >
          <span class="page-name">{{ page.name }}</span>
          <span class="page-size mono">{{ formatBytes(page.size) }}</span>
          <span class="page-id mono">{{ page.id }}</span>
        </button>
        <p v-if="!pages.length" class="empty-hint">
          {{ loading ? t("正在加载…") : t("导入 ZIP 页面包以开始使用") }}
        </p>
      </div>
    </aside>
    <section v-if="current" class="page-content">
      <header class="page-heading">
        <div class="page-title">
          <strong>{{ current.name }}</strong>
          <span class="page-title-id mono">{{ current.id }}</span>
        </div>
        <span class="spacer" />
        <button v-if="backend.exportSessionPage" class="btn" :disabled="busy" @click="exportPage">
          <Io5Download :size="14" />{{ t("导出") }}
        </button>
        <button
          v-if="backend.deleteSessionPage"
          class="btn danger"
          :disabled="busy"
          @click="deletePage"
        >
          <Io5TrashOutline :size="14" />{{ t("删除") }}
        </button>
      </header>
      <div class="page-body">
        <dl class="page-details">
          <dt>ID</dt>
          <dd class="mono">{{ current.id }}</dd>
          <dt>{{ t("大小") }}</dt>
          <dd>{{ formatBytes(current.size) }}</dd>
          <dt>{{ t("导出文件") }}</dt>
          <dd class="mono">{{ current.id }}.zip</dd>
          <dt>{{ t("描述") }}</dt>
          <dd>{{ current.description || "—" }}</dd>
          <dt>{{ t("包版本") }}</dt>
          <dd class="mono">{{ current.version }}</dd>
          <dt>{{ t("运行时版本") }}</dt>
          <dd class="mono">{{ current.backend_version }}</dd>
        </dl>
      </div>
    </section>
    <div v-else class="page-empty">
      <Io5DocumentOutline :size="22" class="page-empty-icon" />
      <strong>{{ t("还没有会话页面") }}</strong>
    </div>
  </main>
</template>

<style scoped>
.pages-manager {
  display: flex;
  flex: 1;
  min-height: 0;
  font-size: 12px;
}
.pages-list {
  width: 236px;
  flex: none;
  display: flex;
  flex-direction: column;
  border-right: 1px solid var(--border);
}
.pages-list header,
.page-heading {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 34px;
  padding: 4px 8px 4px 12px;
  border-bottom: 1px solid var(--border);
}
.pages-list header strong {
  font-size: 12px;
  font-weight: 600;
}
.spacer {
  flex: 1;
}
.muted {
  color: var(--text-faint);
}
.page-items {
  flex: 1;
  min-height: 0;
  overflow: auto;
}
.page-item {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: center;
  gap: 0 8px;
  width: 100%;
  padding: 5px 12px;
  border: 0;
  border-bottom: 1px solid var(--border);
  background: transparent;
  color: var(--text);
  text-align: left;
  font: inherit;
  cursor: pointer;
}
.page-item:hover {
  background: var(--bg-hover);
}
.page-item.selected {
  background: var(--bg-selected);
}
.page-item.selected .page-name {
  color: var(--accent);
}
.page-name,
.page-size,
.page-id {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.page-size,
.page-id {
  color: var(--text-faint);
  font-size: 11px;
}
.page-id {
  grid-column: 1 / -1;
  line-height: 1.4;
}
.page-content {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
}
.page-title {
  display: flex;
  align-items: baseline;
  gap: 7px;
  min-width: 0;
}
.page-title strong,
.page-title-id {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.page-title strong {
  font-size: 12px;
  font-weight: 600;
}
.page-title-id {
  color: var(--text-faint);
  font-size: 11px;
}
.page-body {
  flex: 1;
  min-height: 0;
  overflow: auto;
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 10px 12px 12px;
}
.page-details {
  display: grid;
  grid-template-columns: 72px minmax(0, 1fr);
  gap: 4px 12px;
  margin: 0;
}
.page-details dt {
  color: var(--text-secondary);
}
.page-details dd {
  margin: 0;
  overflow-wrap: anywhere;
  user-select: text;
}
.page-empty {
  flex: 1;
  display: grid;
  place-content: center;
  justify-items: center;
  gap: 6px;
  padding: 16px;
  color: var(--text-faint);
  text-align: center;
}
.page-empty-icon {
  opacity: 0.5;
}
.page-empty strong {
  color: var(--text-secondary);
  font-weight: 600;
}
.danger {
  color: var(--danger);
}
</style>
