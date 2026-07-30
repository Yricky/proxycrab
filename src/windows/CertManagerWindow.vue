<script setup lang="ts">
import { onMounted, ref } from "vue";
import { Io5Copy, Io5Download, Io5Refresh, Io5Warning } from "vue-icons-plus/io5";
import { useBackend } from "../api";
import { appStore, reportError } from "../stores/app";
import { confirmDialog } from "../stores/dialog";
import { proxyStore } from "../stores/proxy";

const backend = useBackend();

const pem = ref("");
const loading = ref(false);

async function refresh(): Promise<void> {
  try {
    const cert = await backend.getCertificate();
    pem.value = cert.pem;
  } catch (error) {
    reportError(error, "获取证书失败");
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

onMounted(() => void refresh());
</script>

<template>
  <div class="cert-root">
    <p class="cert-desc text-secondary">
      这是 ProxyCrab 的根 CA 证书（PEM 格式）。请将它安装到操作系统或浏览器的受信任根证书列表中，
      以便代理解密 HTTPS 流量。
    </p>

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
