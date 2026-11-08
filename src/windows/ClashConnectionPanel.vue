<script setup lang="ts">
import QRCode from "qrcode";
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { Io5Copy, Io5OpenOutline, Io5Reload, Io5Save, Io5Warning } from "vue-icons-plus/io5";
import { useBackend } from "../api";
import { t } from "../i18n";
import type { HttpApiChange } from "../api/generated/HttpApiChange";
import type { ClashConfig } from "../api/generated/ClashConfig";
import type { ClashSubscriptionState } from "../api/generated/ClashSubscriptionState";
import MonacoEditor from "../components/MonacoEditor.vue";
import { appStore, reportError } from "../stores/app";
import { confirmDialog } from "../stores/dialog";
import { HTTP_API_CHANGE_EVENT } from "../stores/http-api-sync";
import { proxyStore } from "../stores/proxy";
import { copyText } from "../utils/clipboard";
import { clashImportUrl } from "../utils/clash-subscription";

const backend = useBackend();
const configBackend = backend.clashConfig;
const props = defineProps<{
  state: ClashSubscriptionState | null;
  url: string;
  loading: boolean;
  mutating: boolean;
  canEnable: boolean;
}>();
const emit = defineEmits<{ toggle: [] }>();
const qrCode = ref("");

const configSaved = ref("");
const configDraft = ref("");
const configLoading = ref(false);
const configSaving = ref(false);
const configError = ref("");
const configExternal = ref(false);
const configDirty = computed(() => configDraft.value !== configSaved.value);
const configEditorOptions = {
  fontSize: 12,
  lineNumbersMinChars: 3,
  padding: { top: 8, bottom: 8 },
};

function applyConfig(value: ClashConfig): void {
  configSaved.value = value.yaml;
  configDraft.value = value.yaml;
  configError.value = "";
  configExternal.value = false;
}

async function loadConfig(): Promise<void> {
  if (!configBackend || configLoading.value) return;
  configLoading.value = true;
  try {
    applyConfig(await configBackend.get());
  } catch (error) {
    configError.value = reportError(error, t("读取 Clash 规则文件失败"));
  } finally {
    configLoading.value = false;
  }
}

async function saveConfig(): Promise<void> {
  if (!configBackend || !configDirty.value || configSaving.value) return;
  configSaving.value = true;
  configError.value = "";
  try {
    applyConfig(await configBackend.save(configDraft.value));
    appStore.toast(t("Clash 规则文件已保存"), "success");
  } catch (error) {
    configError.value = reportError(error, t("保存 Clash 规则文件失败"));
  } finally {
    configSaving.value = false;
  }
}

async function reloadConfig(): Promise<void> {
  if (configLoading.value || configSaving.value) return;
  if (configDirty.value) {
    const ok = await confirmDialog({
      title: t("重新载入规则文件"),
      message: t("当前修改尚未保存，重新载入会丢弃这些修改。确定继续吗？"),
      confirmText: t("重新载入"),
      danger: true,
    });
    if (!ok) return;
  }
  await loadConfig();
}

async function resetConfig(): Promise<void> {
  if (!configBackend || configSaving.value) return;
  const ok = await confirmDialog({
    title: t("恢复默认规则文件"),
    message: t("将覆盖当前规则文件，恢复为默认的 MATCH,ProxyCrab 配置。确定继续吗？"),
    confirmText: t("恢复默认"),
    danger: true,
  });
  if (!ok) return;
  configSaving.value = true;
  configError.value = "";
  try {
    applyConfig(await configBackend.reset());
    appStore.toast(t("Clash 规则文件已恢复默认"), "success");
  } catch (error) {
    configError.value = reportError(error, t("恢复默认规则文件失败"));
  } finally {
    configSaving.value = false;
  }
}

function onHttpApiChange(event: Event): void {
  const { resources } = (event as CustomEvent<HttpApiChange>).detail;
  if (!resources.some((resource) => resource === "all" || resource === "clash_config")) return;
  if (configDirty.value) {
    configExternal.value = true;
    return;
  }
  void loadConfig();
}

async function copyUrl(): Promise<void> {
  if (!props.url) return;
  try {
    await copyText(props.url);
    appStore.toast(t("Clash 订阅链接已复制"), "success");
  } catch (error) {
    reportError(error, t("复制订阅链接失败"));
  }
}

async function importClash(): Promise<void> {
  if (!props.url) return;
  try {
    await backend.openExternal(clashImportUrl(props.url));
  } catch (error) {
    reportError(error, t("打开 Clash 失败，请确认已安装客户端，或复制链接手动导入"));
  }
}

watch(configDraft, () => {
  if (configError.value) configError.value = "";
});

watch(
  () => props.url,
  async (value, _previous, onCleanup) => {
    let stale = false;
    onCleanup(() => {
      stale = true;
    });
    qrCode.value = "";
    if (!value) return;
    try {
      const image = await QRCode.toDataURL(value, {
        errorCorrectionLevel: "M",
        margin: 2,
        width: 200,
      });
      if (!stale) qrCode.value = image;
    } catch (error) {
      if (!stale) reportError(error, t("生成订阅二维码失败"));
    }
  },
  { immediate: true },
);

onMounted(() => {
  if (configBackend) {
    void loadConfig();
    window.addEventListener(HTTP_API_CHANGE_EVENT, onHttpApiChange);
  }
});

onBeforeUnmount(() => {
  if (configBackend) window.removeEventListener(HTTP_API_CHANGE_EVENT, onHttpApiChange);
});
</script>

<template>
  <div class="connection-root">
    <div class="connection-top">
      <div class="connection-heading">
        <div>
          <h3>{{ t("Clash 连接") }}</h3>
          <p class="text-secondary">{{ t("将订阅导入 Clash，把流量转发到 ProxyCrab。") }}</p>
        </div>
        <button
          class="switch"
          :class="{ on: state?.enabled }"
          type="button"
          :aria-pressed="state?.enabled ?? false"
          :aria-label="state?.enabled ? t('关闭 Clash 订阅') : t('开启 Clash 订阅')"
          :title="state?.enabled ? t('关闭 Clash 订阅') : t('开启 Clash 订阅')"
          :disabled="loading || mutating || !state || (!state.enabled && !canEnable)"
          @click="emit('toggle')"
        />
      </div>

      <section v-if="url" class="subscription-content">
        <div class="subscription-info">
          <h3>{{ t("订阅链接") }}</h3>
          <p class="text-secondary">
            {{ t("其他设备需能访问所选 IP。扫描二维码或复制链接导入订阅。") }}
          </p>
          <div class="subscription-link">
            <span class="mono">{{ url }}</span>
            <button class="btn icon" :title="t('复制 Clash 订阅链接')" @click="copyUrl">
              <Io5Copy :size="14" />
            </button>
          </div>
          <button class="btn primary import-button" @click="importClash">
            <Io5OpenOutline :size="14" />{{ t("导入 Clash") }}
          </button>
          <p class="text-secondary">{{ t("导入后选择该订阅，并在 Clash 中开启系统代理。") }}</p>
        </div>
        <div class="subscription-qr">
          <img v-if="qrCode" :src="qrCode" :alt="t('Clash 订阅链接二维码')" />
          <span v-else class="text-faint">{{ t("二维码生成中…") }}</span>
        </div>
      </section>
      <p v-else class="connection-empty text-secondary">
        {{
          loading
            ? t("正在读取订阅状态…")
            : mutating
              ? t("正在更新订阅…")
              : t("开启后生成订阅链接和二维码。")
        }}
      </p>
      <p v-if="state?.enabled && !proxyStore.running" class="connection-warning">
        <Io5Warning :size="14" />{{ t("ProxyCrab 代理尚未启动，启动后才能转发流量。") }}
      </p>
    </div>

    <section v-if="configBackend" class="config-section">
      <div class="config-heading">
        <div class="config-title">
          <h3>{{ t("规则文件") }}</h3>
        </div>
        <div class="config-actions">
          <span v-if="configDirty" class="config-dirty">{{ t("未保存") }}</span>
          <button
            class="btn icon"
            :title="t('重新载入规则文件')"
            :disabled="configLoading || configSaving"
            @click="reloadConfig"
          >
            <Io5Reload :size="14" />
          </button>
          <button class="btn" :disabled="configLoading || configSaving" @click="resetConfig">
            {{ t("恢复默认") }}
          </button>
          <button
            class="btn primary"
            :disabled="!configDirty || configLoading || configSaving"
            @click="saveConfig"
          >
            <Io5Save :size="14" />{{ configSaving ? t("保存中…") : t("保存") }}
          </button>
        </div>
      </div>
      <p v-if="configError" class="config-error">{{ configError }}</p>
      <p v-else-if="configExternal && configDirty" class="config-warning">
        {{ t("文件已在外部更新；保存会覆盖外部修改，重新载入会丢弃当前修改。") }}
      </p>
      <div v-if="configLoading && !configSaved" class="config-loading text-secondary">
        {{ t("正在读取规则文件…") }}
      </div>
      <div v-else class="config-editor">
        <MonacoEditor v-model="configDraft" language="yaml" :options="configEditorOptions" />
      </div>
    </section>
  </div>
</template>

<style scoped>
.connection-root {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding: var(--space-3);
  overflow: hidden;
  min-height: 0;
  flex: 1;
}
.connection-top {
  flex: 0 1 auto;
  min-height: 0;
  max-height: 48%;
  overflow: auto;
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}
.connection-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
}
h3 {
  margin: 0 0 4px;
  font-size: 13px;
  font-weight: 600;
}
p {
  margin: 0;
  font-size: 12px;
}
.subscription-content {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 160px;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-3) 0;
  border-top: 1px solid var(--border);
  border-bottom: 1px solid var(--border);
}
.subscription-info {
  min-width: 0;
}
.subscription-link {
  display: flex;
  align-items: center;
  gap: 4px;
  margin-top: var(--space-2);
  padding: 4px 4px 4px 8px;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--bg-panel);
}
.subscription-link span {
  flex: 1;
  min-width: 0;
  overflow-wrap: anywhere;
  user-select: text;
  font-size: 11px;
}
.subscription-link .btn {
  flex: none;
}
.import-button {
  margin: var(--space-2) 0;
}
.subscription-qr {
  width: 160px;
  height: 160px;
  display: grid;
  place-items: center;
  background: #fff;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  overflow: hidden;
  font-size: 11px;
}
.subscription-qr img {
  display: block;
  width: 158px;
  height: 158px;
}
.connection-empty {
  padding: var(--space-3) 0;
  border-top: 1px solid var(--border);
}
.connection-warning {
  display: flex;
  align-items: flex-start;
  gap: var(--space-1);
  color: var(--warning);
}
.connection-warning svg {
  flex: none;
  margin-top: 2px;
}
.connection-note {
  font-size: 11px;
}
.config-section {
  flex: 1 1 52%;
  min-height: 160px;
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding-top: var(--space-3);
  border-top: 1px solid var(--border);
}
.config-heading {
  flex: none;
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--space-2);
}
.config-title {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.config-title h3 {
  margin: 0;
}
.config-path {
  min-width: 0;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-faint);
  font-size: 10px;
  user-select: text;
}
.config-actions {
  flex: none;
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: flex-end;
  gap: 4px;
}
.config-dirty {
  color: var(--warning);
  font-size: 11px;
}
.config-hint {
  flex: none;
  font-size: 11px;
  line-height: 1.45;
}
.config-error {
  flex: none;
  color: var(--danger);
  font-size: 11px;
  line-height: 1.45;
  user-select: text;
}
.config-warning {
  flex: none;
  color: var(--warning);
  font-size: 11px;
  line-height: 1.45;
}
.config-loading {
  flex: 1;
  min-height: 0;
  display: grid;
  place-items: center;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  font-size: 11px;
}
.config-editor {
  flex: 1;
  min-height: 0;
  display: flex;
  overflow: hidden;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
}
</style>
