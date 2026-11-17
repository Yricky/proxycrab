<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useBackend } from "../api";
import { selectFile } from "../api/select-file";
import { BackendError } from "../api/backend-error";
import { saveBytesFile } from "../api/save-file";
import type { SessionPage } from "../api/generated/SessionPage";
import type { HttpApiChange } from "../api/generated/HttpApiChange";
import { HTTP_API_CHANGE_EVENT } from "../stores/http-api-sync";
import { appStore, reportError } from "../stores/app";
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
      if (
        !window.confirm(t("同 ID 的会话页面已存在，是否替换整个页面包？已分享的链接将使用新版本。"))
      )
        return;
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
  if (!current.value || !window.confirm(t("删除此会话页面？已分享的页面将失效。"))) return;
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
function changed(event: Event): void {
  const change = (event as CustomEvent<HttpApiChange>).detail;
  if (change.resources.includes("all") || change.resources.includes("session_pages"))
    void refresh();
}
onMounted(() => {
  void refresh();
  window.addEventListener(HTTP_API_CHANGE_EVENT, changed);
});
onBeforeUnmount(() => {
  disposed = true;
  window.removeEventListener(HTTP_API_CHANGE_EVENT, changed);
});
</script>

<template>
  <main class="pages-manager">
    <aside class="pages-list">
      <header>
        <strong
          >{{ t("会话页面") }} <span class="muted">{{ pages.length }}</span></strong
        >
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
          <strong>{{ page.name }}</strong>
          <span class="muted">{{ page.id }} · {{ formatBytes(page.size) }}</span>
        </button>
        <p v-if="!pages.length" class="empty-hint">
          {{ loading ? t("正在加载…") : t("导入 ZIP 页面包以开始使用") }}
        </p>
      </div>
    </aside>
    <section v-if="current" class="page-content">
      <header class="page-heading">
        <div class="page-title">
          <strong>{{ current.name }}</strong
          ><span class="muted">{{ current.description || current.id }}</span>
        </div>
        <button v-if="backend.exportSessionPage" class="btn" :disabled="busy" @click="exportPage">
          {{ t("导出") }}
        </button>
        <button
          v-if="backend.deleteSessionPage"
          class="btn danger"
          :disabled="busy"
          @click="deletePage"
        >
          {{ t("删除") }}
        </button>
      </header>
      <dl class="page-details">
        <dt>ID</dt>
        <dd>{{ current.id }}</dd>
        <dt>{{ t("大小") }}</dt>
        <dd>{{ formatBytes(current.size) }}</dd>
        <dt>{{ t("描述") }}</dt>
        <dd>{{ current.description || "—" }}</dd>
      </dl>
    </section>
    <div v-else class="empty-hint page-empty">{{ t("选择会话页面查看信息") }}</div>
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
  width: 230px;
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
  min-height: 42px;
  padding: 7px 10px;
  border-bottom: 1px solid var(--border);
}
.pages-list header strong {
  flex: 1;
}
.page-items {
  overflow: auto;
}
.page-item {
  display: flex;
  flex-direction: column;
  gap: 4px;
  width: 100%;
  padding: 10px 12px;
  text-align: left;
  color: var(--text);
  background: transparent;
  border: 0;
  border-bottom: 1px solid var(--border);
  cursor: pointer;
}
.page-item:hover,
.page-item.selected {
  background: var(--bg-hover);
}
.page-item.selected {
  box-shadow: inset 3px 0 var(--accent);
}
.page-item strong,
.page-item span {
  overflow-wrap: anywhere;
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
  flex: 1;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
}
.page-title span {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.page-details {
  display: grid;
  grid-template-columns: auto 1fr;
  gap: 10px 16px;
  margin: 16px;
}
.page-details dt {
  color: var(--text-secondary);
}
.page-details dd {
  margin: 0;
  overflow-wrap: anywhere;
}
.page-empty {
  display: grid;
  flex: 1;
  place-content: center;
  padding: 16px;
}
.muted {
  font-size: 11px;
  color: var(--text-secondary);
}
.danger {
  color: var(--danger);
}
</style>
