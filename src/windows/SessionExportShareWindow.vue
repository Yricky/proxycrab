<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";
import { useBackend } from "../api";
import type {
  HarShareScope,
  HarShareState,
  HttpApiChange,
  SessionFilter,
  SessionShareState,
} from "../api/types";
import { appStore, reportError } from "../stores/app";
import { HTTP_API_CHANGE_EVENT } from "../stores/http-api-sync";
import { emptySessionFilter } from "../stores/logs";
import { copyText } from "../utils/clipboard";
import { buildHarShareLinks, buildSessionShareLinks } from "../utils/session-share";

const props = defineProps<{ sessionId: number }>();

const backend = useBackend();
const pageShare = ref<SessionShareState | null>(null);
const harShare = ref<HarShareState | null>(null);
const pageLinks = ref<string[]>([]);
const harLinks = ref<string[]>([]);
const harScope = ref<HarShareScope>("all");
const loading = ref(false);
const pageMutating = ref(false);
const harMutating = ref(false);
const serviceRunning = ref(true);
let loadSequence = 0;

const ID_PAGE_SIZE = 10_000;

function linksForState(
  state: SessionShareState | HarShareState,
  addresses: string[],
  port: number,
  kind: "page" | "har",
): string[] {
  if (!state.token) return [];
  return kind === "page"
    ? buildSessionShareLinks(addresses, port, state.token)
    : buildHarShareLinks(addresses, port, state.token);
}

async function load(): Promise<void> {
  const sequence = ++loadSequence;
  loading.value = true;
  try {
    const [nextPageShare, nextHarShare, status, addresses] = await Promise.all([
      backend.getSessionShare(props.sessionId),
      backend.getHarShare(props.sessionId),
      backend.getHttpServiceStatus(),
      backend.listLocalIps(),
    ]);
    if (sequence !== loadSequence) return;
    pageShare.value = nextPageShare;
    harShare.value = nextHarShare;
    if (nextHarShare.scope) harScope.value = nextHarShare.scope;
    serviceRunning.value = status.running;
    pageLinks.value = linksForState(nextPageShare, addresses, status.port, "page");
    harLinks.value = linksForState(nextHarShare, addresses, status.port, "har");
  } catch (error) {
    if (sequence === loadSequence) reportError(error, "读取分享状态失败");
  } finally {
    if (sequence === loadSequence) loading.value = false;
  }
}

async function togglePageShare(): Promise<void> {
  if (pageMutating.value || !pageShare.value) return;
  pageMutating.value = true;
  try {
    if (pageShare.value.enabled) {
      pageShare.value = await backend.disableSessionShare(props.sessionId);
      pageLinks.value = [];
      appStore.toast("链接分享已关闭", "success");
    } else {
      await backend.enableSessionShare(props.sessionId);
      await load();
      appStore.toast("链接分享已开启", "success");
    }
  } catch (error) {
    reportError(error, pageShare.value.enabled ? "关闭链接分享失败" : "开启链接分享失败");
  } finally {
    pageMutating.value = false;
  }
}

async function collectFrozenIds(filter: SessionFilter): Promise<number[]> {
  const latest = await backend.getLogIds({
    session_id: props.sessionId,
    filter: emptySessionFilter(),
    limit: 1,
  });
  const initialMaxId = latest.matched_ids[0] ?? 0;
  let cursor = initialMaxId > 0 ? initialMaxId + 1 : undefined;
  const ids = new Set<number>();

  while (true) {
    const payload = await backend.getLogIds({
      session_id: props.sessionId,
      filter,
      max_id: cursor,
      limit: ID_PAGE_SIZE,
    });
    for (const id of payload.matched_ids) ids.add(id);
    if (payload.matched_ids.length < ID_PAGE_SIZE) break;
    cursor = Math.min(...payload.matched_ids);
  }

  return [...ids].sort((left, right) => left - right);
}

async function toggleHarShare(): Promise<void> {
  if (harMutating.value || !harShare.value) return;
  harMutating.value = true;
  try {
    if (harShare.value.enabled) {
      harShare.value = await backend.disableHarShare(props.sessionId);
      harLinks.value = [];
      appStore.toast("HAR 分享已关闭", "success");
    } else {
      const filter =
        harScope.value === "filtered"
          ? (await backend.getSessionView(props.sessionId)).filter
          : emptySessionFilter();
      const logIds = await collectFrozenIds(filter);
      await backend.enableHarShare({
        session_id: props.sessionId,
        scope: harScope.value,
        log_ids: logIds,
      });
      await load();
      appStore.toast(`HAR 分享已开启，已固化 ${logIds.length} 条记录`, "success");
    }
  } catch (error) {
    reportError(error, harShare.value.enabled ? "关闭 HAR 分享失败" : "开启 HAR 分享失败");
  } finally {
    harMutating.value = false;
  }
}

async function openLink(url: string, kind: "page" | "har"): Promise<void> {
  try {
    await backend.openExternal(url);
  } catch (error) {
    reportError(error, kind === "har" ? "打开 HAR 分享链接失败" : "打开分享链接失败");
  }
}

async function copyLink(url: string, kind: "page" | "har"): Promise<void> {
  try {
    await copyText(url);
    appStore.toast(kind === "har" ? "HAR 分享链接已复制" : "分享链接已复制", "success");
  } catch (error) {
    reportError(error, kind === "har" ? "复制 HAR 分享链接失败" : "复制分享链接失败");
  }
}

function onRefreshRequest(event: Event): void {
  if ((event as CustomEvent<number>).detail === props.sessionId) void load();
}

function onHttpApiChange(event: Event): void {
  const change = (event as CustomEvent<HttpApiChange>).detail;
  if (
    (change.resources.includes("all") ||
      change.resources.includes("session_share") ||
      change.resources.includes("session_har_share")) &&
    (change.session_id === undefined || change.session_id === props.sessionId)
  ) {
    void load();
  }
}

onMounted(() => {
  void load();
  window.addEventListener("proxycrab-refresh-session-share", onRefreshRequest);
  window.addEventListener(HTTP_API_CHANGE_EVENT, onHttpApiChange);
});

onBeforeUnmount(() => {
  window.removeEventListener("proxycrab-refresh-session-share", onRefreshRequest);
  window.removeEventListener(HTTP_API_CHANGE_EVENT, onHttpApiChange);
});
</script>

<template>
  <main class="share-root">
    <section class="share-card" aria-labelledby="page-share-title">
      <header class="card-heading">
        <div>
          <h3 id="page-share-title">链接分享</h3>
          <p>通过浏览器只读查看此 Session；应用退出或关闭开关后链接失效。</p>
        </div>
        <button
          class="switch"
          :class="{ on: pageShare?.enabled }"
          type="button"
          :disabled="loading || pageMutating || !pageShare"
          :aria-pressed="pageShare?.enabled ?? false"
          :aria-label="pageShare?.enabled ? '关闭链接分享' : '开启链接分享'"
          :title="pageShare?.enabled ? '关闭链接分享' : '开启链接分享'"
          @click="togglePageShare"
        />
      </header>

      <div v-if="loading && !pageShare" class="share-empty">正在读取分享状态…</div>
      <template v-else-if="pageShare?.enabled">
        <div v-if="pageLinks.length" class="share-links">
          <div
            v-for="url in pageLinks"
            :key="url"
            class="share-link-row"
            role="link"
            tabindex="0"
            @click="openLink(url, 'page')"
            @keydown.enter="openLink(url, 'page')"
            @keydown.space.prevent="openLink(url, 'page')"
          >
            <span class="share-link mono">{{ url }}</span>
            <button class="btn" type="button" @click.stop="copyLink(url, 'page')">复制</button>
          </div>
        </div>
        <div v-else class="share-empty">暂无可用的本机 IPv4 地址</div>
        <p v-if="!serviceRunning" class="share-warning">
          管理 HTTP 服务未运行，以上地址当前无法访问。
        </p>
      </template>
      <div v-else class="share-empty">开启后将在这里列出可打开和复制的分享地址。</div>
    </section>

    <section class="share-card" aria-labelledby="har-share-title">
      <header class="card-heading">
        <div>
          <h3 id="har-share-title">HAR 分享</h3>
          <p>生成直接下载 HAR 文件的链接，记录范围在开启分享时固化。</p>
        </div>
        <button
          class="switch"
          :class="{ on: harShare?.enabled }"
          type="button"
          :disabled="loading || harMutating || !harShare"
          :aria-pressed="harShare?.enabled ?? false"
          :aria-label="harShare?.enabled ? '关闭 HAR 分享' : '开启 HAR 分享'"
          :title="harShare?.enabled ? '关闭 HAR 分享' : '开启 HAR 分享'"
          @click="toggleHarShare"
        />
      </header>

      <div class="har-warning" role="note">
        HAR 不会脱敏，可能包含 Authorization、Cookie、请求体、响应体及其他敏感信息，请谨慎分享。
      </div>

      <div v-if="loading && !harShare" class="share-empty">正在读取分享状态…</div>
      <template v-else-if="harShare?.enabled">
        <p class="snapshot-summary">
          已固化 {{ harShare.log_count }} 条记录 ·
          {{ harShare.scope === "filtered" ? "当前筛选结果" : "全部结果" }}
        </p>
        <div v-if="harLinks.length" class="share-links">
          <div
            v-for="url in harLinks"
            :key="url"
            class="share-link-row"
            role="link"
            tabindex="0"
            @click="openLink(url, 'har')"
            @keydown.enter="openLink(url, 'har')"
            @keydown.space.prevent="openLink(url, 'har')"
          >
            <span class="share-link mono">{{ url }}</span>
            <button class="btn" type="button" @click.stop="copyLink(url, 'har')">复制</button>
          </div>
        </div>
        <div v-else class="share-empty">暂无可用的本机 IPv4 地址</div>
        <p v-if="!serviceRunning" class="share-warning">
          管理 HTTP 服务未运行，以上地址当前无法访问。
        </p>
      </template>
      <template v-else>
        <div class="scope-picker" role="group" aria-label="HAR 分享范围">
          <button
            type="button"
            :class="{ active: harScope === 'all' }"
            @click="harScope = 'all'"
          >
            全部结果
          </button>
          <button
            type="button"
            :class="{ active: harScope === 'filtered' }"
            @click="harScope = 'filtered'"
          >
            当前筛选结果
          </button>
        </div>
        <p class="scope-hint">
          {{
            harScope === "filtered"
              ? "开启时固化当前 Session 已应用筛选命中的记录 ID。"
              : "开启时固化当前 Session 的全部记录 ID。"
          }}
        </p>
      </template>
    </section>
  </main>
</template>

<style scoped>
.share-root {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding: 18px;
  overflow-y: auto;
}
.share-card {
  flex: none;
  padding: 16px;
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  background: var(--bg-panel);
}
.card-heading,
.share-link-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.card-heading {
  align-items: flex-start;
}
.card-heading h3 {
  margin: 0;
  font-size: 13px;
}
.card-heading p {
  margin: 4px 0 0;
  color: var(--text-secondary);
  font-size: 12px;
}
.switch:disabled {
  cursor: default;
  opacity: 0.5;
}
.share-links {
  display: flex;
  flex-direction: column;
  gap: 7px;
  margin-top: 14px;
}
.share-link-row {
  min-width: 0;
  padding: 7px 7px 7px 10px;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--bg-app);
  cursor: pointer;
}
.share-link-row:hover {
  border-color: var(--border-strong);
}
.share-link-row:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 1px;
}
.share-link {
  min-width: 0;
  flex: 1;
  color: var(--accent);
  overflow-wrap: anywhere;
}
.share-empty {
  margin-top: 14px;
  padding: 16px;
  border: 1px dashed var(--border-strong);
  border-radius: var(--radius-md);
  color: var(--text-secondary);
  text-align: center;
  font-size: 12px;
}
.share-warning {
  margin: 8px 0 0;
  color: var(--warning);
  font-size: 11px;
}
.har-warning {
  margin-top: 14px;
  padding: 10px 12px;
  border-radius: var(--radius-md);
  background: color-mix(in srgb, var(--warning) 10%, transparent);
  color: var(--warning);
  font-size: 11px;
  line-height: 1.5;
}
.snapshot-summary {
  margin: 14px 0 0;
  color: var(--text-secondary);
  font-size: 12px;
}
.scope-picker {
  display: inline-flex;
  margin-top: 14px;
  padding: 2px;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--bg-app);
}
.scope-picker button {
  padding: 5px 10px;
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  font-size: 12px;
}
.scope-picker button.active {
  background: var(--bg-active);
  color: var(--text-primary);
}
.scope-hint {
  margin: 8px 0 0;
  color: var(--text-faint);
  font-size: 11px;
}
</style>
