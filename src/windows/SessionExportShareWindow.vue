<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";
import { useBackend } from "../api";
import type { HttpApiChange, SessionShareState } from "../api/types";
import { appStore, reportError } from "../stores/app";
import { confirmDialog } from "../stores/dialog";
import { HTTP_API_CHANGE_EVENT } from "../stores/http-api-sync";
import { copyText } from "../utils/clipboard";
import { buildSessionShareLinks } from "../utils/session-share";

const props = defineProps<{ sessionId: number }>();

const backend = useBackend();
const share = ref<SessionShareState | null>(null);
const links = ref<string[]>([]);
const loading = ref(false);
const mutating = ref(false);
const serviceRunning = ref(true);
let loadSequence = 0;

async function load(): Promise<void> {
  const sequence = ++loadSequence;
  loading.value = true;
  try {
    const [nextShare, status, addresses] = await Promise.all([
      backend.getSessionShare(props.sessionId),
      backend.getHttpServiceStatus(),
      backend.listLocalIps(),
    ]);
    if (sequence !== loadSequence) return;
    share.value = nextShare;
    serviceRunning.value = status.running;
    links.value = nextShare.token
      ? buildSessionShareLinks(addresses, status.port, nextShare.token)
      : [];
  } catch (error) {
    if (sequence === loadSequence) reportError(error, "读取分享状态失败");
  } finally {
    if (sequence === loadSequence) loading.value = false;
  }
}

async function enableShare(): Promise<void> {
  if (mutating.value) return;
  mutating.value = true;
  try {
    await backend.enableSessionShare(props.sessionId);
    await load();
    appStore.toast("分享已启用", "success");
  } catch (error) {
    reportError(error, "启用分享失败");
  } finally {
    mutating.value = false;
  }
}

async function disableShare(): Promise<void> {
  if (mutating.value) return;
  const confirmed = await confirmDialog({
    title: "禁用分享",
    message: "禁用后，该会话已发出的分享链接会立即失效。",
    confirmText: "禁用分享",
    danger: true,
  });
  if (!confirmed) return;

  mutating.value = true;
  try {
    share.value = await backend.disableSessionShare(props.sessionId);
    links.value = [];
    appStore.toast("分享已禁用", "success");
  } catch (error) {
    reportError(error, "禁用分享失败");
  } finally {
    mutating.value = false;
  }
}

async function openLink(url: string): Promise<void> {
  try {
    await backend.openExternal(url);
  } catch (error) {
    reportError(error, "打开分享链接失败");
  }
}

async function copyLink(url: string): Promise<void> {
  try {
    await copyText(url);
    appStore.toast("分享链接已复制", "success");
  } catch (error) {
    reportError(error, "复制分享链接失败");
  }
}

function onRefreshRequest(event: Event): void {
  if ((event as CustomEvent<number>).detail === props.sessionId) void load();
}

function onHttpApiChange(event: Event): void {
  const change = (event as CustomEvent<HttpApiChange>).detail;
  if (
    (change.resources.includes("all") || change.resources.includes("session_share")) &&
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
  <div class="share-root">
    <div class="share-section">
      <div class="section-heading">
        <div>
          <h3>分享</h3>
          <p>通过浏览器只读查看此会话；应用退出或主动禁用后链接失效。</p>
        </div>
        <span v-if="share" class="share-status" :class="{ enabled: share.enabled }">
          {{ share.enabled ? "已启用" : "未启用" }}
        </span>
      </div>

      <div v-if="loading" class="share-empty">正在读取分享状态…</div>
      <template v-else-if="share?.enabled">
        <div v-if="links.length" class="share-links">
          <div
            v-for="url in links"
            :key="url"
            class="share-link-row"
            role="link"
            tabindex="0"
            @click="openLink(url)"
            @keydown.enter="openLink(url)"
            @keydown.space.prevent="openLink(url)"
          >
            <span class="share-link mono">{{ url }}</span>
            <button class="btn" type="button" @click.stop="copyLink(url)">复制</button>
          </div>
        </div>
        <div v-else class="share-empty">暂无可用的本机 IPv4 地址</div>
        <p v-if="!serviceRunning" class="share-warning">
          管理 HTTP 服务未运行，以上地址当前无法访问。
        </p>
      </template>
      <div v-else class="share-empty">启用后将在这里列出可打开和复制的分享地址。</div>
    </div>

    <footer>
      <button
        v-if="share?.enabled"
        class="btn danger"
        :disabled="mutating"
        @click="disableShare"
      >
        {{ mutating ? "禁用中…" : "禁用分享" }}
      </button>
      <button
        v-else
        class="btn primary"
        :disabled="loading || mutating || !share"
        @click="enableShare"
      >
        {{ mutating ? "启用中…" : "启用分享" }}
      </button>
    </footer>
  </div>
</template>

<style scoped>
.share-root { flex: 1; min-height: 0; display: flex; flex-direction: column; padding: 18px; overflow: auto; }
.share-section { flex: 1; min-height: 0; }
.section-heading, .share-link-row, footer { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
.section-heading { align-items: flex-start; }
.section-heading h3 { margin: 0; font-size: 13px; }
.section-heading p { margin: 4px 0 0; color: var(--text-secondary); font-size: 12px; }
.share-status { flex: none; padding: 2px 7px; border-radius: 999px; background: var(--bg-active); color: var(--text-secondary); font-size: 11px; }
.share-status.enabled { background: color-mix(in srgb, var(--success) 16%, transparent); color: var(--success); }
.share-links { display: flex; flex-direction: column; gap: 7px; margin-top: 14px; }
.share-link-row { min-width: 0; padding: 7px 7px 7px 10px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--bg-panel); cursor: pointer; }
.share-link-row:hover { border-color: var(--border-strong); }
.share-link-row:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
.share-link { min-width: 0; flex: 1; color: var(--accent); overflow-wrap: anywhere; }
.share-empty { margin-top: 14px; padding: 16px; border: 1px dashed var(--border-strong); border-radius: var(--radius-md); color: var(--text-secondary); text-align: center; font-size: 12px; }
.share-warning { margin: 8px 0 0; color: var(--warning); font-size: 11px; }
footer { justify-content: flex-end; flex: none; margin-top: 18px; padding-top: 12px; border-top: 1px solid var(--border); }
</style>
