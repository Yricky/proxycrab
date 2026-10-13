<script setup lang="ts">
import QRCode from "qrcode";
import { ref, watch } from "vue";
import { Io5Copy, Io5OpenOutline, Io5Warning } from "vue-icons-plus/io5";
import { useBackend } from "../api";
import type { ClashSubscriptionState } from "../api/types";
import { appStore, reportError } from "../stores/app";
import { proxyStore } from "../stores/proxy";
import { copyText } from "../utils/clipboard";
import { clashImportUrl } from "../utils/clash-subscription";

const backend = useBackend();
const props = defineProps<{
  state: ClashSubscriptionState | null;
  url: string;
  loading: boolean;
  mutating: boolean;
  canEnable: boolean;
}>();
const emit = defineEmits<{ toggle: [] }>();
const qrCode = ref("");

async function copyUrl(): Promise<void> {
  if (!props.url) return;
  try {
    await copyText(props.url);
    appStore.toast("Clash 订阅链接已复制", "success");
  } catch (error) {
    reportError(error, "复制订阅链接失败");
  }
}

async function importClash(): Promise<void> {
  if (!props.url) return;
  try {
    await backend.openExternal(clashImportUrl(props.url));
  } catch (error) {
    reportError(error, "打开 Clash 失败，请确认已安装客户端，或复制链接手动导入");
  }
}

watch(() => props.url, async (value, _previous, onCleanup) => {
  let stale = false;
  onCleanup(() => { stale = true; });
  qrCode.value = "";
  if (!value) return;
  try {
    const image = await QRCode.toDataURL(value, { errorCorrectionLevel: "M", margin: 2, width: 200 });
    if (!stale) qrCode.value = image;
  } catch (error) {
    if (!stale) reportError(error, "生成订阅二维码失败");
  }
}, { immediate: true });

</script>

<template>
  <div class="connection-root">
    <div class="connection-heading">
      <div>
        <h3>Clash 连接</h3>
        <p class="text-secondary">将订阅导入 Clash，把流量转发到 ProxyCrab。</p>
      </div>
      <button
        class="switch" :class="{ on: state?.enabled }" type="button"
        :aria-pressed="state?.enabled ?? false"
        :aria-label="state?.enabled ? '关闭 Clash 订阅' : '开启 Clash 订阅'"
        :title="state?.enabled ? '关闭 Clash 订阅' : '开启 Clash 订阅'"
        :disabled="loading || mutating || !state || (!state.enabled && !canEnable)"
        @click="emit('toggle')"
      />
    </div>

    <section v-if="url" class="subscription-content">
      <div class="subscription-info">
        <h3>订阅链接</h3>
        <p class="text-secondary">其他设备需能访问所选 IP。扫描二维码或复制链接导入订阅。</p>
        <div class="subscription-link">
          <span class="mono">{{ url }}</span>
          <button class="btn icon" title="复制 Clash 订阅链接" @click="copyUrl"><Io5Copy :size="14" /></button>
        </div>
        <button class="btn primary import-button" @click="importClash"><Io5OpenOutline :size="14" />导入 Clash</button>
        <p class="text-secondary">导入后选择该订阅，并在 Clash 中开启系统代理。</p>
      </div>
      <div class="subscription-qr">
        <img v-if="qrCode" :src="qrCode" alt="Clash 订阅链接二维码" />
        <span v-else class="text-faint">二维码生成中…</span>
      </div>
    </section>
    <p v-else class="connection-empty text-secondary">{{ loading ? '正在读取订阅状态…' : mutating ? '正在更新订阅…' : '开启后生成订阅链接和二维码。' }}</p>

    <p v-if="state?.enabled && !proxyStore.running" class="connection-warning"><Io5Warning :size="14" />ProxyCrab 代理尚未启动，启动后才能转发流量。</p>
    <p class="connection-note text-secondary">关闭仅停止提供订阅，已导入的配置仍可使用。应用重启后恢复关闭；更换 IP 或重新开启后，请更新 Clash 中的订阅地址。</p>
  </div>
</template>

<style scoped>
.connection-root { display: flex; flex-direction: column; gap: var(--space-3); padding: var(--space-3); overflow: auto; min-height: 0; flex: 1; }
.connection-heading { display: flex; align-items: center; justify-content: space-between; gap: var(--space-3); }
h3 { margin: 0 0 4px; font-size: 13px; font-weight: 600; }
p { margin: 0; font-size: 12px; }
.subscription-content { display: grid; grid-template-columns: minmax(0, 1fr) 160px; align-items: center; gap: var(--space-3); padding: var(--space-3) 0; border-top: 1px solid var(--border); border-bottom: 1px solid var(--border); }
.subscription-info { min-width: 0; }
.subscription-link { display: flex; align-items: center; gap: 4px; margin-top: var(--space-2); padding: 4px 4px 4px 8px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--bg-panel); }
.subscription-link span { flex: 1; min-width: 0; overflow-wrap: anywhere; user-select: text; font-size: 11px; }
.subscription-link .btn { flex: none; }
.import-button { margin: var(--space-2) 0; }
.subscription-qr { width: 160px; height: 160px; display: grid; place-items: center; background: #fff; border: 1px solid var(--border); border-radius: var(--radius-md); overflow: hidden; font-size: 11px; }
.subscription-qr img { display: block; width: 158px; height: 158px; }
.connection-empty { padding: var(--space-3) 0; border-top: 1px solid var(--border); }
.connection-warning { display: flex; align-items: flex-start; gap: var(--space-1); color: var(--warning); }
.connection-warning svg { flex: none; margin-top: 2px; }
.connection-note { margin-top: auto; font-size: 11px; }
</style>
