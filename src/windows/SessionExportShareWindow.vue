<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { Io5ChevronDown, Io5ChevronForward } from "vue-icons-plus/io5";
import RedactionSelect from "../components/RedactionSelect.vue";
import ShareAssetTree from "../components/ShareAssetTree.vue";
import { useBackend } from "../api";
import { t } from "../i18n";
import type { HttpApiChange } from "../api/generated/HttpApiChange";
import type { AssetMetadata } from "../api/generated/AssetMetadata";
import type { SessionShareState } from "../api/generated/SessionShareState";
import { sessionsStore } from "../stores/sessions";
import { appStore, reportError } from "../stores/app";
import { HTTP_API_CHANGE_EVENT } from "../stores/http-api-sync";
import { resourceMatches } from "../utils/bundle";
import { formatBytes } from "../utils/format";
import { copyText } from "../utils/clipboard";
import { buildSessionShareLinks } from "../utils/session-share";

const props = defineProps<{ sessionId: number }>();

const backend = useBackend();
const canShare = computed(() => typeof backend.enableSessionShare === "function");
const canPickAssets = computed(() => canShare.value && typeof backend.listAssets === "function");
const pageShare = ref<SessionShareState | null>(null);
const pageLinks = ref<string[]>([]);
const pagePersistent = ref(false);
const configShare = ref<{ assets: string[] } | null>(null);
const pageRedaction = ref<string | null>(null);
const loading = ref(false);
const pageMutating = ref(false);
const serviceRunning = ref(true);
const assets = ref<AssetMetadata[]>([]);
const assetsLoading = ref(false);
const assetsError = ref("");
const treeExpanded = ref(false);
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
    if (sequence === loadSequence) reportError(error, t("读取分享状态失败"));
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
      redaction_script: value,
      persistent: pagePersistent.value,
    });
    sessionsStore.updateShare(pageShare.value);
    pageRedaction.value = value;
  } catch (error) {
    reportError(error, t("保存脱敏脚本绑定失败"));
  } finally {
    pageMutating.value = false;
  }
}

async function setPagePersistent(persistent: boolean): Promise<void> {
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
    reportError(error, t("保存分享持久化设置失败"));
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
      appStore.toast(t("链接分享已关闭"), "success");
    } else {
      pageShare.value = await backend.enableSessionShare(props.sessionId, {
        session_config_share: configShare.value,
        redaction_script: pageRedaction.value ?? undefined,
        persistent: pagePersistent.value,
      });
      sessionsStore.updateShare(pageShare.value);
      await load();
      appStore.toast(t("链接分享已开启"), "success");
    }
  } catch (error) {
    reportError(error, pageShare.value.enabled ? t("关闭链接分享失败") : t("开启链接分享失败"));
  } finally {
    pageMutating.value = false;
  }
}

async function openLink(url: string): Promise<void> {
  try {
    await backend.openExternal(url);
  } catch (error) {
    reportError(error, t("打开分享链接失败"));
  }
}

async function copyLink(url: string): Promise<void> {
  try {
    await copyText(url);
    appStore.toast(t("分享链接已复制"), "success");
  } catch (error) {
    reportError(error, t("复制分享链接失败"));
  }
}

async function saveConfigShare(value: { assets: string[] } | null): Promise<boolean> {
  if (!pageShare.value?.enabled) {
    configShare.value = value;
    return true;
  }
  pageMutating.value = true;
  try {
    const next = await backend.enableSessionShare(props.sessionId, {
      persistent: pagePersistent.value,
      session_config_share: value,
    });
    pageShare.value = next;
    configShare.value = next.session_config_share ?? null;
    sessionsStore.updateShare(next);
    return true;
  } catch (e) {
    reportError(e, t("保存配置分享设置失败"));
    return false;
  } finally {
    pageMutating.value = false;
  }
}

function toggleConfigShare(): void {
  treeExpanded.value = !configShare.value;
  void saveConfigShare(configShare.value ? null : { assets: [] });
}

function setRules(rules: string[]): void {
  if (!configShare.value) return;
  void saveConfigShare({ assets: rules });
}

const configHint = computed(() => {
  if (!pageShare.value?.enabled) return t("开启链接分享后可配置。");
  if (configShare.value === null) return t("开启后，分享包内附带此 Session 的配置与脚本。");
  return "";
});

const configSummary = computed(() => {
  const rules = configShare.value?.assets ?? [];
  if (!rules.length) return t("未随附资源");
  if (assetsLoading.value) return t("{n} 条规则 · 正在读取资源…", rules.length);
  const matched = resourceMatches(
    rules,
    assets.value.map((asset) => asset.id),
  );
  const size = assets.value
    .filter((asset) => matched.includes(asset.id))
    .reduce((sum, asset) => sum + asset.size, 0);
  return t("{n} 条规则 · 匹配 {m} 个资源 · {size}", {
    n: rules.length,
    m: matched.length,
    size: formatBytes(size),
  });
});

async function loadAssets(): Promise<void> {
  if (!canPickAssets.value) return;
  assetsLoading.value = true;
  assetsError.value = "";
  try {
    assets.value = await backend.listAssets();
  } catch (error) {
    assetsError.value = error instanceof Error ? error.message : String(error);
  } finally {
    assetsLoading.value = false;
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
  void loadAssets();
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
    <header class="share-heading">
      <div>
        <h3>{{ t("链接分享") }}</h3>
        <p>
          {{
            t("只读查看此 Session；{hint}", {
              hint: pagePersistent
                ? t("重启后保留原链接，关闭分享后失效。")
                : t("应用退出或关闭分享后链接失效。"),
            })
          }}
        </p>
      </div>
      <button
        class="switch"
        :class="{ on: pageShare?.enabled }"
        type="button"
        :disabled="loading || pageMutating || !pageShare"
        :aria-pressed="pageShare?.enabled ?? false"
        :aria-label="pageShare?.enabled ? t('关闭链接分享') : t('开启链接分享')"
        :title="pageShare?.enabled ? t('关闭链接分享') : t('开启链接分享')"
        @click="togglePageShare"
      />
    </header>

    <div v-if="canShare" class="settings">
      <div class="setting-row">
        <span>{{ t("重启后保留") }}</span>
        <span class="setting-spacer" />
        <button
          class="switch"
          :class="{ on: pagePersistent }"
          type="button"
          role="switch"
          :aria-label="t('重启后保留')"
          :title="t('重启后保留')"
          :aria-checked="pagePersistent"
          :disabled="loading || pageMutating || !pageShare"
          @click="setPagePersistent(!pagePersistent)"
        />
      </div>
      <RedactionSelect
        v-if="backend.listRedactionScripts"
        :inherit-label="t('跟随本机无 API Key')"
        :inherited-value-label="
          pageShare?.redaction_identity ? t('跟随 API Key 脱敏配置') : undefined
        "
        :model-value="pageRedaction"
        :disabled="loading || pageMutating"
        @update:model-value="setRedaction($event)"
      />
      <div v-if="canPickAssets" class="setting-block">
        <div class="setting-row">
          <span>{{ t("允许下载 Session 配置") }}</span>
          <span class="setting-spacer" />
          <button
            class="switch"
            :class="{ on: !!configShare }"
            type="button"
            role="switch"
            :aria-label="t('允许下载 Session 配置')"
            :title="t('允许下载 Session 配置')"
            :aria-checked="!!configShare"
            :disabled="loading || pageMutating || !pageShare?.enabled"
            @click="toggleConfigShare"
          />
        </div>
        <p v-if="configHint" class="setting-hint">{{ configHint }}</p>
        <template v-if="configShare">
          <button
            class="tree-toggle"
            type="button"
            :aria-expanded="treeExpanded"
            @click="treeExpanded = !treeExpanded"
          >
            <component :is="treeExpanded ? Io5ChevronDown : Io5ChevronForward" :size="11" />
            <span>{{ configSummary }}</span>
          </button>
          <div v-if="treeExpanded" class="tree-panel">
            <p v-if="assetsLoading" class="tree-note">{{ t("正在读取工作区资源…") }}</p>
            <p v-else-if="assetsError" class="tree-note text-danger">{{ assetsError }}</p>
            <p v-else-if="!assets.length" class="tree-note">{{ t("工作区暂无资源。") }}</p>
            <ShareAssetTree
              v-else
              :assets="assets"
              :rules="configShare.assets"
              :disabled="pageMutating"
              @rules="setRules"
            />
          </div>
        </template>
      </div>
    </div>

    <div v-if="loading && !pageShare" class="share-empty">{{ t("正在读取分享状态…") }}</div>
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
          <button class="btn" type="button" @click.stop="copyLink(url)">{{ t("复制") }}</button>
        </div>
      </div>
      <div v-else class="share-empty">{{ t("暂无可用的本机 IPv4 地址") }}</div>
      <p v-if="!serviceRunning" class="share-warning">
        {{ t("管理 HTTP 服务未运行，以上地址当前无法访问。") }}
      </p>
    </template>
    <div v-else class="share-empty">{{ t("开启后将在这里列出可打开和复制的分享地址。") }}</div>
  </main>
</template>

<style scoped>
.share-root {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 18px;
  overflow-y: auto;
}
.share-heading {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
}
.share-heading h3 {
  margin: 0;
  font-size: 13px;
}
.share-heading p {
  margin: 4px 0 0;
  color: var(--text-secondary);
  font-size: 12px;
}
.settings {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.setting-row {
  display: flex;
  align-items: center;
  gap: 12px;
  min-height: 24px;
  font-size: 12px;
}
.setting-spacer {
  flex: 1;
}
.setting-hint {
  margin: 0;
  font-size: 11px;
  line-height: 1.5;
  color: var(--text-faint);
}
.setting-block {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.tree-toggle {
  display: inline-flex;
  align-items: center;
  align-self: flex-start;
  max-width: 100%;
  gap: 5px;
  padding: 3px 0;
  border: 0;
  background: transparent;
  color: var(--text-secondary);
  font: inherit;
  font-size: 11px;
  cursor: pointer;
}
.tree-toggle:hover {
  color: var(--text);
}
.tree-toggle span {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.tree-panel {
  margin: 2px 0 4px;
  padding-top: 2px;
  border-top: 1px solid var(--border);
}
.tree-note {
  margin: 6px 0;
  font-size: 11px;
  color: var(--text-faint);
}
.switch:disabled {
  cursor: default;
  opacity: 0.5;
}
.share-links {
  display: flex;
  flex-direction: column;
  gap: 7px;
}
.share-link-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-width: 0;
  gap: 12px;
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
