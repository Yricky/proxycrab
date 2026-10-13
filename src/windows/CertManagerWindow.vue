<script setup lang="ts">
import QRCode from "qrcode";
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { Io5Copy, Io5Download, Io5Refresh, Io5Warning } from "vue-icons-plus/io5";
import { useBackend } from "../api";
import type { ClashSubscriptionState, HttpApiChange, HttpServiceStatus } from "../api/types";
import { appStore, reportError } from "../stores/app";
import { confirmDialog } from "../stores/dialog";
import { HTTP_API_CHANGE_EVENT } from "../stores/http-api-sync";
import { proxyStore } from "../stores/proxy";
import { copyText as writeClipboardText } from "../utils/clipboard";
import { clashServerAddresses, clashSubscriptionUrl } from "../utils/clash-subscription";
import ClashConnectionPanel from "./ClashConnectionPanel.vue";

const backend = useBackend();
const addresses = ref<string[]>([]);
const selectedIp = ref("");
const service = ref<HttpServiceStatus | null>(null);
const clashState = ref<ClashSubscriptionState | null>(null);
const connectionLoading = ref(false);
const connectionMutating = ref(false);
const canConnect = computed(() => !!service.value?.running && addresses.value.includes(selectedIp.value));
const certificateUrl = computed(() => canConnect.value ? `http://${selectedIp.value}:${service.value!.port}/ca.crt` : "");
const clashUrl = computed(() => {
  if (connectionMutating.value || !canConnect.value || !clashState.value?.enabled || !clashState.value.token || !clashState.value.server) return "";
  return clashSubscriptionUrl(clashState.value.server, service.value!.port, clashState.value.token);
});
const activeTab = ref<"certificate" | "connection">("certificate");

const pem = ref("");
const loading = ref(false);
const qrCodeDataUrl = ref("");

async function refreshConnection(): Promise<void> {
  if (connectionLoading.value || connectionMutating.value) return;
  connectionLoading.value = true;
  try {
    const [ips, status, subscription] = await Promise.all([
      backend.listLocalIps(), backend.getHttpServiceStatus(),
      backend.clashSubscription?.getState() ?? Promise.resolve(null),
    ]);
    addresses.value = clashServerAddresses(ips);
    service.value = status;
    clashState.value = subscription;
    selectedIp.value = subscription?.server ?? (addresses.value.includes(selectedIp.value) ? selectedIp.value : addresses.value[0] ?? "");
  } catch (error) {
    reportError(error, "读取连接配置失败");
  } finally {
    connectionLoading.value = false;
  }
}

async function selectIp(event: Event): Promise<void> {
  const ip = (event.target as HTMLSelectElement).value;
  if (connectionLoading.value || connectionMutating.value) return;
  selectedIp.value = ip;
  if (!backend.clashSubscription || !clashState.value?.enabled) return;
  connectionMutating.value = true;
  try {
    clashState.value = await backend.clashSubscription.enable(ip);
  } catch (error) {
    selectedIp.value = clashState.value.server ?? "";
    reportError(error, "更新订阅 IP 失败");
  } finally {
    connectionMutating.value = false;
  }
}

async function toggleClash(): Promise<void> {
  if (!backend.clashSubscription || !clashState.value || connectionLoading.value || connectionMutating.value) return;
  connectionMutating.value = true;
  try {
    clashState.value = clashState.value.enabled
      ? await backend.clashSubscription.disable()
      : await backend.clashSubscription.enable(selectedIp.value);
  } catch (error) {
    reportError(error, "切换 Clash 订阅失败");
  } finally {
    connectionMutating.value = false;
  }
}

async function refresh(): Promise<void> {
  try {
    const cert = await backend.getCertificate();
    pem.value = cert.pem;
  } catch (error) {
    reportError(error, "获取证书失败");
  }
}

function onHttpApiChange(event: Event): void {
  const { resources } = (event as CustomEvent<HttpApiChange>).detail;
  if (resources.some((resource) => ["all", "config", "clash_subscription"].includes(resource))) void refreshConnection();
  if (resources.includes("all") || resources.includes("certificate")) {
    void refresh();
  }
}

async function copyPem(): Promise<void> {
  if (!pem.value) return;
  try {
    await writeClipboardText(pem.value);
    appStore.toast("证书 PEM 已复制到剪贴板", "success");
  } catch (error) {
    reportError(error, "复制失败");
  }
}

async function copyDownloadUrl(): Promise<void> {
  if (!certificateUrl.value) return;
  try {
    await writeClipboardText(certificateUrl.value);
    appStore.toast("证书下载地址已复制", "success");
  } catch (error) {
    reportError(error, "复制失败");
  }
}

async function openDownloadUrl(): Promise<void> {
  if (!certificateUrl.value) return;
  try {
    await backend.openExternal(certificateUrl.value);
  } catch (error) {
    reportError(error, "打开证书下载地址失败");
  }
}

async function downloadPem(): Promise<void> {
  if (!pem.value) return;
  try {
    await backend.saveFile({
      filename: "proxycrab-ca.pem",
      data: new TextEncoder().encode(pem.value),
    });
  } catch (error) {
    reportError(error, "保存证书失败");
  }
}

async function regenerate(): Promise<void> {
  const host = backend.host;
  if (!host) return;
  const ok = await confirmDialog({
    title: "重新生成 CA 证书",
    message: "重新生成后，旧证书签发的所有站点证书将失效，需要重新安装并信任新证书。确定继续吗？",
    confirmText: "重新生成",
    danger: true,
  });
  if (!ok) return;
  loading.value = true;
  try {
    const cert = await host.regenerateCertificate();
    pem.value = cert.pem;
    appStore.toast("证书已重新生成", "success");
  } catch (error) {
    reportError(error, "重新生成证书失败");
  } finally {
    loading.value = false;
  }
}

watch(certificateUrl, async (url, _previous, onCleanup) => {
  let stale = false;
  onCleanup(() => { stale = true; });
  qrCodeDataUrl.value = "";
  if (!url) return;
  try {
    const image = await QRCode.toDataURL(url, { errorCorrectionLevel: "M", margin: 1, width: 160 });
    if (!stale) qrCodeDataUrl.value = image;
  } catch (error) {
    if (!stale) reportError(error, "生成证书下载二维码失败");
  }
});

onMounted(() => {
  void refresh();
  void refreshConnection();
  window.addEventListener(HTTP_API_CHANGE_EVENT, onHttpApiChange);
});

onBeforeUnmount(() => {
  window.removeEventListener(HTTP_API_CHANGE_EVENT, onHttpApiChange);
});
</script>

<template>
  <div class="cert-manager-root">
    <div class="cert-tabs" role="tablist" aria-label="代理配置步骤">
      <button id="certificate-tab" class="cert-tab" :class="{ active: activeTab === 'certificate' }" role="tab" :aria-selected="activeTab === 'certificate'" aria-controls="certificate-panel" @click="activeTab = 'certificate'">1.证书配置</button>
      <button v-if="backend.clashSubscription" id="connection-tab" class="cert-tab" :class="{ active: activeTab === 'connection' }" role="tab" :aria-selected="activeTab === 'connection'" aria-controls="connection-panel" @click="activeTab = 'connection'">2. 代理连接</button>
    </div>
    <div class="connection-ip-row">
      <label for="connection-ip">连接 IP</label>
      <select id="connection-ip" class="input mono" :value="selectedIp" :disabled="connectionLoading || connectionMutating || !addresses.length" @change="selectIp">
        <option v-if="!addresses.length" value="">暂无可用的 IPv4 地址</option>
        <option v-else-if="selectedIp && !addresses.includes(selectedIp)" :value="selectedIp" disabled>{{ selectedIp }}（已不可用）</option>
        <option v-for="ip in addresses" :key="ip" :value="ip">{{ ip }}</option>
      </select>
      <button class="btn icon" title="刷新 IP 和连接状态" :disabled="connectionLoading || connectionMutating" @click="refreshConnection"><Io5Refresh :size="14" /></button>
    </div>
    <p v-if="service && !service.running" class="connection-warning"><Io5Warning :size="14" />管理 HTTP 服务未运行，暂时无法下载证书或订阅。</p>
    <p v-else-if="!connectionLoading && !addresses.length" class="connection-warning"><Io5Warning :size="14" />请连接网络后刷新，选择其他设备可访问的本机 IP。</p>
    <p v-else-if="selectedIp && !addresses.includes(selectedIp)" class="connection-warning"><Io5Warning :size="14" />原 IP 已不可用，请重新选择 IP 并更新 Clash 中的订阅地址。</p>
  <div v-show="activeTab === 'certificate'" id="certificate-panel" class="cert-root" role="tabpanel" aria-labelledby="certificate-tab">
    <p class="cert-desc text-secondary">
      这是 ProxyCrab 的根 CA 证书（PEM 格式）。请将它安装到操作系统或浏览器的受信任根证书列表中，
      以便代理解密 HTTPS 流量。
    </p>

    <section class="cert-download" aria-labelledby="cert-download-title">
      <div class="cert-download-info">
        <h3 id="cert-download-title">从其他设备下载</h3>
        <p class="text-secondary">
          让手机或其他设备能访问所选 IP，打开以下链接或扫描二维码，无需先连接代理。
        </p>
        <div v-if="certificateUrl" class="cert-download-link">
          <a :href="certificateUrl" class="mono" @click.prevent="openDownloadUrl">
            {{ certificateUrl }}
          </a>
          <button class="btn icon" title="复制证书下载地址" @click="copyDownloadUrl">
            <Io5Copy :size="14" />
          </button>
        </div>
      </div>
      <div v-if="certificateUrl" class="cert-qr">
        <img v-if="qrCodeDataUrl" :src="qrCodeDataUrl" alt="证书下载地址二维码" />
        <span v-else class="text-faint">二维码生成中…</span>
      </div>
    </section>

    <div class="cert-content-label">证书内容</div>
    <pre class="cert-pem mono">{{ pem || "加载中…" }}</pre>

    <div v-if="proxyStore.running" class="cert-hint">
      <Io5Warning :size="14" />
      <span>代理正在运行，需先停止代理才能重新生成证书。</span>
    </div>

    <div class="cert-actions">
      <button class="btn" :disabled="!pem" @click="copyPem">
        <Io5Copy :size="14" /> 复制 PEM
      </button>
      <button class="btn" :disabled="!pem" @click="downloadPem">
        <Io5Download :size="14" /> 下载 PEM
      </button>
      <span class="cert-spacer" />
      <button
        v-if="backend.host"
        class="btn danger"
        :disabled="proxyStore.running || loading"
        :title="proxyStore.running ? '需先停止代理' : '重新生成 CA 证书'"
        @click="regenerate"
      >
        <Io5Refresh :size="14" /> 重新生成证书
      </button>
    </div>
  </div>
    <ClashConnectionPanel
      v-if="activeTab === 'connection' && backend.clashSubscription"
      id="connection-panel" role="tabpanel" aria-labelledby="connection-tab"
      :state="clashState" :url="clashUrl" :loading="connectionLoading" :mutating="connectionMutating"
      :can-enable="canConnect" @toggle="toggleClash"
    />
  </div>
</template>

<style scoped>
.cert-manager-root {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.cert-tabs {
  display: flex;
  flex: none;
  gap: var(--space-3);
  padding: 0 var(--space-3);
  border-bottom: 1px solid var(--border);
}

.cert-tab {
  padding: 10px 2px 8px;
  border: 0;
  border-bottom: 2px solid transparent;
  background: transparent;
  color: var(--text-secondary);
  font-size: 12px;
  cursor: pointer;
}

.cert-tab.active {
  color: var(--accent);
  border-bottom-color: var(--accent);
}

.connection-ip-row { display: flex; align-items: center; gap: var(--space-2); padding: var(--space-2) var(--space-3) 0; flex: none; }
.connection-ip-row label { flex: none; font-size: 12px; }
.connection-ip-row select { flex: 1; min-width: 0; }
.connection-warning { display: flex; align-items: flex-start; gap: var(--space-1); color: var(--warning); font-size: 12px; margin: var(--space-2) var(--space-3) 0; }
.connection-warning svg { flex: none; margin-top: 2px; }

.cert-root {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding: var(--space-3);
  gap: var(--space-2);
}

.cert-desc {
  margin: 0;
  font-size: 12px;
  flex: none;
}

.cert-download {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 112px;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-2) 0;
  border-top: 1px solid var(--border);
  border-bottom: 1px solid var(--border);
  flex: none;
}

.cert-download-info {
  min-width: 0;
}

.cert-download h3 {
  margin: 0 0 2px;
  font-size: 13px;
  font-weight: 600;
}

.cert-download p {
  margin: 0 0 var(--space-2);
  font-size: 12px;
}

.cert-download-link {
  display: flex;
  align-items: center;
  min-width: 0;
  height: 32px;
  padding-left: 10px;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--bg-panel);
}

.cert-download-link a {
  min-width: 0;
  flex: 1;
  overflow: hidden;
  color: var(--accent);
  text-overflow: ellipsis;
  white-space: nowrap;
  user-select: text;
}

.cert-download-link a:hover {
  text-decoration: underline;
}

.cert-download-link .btn {
  margin: 2px;
  flex: none;
}

.cert-qr {
  display: grid;
  place-items: center;
  width: 112px;
  height: 112px;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: #fff;
  overflow: hidden;
  font-size: 11px;
}

.cert-qr img {
  display: block;
  width: 104px;
  height: 104px;
}

.cert-content-label {
  color: var(--text-secondary);
  font-size: 12px;
  font-weight: 600;
  flex: none;
}

.cert-pem {
  flex: 1;
  min-height: 0;
  margin: 0;
  padding: var(--space-2);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--bg-panel);
  overflow: auto;
  white-space: pre-wrap;
  word-break: break-all;
  user-select: text;
}

.cert-hint {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  color: var(--warning);
  font-size: 12px;
  flex: none;
}

.cert-actions {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  flex: none;
}

.cert-spacer {
  flex: 1;
}
</style>
