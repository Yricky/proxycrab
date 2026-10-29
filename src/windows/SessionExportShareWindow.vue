<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";
import ShareAssetsEditor from "../components/ShareAssetsEditor.vue";
import RedactionSelect from "../components/RedactionSelect.vue";
import { useBackend } from "../api";
import type { HttpApiChange, SessionShareState } from "../api/types";
import { sessionsStore } from "../stores/sessions";
import { appStore, reportError } from "../stores/app";
import { HTTP_API_CHANGE_EVENT } from "../stores/http-api-sync";
import { copyText } from "../utils/clipboard";
import { buildSessionShareLinks } from "../utils/session-share";

const props = defineProps<{ sessionId: number }>();

const backend = useBackend();
const pageShare = ref<SessionShareState | null>(null);
const pageLinks = ref<string[]>([]);
const pagePersistent = ref(false);
const configShare = ref<{ assets: string[] } | null>(null);
const editingAssets = ref(false);
const pageRedaction = ref<string | null>(null);
const loading = ref(false);
const pageMutating = ref(false);
const serviceRunning = ref(true);
let loadSequence = 0;

async function load(): Promise<void> {
  const sequence = ++loadSequence;
  loading.value = true;
  try {
    const [nextPageShare, status, addresses] = await Promise.all([
      backend.getSessionShare(props.sessionId),
      backend.getHttpServiceStatus(),
      backend.listLocalIps?.() ?? [],
    ]);
    if (sequence !== loadSequence) return;
    pageShare.value = nextPageShare;
    if (nextPageShare.enabled) configShare.value = nextPageShare.session_config_share ?? null;
    sessionsStore.updateShare(nextPageShare);
    if (nextPageShare.enabled) pagePersistent.value = nextPageShare.persistent;
    if (nextPageShare.enabled) pageRedaction.value = nextPageShare.redaction_script ?? null;
    serviceRunning.value = status.running;
    pageLinks.value = nextPageShare.token
      ? buildSessionShareLinks(addresses, status.port, nextPageShare.token)
      : [];
  } catch (error) {
    if (sequence === loadSequence) reportError(error, "读取分享状态失败");
  } finally {
    if (sequence === loadSequence) loading.value = false;
  }
}

async function setRedaction(value: string | null): Promise<void> {
  if (!pageShare.value?.enabled) {
    pageRedaction.value = value;
    return;
  }
  pageMutating.value = true;
  try {
    pageShare.value = await backend.enableSessionShare(props.sessionId, {
      redactionScript: value,
      persistent: pagePersistent.value,
    });
    sessionsStore.updateShare(pageShare.value);
    pageRedaction.value = value;
  } catch (error) {
    reportError(error, "保存脱敏脚本绑定失败");
  } finally {
    pageMutating.value = false;
  }
}

async function setPagePersistent(event: Event): Promise<void> {
  const input = event.target as HTMLInputElement;
  const persistent = input.checked;
  if (!pageShare.value?.enabled) {
    pagePersistent.value = persistent;
    return;
  }
  pageMutating.value = true;
  try {
    pageShare.value = await backend.enableSessionShare(props.sessionId, {
      persistent,
    });
    sessionsStore.updateShare(pageShare.value);
    pagePersistent.value = pageShare.value.persistent;
  } catch (error) {
    input.checked = pagePersistent.value;
    reportError(error, "保存分享持久化设置失败");
  } finally {
    pageMutating.value = false;
  }
}

async function togglePageShare(): Promise<void> {
  if (pageMutating.value || !pageShare.value) return;
  pageMutating.value = true;
  try {
    if (pageShare.value.enabled) {
      pageShare.value = await backend.disableSessionShare(props.sessionId);
      sessionsStore.updateShare(pageShare.value);
      pageLinks.value = [];
      appStore.toast("链接分享已关闭", "success");
    } else {
      pageShare.value = await backend.enableSessionShare(props.sessionId, {
        sessionConfigShare: configShare.value,
        redactionScript: pageRedaction.value ?? undefined,
        persistent: pagePersistent.value,
      });
      sessionsStore.updateShare(pageShare.value);
      await load();
      appStore.toast("链接分享已开启", "success");
    }
  } catch (error) {
    reportError(error, pageShare.value.enabled ? "关闭链接分享失败" : "开启链接分享失败");
  } finally {
    pageMutating.value = false;
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

async function saveConfigShare(value: { assets: string[] } | null): Promise<void> {
  if (!pageShare.value?.enabled) {
    configShare.value = value;
    editingAssets.value = false;
    return;
  }
  pageMutating.value = true;
  try {
    const next = await backend.enableSessionShare(props.sessionId, {
      persistent: pagePersistent.value,
      sessionConfigShare: value,
    });
    pageShare.value = next;
    configShare.value = next.session_config_share ?? null;
    sessionsStore.updateShare(next);
    editingAssets.value = false;
  } catch (e) {
    reportError(e, "保存配置分享设置失败");
  } finally {
    pageMutating.value = false;
  }
}
function toggleConfigShare(): void {
  if (configShare.value) void saveConfigShare(null);
  else editingAssets.value = true;
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
  loadSequence++;
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
          <p>
            只读查看此 Session；{{
              pagePersistent
                ? "重启后保留原链接，关闭分享后失效。"
                : "应用退出或关闭分享后链接失效。"
            }}
          </p>
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

      <label v-if="typeof backend.enableSessionShare === 'function'" class="persistence-option">
        <input
          type="checkbox"
          :checked="pagePersistent"
          :disabled="loading || pageMutating || !pageShare"
          @change="setPagePersistent"
        />
        重启后保留
      </label>
      <RedactionSelect
        v-if="typeof backend.enableSessionShare === 'function' && backend.listRedactionScripts"
        inherit-label="跟随本机无 API Key"
        :inherited-value-label="pageShare?.redaction_identity ? '跟随 API Key 脱敏配置' : undefined"
        :model-value="pageRedaction"
        :disabled="loading || pageMutating"
        @update:model-value="setRedaction($event)"
      />
      <div v-if="backend.listAssets && backend.enableSessionShare" class="config-share">
        <div class="config-share-row">
          <label
            ><input
              type="checkbox"
              :checked="!!configShare"
              :disabled="loading || pageMutating"
              @change="toggleConfigShare"
            />
            允许下载 Session 配置</label
          >
          <button
            v-if="configShare"
            class="btn"
            :disabled="pageMutating"
            @click="editingAssets = true"
          >
            随附资源 {{ configShare.assets.length }} 条规则 · 编辑
          </button>
        </div>
        <p>下载时使用最新配置、脚本及资源；目录前缀包含其下后续新增的资源。</p>
        <ShareAssetsEditor
          v-if="editingAssets"
          :initial="configShare?.assets ?? []"
          @save="(assets) => saveConfigShare({ assets })"
          @cancel="editingAssets = false"
        />
      </div>
      <div v-if="loading && !pageShare" class="share-empty">正在读取分享状态…</div>
      <template v-else-if="pageShare?.enabled">
        <div v-if="pageLinks.length" class="share-links">
          <div
            v-for="url in pageLinks"
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
      <div v-else class="share-empty">开启后将在这里列出可打开和复制的分享地址。</div>
    </section>
  </main>
</template>

<style scoped>
.config-share {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.config-share-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}
.config-share p {
  margin: 0;
  font-size: 12px;
  color: var(--text-secondary);
}
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
.persistence-option {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 10px;
  color: var(--text-secondary);
  font-size: 12px;
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
</style>
