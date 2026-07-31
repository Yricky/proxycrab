<script setup lang="ts">
import { openUrl } from "@tauri-apps/plugin-opener";
import QRCode from "qrcode";
import { onBeforeUnmount, onMounted, ref } from "vue";
import { Io5Copy, Io5Download, Io5Refresh, Io5Warning } from "vue-icons-plus/io5";
import { useBackend } from "../api";
import type { HttpApiChange } from "../api/types";
import { appStore, reportError } from "../stores/app";
import { confirmDialog } from "../stores/dialog";
import { HTTP_API_CHANGE_EVENT } from "../stores/http-api-sync";
import { proxyStore } from "../stores/proxy";

const backend = useBackend();
const CERT_DOWNLOAD_URL = "http://proxy.crab/ca.crt";

const pem = ref("");
const loading = ref(false);
const qrCodeDataUrl = ref("");

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
  if (resources.includes("all") || resources.includes("certificate")) {
    void refresh();
  }
}

async function copyPem(): Promise<void> {
  if (!pem.value) return;
  try {
    await navigator.clipboard.writeText(pem.value);
    appStore.toast("证书 PEM 已复制到剪贴板", "success");
  } catch (error) {
    reportError(error, "复制失败");
  }
}

async function copyDownloadUrl(): Promise<void> {
  try {
    await navigator.clipboard.writeText(CERT_DOWNLOAD_URL);
    appStore.toast("证书下载地址已复制", "success");
  } catch (error) {
    reportError(error, "复制失败");
  }
}

async function openDownloadUrl(): Promise<void> {
  try {
    await openUrl(CERT_DOWNLOAD_URL);
  } catch (error) {
    reportError(error, "打开证书下载地址失败");
  }
}

function downloadPem(): void {
  if (!pem.value) return;
  const blob = new Blob([pem.value], { type: "application/x-pem-file" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = "proxycrab-ca.pem";
  link.click();
  URL.revokeObjectURL(url);
}

async function regenerate(): Promise<void> {
  const ok = await confirmDialog({
    title: "重新生成 CA 证书",
    message: "重新生成后，旧证书签发的所有站点证书将失效，需要重新安装并信任新证书。确定继续吗？",
    confirmText: "重新生成",
    danger: true,
  });
  if (!ok) return;
  loading.value = true;
  try {
    const cert = await backend.regenerateCertificate();
    pem.value = cert.pem;
    appStore.toast("证书已重新生成", "success");
  } catch (error) {
    reportError(error, "重新生成证书失败");
  } finally {
    loading.value = false;
  }
}

async function renderQrCode(): Promise<void> {
  try {
    qrCodeDataUrl.value = await QRCode.toDataURL(CERT_DOWNLOAD_URL, {
      errorCorrectionLevel: "M",
      margin: 1,
      width: 160,
    });
  } catch (error) {
    reportError(error, "生成证书下载二维码失败");
  }
}

onMounted(() => {
  void refresh();
  void renderQrCode();
  window.addEventListener(HTTP_API_CHANGE_EVENT, onHttpApiChange);
});

onBeforeUnmount(() => {
  window.removeEventListener(HTTP_API_CHANGE_EVENT, onHttpApiChange);
});
</script>

<template>
  <div class="cert-root">
    <p class="cert-desc text-secondary">
      这是 ProxyCrab 的根 CA 证书（PEM 格式）。请将它安装到操作系统或浏览器的受信任根证书列表中，
      以便代理解密 HTTPS 流量。
    </p>

    <section class="cert-download" aria-labelledby="cert-download-title">
      <div class="cert-download-info">
        <h3 id="cert-download-title">从代理设备下载</h3>
        <p class="text-secondary">
          让手机或其他设备连接 ProxyCrab 代理后，打开以下链接或扫描二维码。
        </p>
        <div class="cert-download-link">
          <a :href="CERT_DOWNLOAD_URL" class="mono" @click.prevent="openDownloadUrl">
            {{ CERT_DOWNLOAD_URL }}
          </a>
          <button class="btn icon" title="复制证书下载地址" @click="copyDownloadUrl">
            <Io5Copy :size="14" />
          </button>
        </div>
      </div>
      <div class="cert-qr">
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
        class="btn danger"
        :disabled="proxyStore.running || loading"
        :title="proxyStore.running ? '需先停止代理' : '重新生成 CA 证书'"
        @click="regenerate"
      >
        <Io5Refresh :size="14" /> 重新生成证书
      </button>
    </div>
  </div>
</template>

<style scoped>
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
