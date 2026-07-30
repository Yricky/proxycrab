<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { Io5Warning } from "vue-icons-plus/io5";
import { useBackend } from "../api";
import type { AppConfig, HttpApiChange } from "../api/types";
import { appStore, reportError } from "../stores/app";
import { HTTP_API_CHANGE_EVENT } from "../stores/http-api-sync";
import { proxyStore } from "../stores/proxy";

const backend = useBackend();

const currentPath = ref("");
const configuredPath = ref("");
const config = ref<AppConfig | null>(null);
const proxyHost = ref("");
const proxyPort = ref(8080);
const httpServiceError = ref<string | null>(null);
const loadedConfiguredPath = ref("");
const loadedProxyHost = ref("");
const loadedProxyPort = ref(8080);
const externalChanged = ref(false);

const dirty = computed(
  () =>
    configuredPath.value !== loadedConfiguredPath.value ||
    proxyHost.value !== loadedProxyHost.value ||
    Number(proxyPort.value) !== loadedProxyPort.value,
);

async function refresh(): Promise<void> {
  try {
    const ws = await backend.getWorkspace();
    currentPath.value = ws.current_path;
    configuredPath.value = ws.configured_path;
    loadedConfiguredPath.value = ws.configured_path;
  } catch (error) {
    reportError(error, "获取工作区信息失败");
  }
  try {
    const cfg = await backend.getConfig();
    config.value = cfg;
    proxyHost.value = cfg.proxy_host;
    proxyPort.value = cfg.proxy_port;
    loadedProxyHost.value = cfg.proxy_host;
    loadedProxyPort.value = cfg.proxy_port;
  } catch (error) {
    reportError(error, "获取配置失败");
  }
  try {
    httpServiceError.value = await backend.getHttpServiceError();
  } catch (error) {
    reportError(error, "获取 HTTP 管理接口状态失败");
  }
}

function onHttpApiChange(event: Event): void {
  const { resources } = (event as CustomEvent<HttpApiChange>).detail;
  if (
    !resources.includes("all") &&
    !resources.includes("workspace") &&
    !resources.includes("config")
  ) {
    return;
  }
  if (dirty.value) {
    externalChanged.value = true;
    return;
  }
  externalChanged.value = false;
  void refresh();
}

function reloadExternal(): void {
  externalChanged.value = false;
  void refresh();
}

async function saveWorkspace(): Promise<void> {
  const path = configuredPath.value.trim();
  if (!path) {
    reportError("工作区路径不能为空");
    return;
  }
  try {
    const workspace = await backend.setWorkspaceForNextStart(path);
    configuredPath.value = workspace.configured_path;
    loadedConfiguredPath.value = workspace.configured_path;
    externalChanged.value = false;
    appStore.toast("已保存，重启后生效", "success");
  } catch (error) {
    reportError(error, "保存工作区失败");
  }
}

async function saveProxyListen(): Promise<void> {
  if (!config.value) return;
  const host = proxyHost.value.trim();
  const port = Number(proxyPort.value);
  if (!host) {
    reportError("监听地址不能为空");
    return;
  }
  if (!Number.isInteger(port) || port <= 0 || port > 65535) {
    reportError("端口必须是 1-65535 的整数");
    return;
  }
  try {
    const next: AppConfig = { ...config.value, proxy_host: host, proxy_port: port };
    config.value = await backend.replaceConfig(next);
    proxyHost.value = config.value.proxy_host;
    proxyPort.value = config.value.proxy_port;
    loadedProxyHost.value = config.value.proxy_host;
    loadedProxyPort.value = config.value.proxy_port;
    externalChanged.value = false;
    appStore.toast("已保存，需重启代理后生效", "success");
  } catch (error) {
    reportError(error, "保存代理配置失败");
  }
}

onMounted(() => {
  void refresh();
  window.addEventListener(HTTP_API_CHANGE_EVENT, onHttpApiChange);
});

onBeforeUnmount(() => {
  window.removeEventListener(HTTP_API_CHANGE_EVENT, onHttpApiChange);
});
</script>

<template>
  <div class="st-root">
    <div v-if="externalChanged" class="st-external">
      <span>设置已被外部修改，当前未保存输入仍保留。</span>
      <button class="btn compact" @click="reloadExternal">重新加载</button>
    </div>
    <section class="st-section">
      <h3 class="section-title">工作区</h3>
      <div class="st-field">
        <span class="st-label text-secondary">当前工作区</span>
        <span class="st-path mono">{{ currentPath || "—" }}</span>
      </div>
      <div class="st-field">
        <span class="st-label text-secondary">下次启动工作区</span>
        <div class="st-row">
          <input v-model="configuredPath" class="input st-input" placeholder="工作区目录路径" />
          <button class="btn primary" @click="saveWorkspace">保存</button>
        </div>
      </div>
    </section>

    <section class="st-section">
      <h3 class="section-title">代理监听</h3>
      <div v-if="proxyStore.running" class="st-hint">
        <Io5Warning :size="14" />
        <span>代理正在运行，修改监听配置前请先停止代理。</span>
      </div>
      <div class="st-row">
        <input v-model="proxyHost" class="input st-input" placeholder="监听地址，如 127.0.0.1" />
        <input
          v-model.number="proxyPort"
          type="number"
          class="input st-port"
          placeholder="端口"
          min="1"
          max="65535"
        />
        <button class="btn primary" :disabled="!config" @click="saveProxyListen">保存</button>
      </div>
    </section>

    <section class="st-section">
      <h3 class="section-title">HTTP 管理接口</h3>
      <div v-if="httpServiceError" class="st-warning">
        <Io5Warning :size="16" />
        <span class="st-warning-text">{{ httpServiceError }}</span>
      </div>
      <div v-else class="st-ok">
        <span class="st-dot" />
        <span>运行中</span>
        <span class="mono text-secondary">http://127.0.0.1:18089</span>
      </div>
    </section>
  </div>
</template>

<style scoped>
.st-root {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
  padding: var(--space-3);
}

.st-section {
  flex: none;
}
.st-external {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2);
  border: 1px solid var(--warning);
  border-radius: var(--radius-md);
  color: var(--warning);
  font-size: 12px;
}
.st-external span {
  flex: 1;
}

.st-field {
  margin-bottom: var(--space-2);
}

.st-label {
  display: block;
  font-size: 12px;
  margin-bottom: var(--space-1);
}

.st-path {
  display: block;
  word-break: break-all;
  user-select: text;
}

.st-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.st-input {
  flex: 1;
  min-width: 0;
}

.st-port {
  width: 90px;
}

.st-hint {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  color: var(--warning);
  font-size: 12px;
  margin-bottom: var(--space-2);
}

.st-warning {
  display: flex;
  align-items: flex-start;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border: 1px solid var(--warning);
  border-radius: var(--radius-md);
  color: var(--warning);
}

.st-warning-text {
  word-break: break-all;
  user-select: text;
}

.st-ok {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.st-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--success);
  flex: none;
}
</style>
