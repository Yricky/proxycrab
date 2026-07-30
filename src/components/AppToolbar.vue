<script setup lang="ts">
import { proxyStore } from "../stores/proxy";
import ThemeToggle from "./ThemeToggle.vue";
import {
  openBase64,
  openCertManager,
  openColumnManager,
  openInterceptorManager,
  openSettings,
  openSystemLogs,
} from "../windows/launcher";
import {
  Io5Build,
  Io5Key,
  Io5List,
  Io5Newspaper,
  Io5Play,
  Io5Settings,
  Io5Stop,
  Io5Flash,
} from "vue-icons-plus/io5";

function statusClass(status: string): string {
  switch (status) {
    case "running":
      return "ok";
    case "starting":
    case "stopping":
      return "warn";
    case "failed":
      return "err";
    default:
      return "";
  }
}
</script>

<template>
  <header class="toolbar">
    <div class="tb-group">
      <span class="tb-brand">
        <Io5Flash :size="15" class="brand-icon" />
        ProxyCrab
      </span>
    </div>

    <div class="tb-group tb-proxy">
      <button
        class="btn"
        :class="proxyStore.running ? 'danger' : 'primary'"
        :disabled="proxyStore.busy"
        @click="proxyStore.toggle()"
      >
        <Io5Stop v-if="proxyStore.running" :size="13" />
        <Io5Play v-else :size="13" />
        {{ proxyStore.running ? "停止代理" : "启动代理" }}
      </button>
      <span class="proxy-status" :class="statusClass(proxyStore.status.status)">
        <span class="dot" />
        {{ proxyStore.label }}
      </span>
    </div>

    <div class="tb-spacer" />

    <div class="tb-group">
      <button class="btn icon" title="拦截器" @click="openInterceptorManager()">
        <Io5Flash :size="16" />
      </button>
      <button class="btn icon" title="自定义列" @click="openColumnManager()">
        <Io5List :size="16" />
      </button>
      <button class="btn icon" title="证书管理" @click="openCertManager()">
        <Io5Key :size="16" />
      </button>
      <button class="btn icon" title="Base64 编解码" @click="openBase64()">
        <Io5Build :size="16" />
      </button>
      <button class="btn icon" title="系统日志" @click="openSystemLogs()">
        <Io5Newspaper :size="16" />
      </button>
      <button class="btn icon" title="设置" @click="openSettings()">
        <Io5Settings :size="16" />
      </button>
      <ThemeToggle />
    </div>
  </header>
</template>

<style scoped>
.toolbar {
  height: var(--toolbar-height);
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 0 12px;
  background: var(--bg-panel);
  border-bottom: 1px solid var(--border);
  flex: none;
}
.tb-group {
  display: flex;
  align-items: center;
  gap: 8px;
}
.tb-brand {
  display: flex;
  align-items: center;
  gap: 6px;
  font-weight: 700;
  font-size: 14px;
}
.brand-icon {
  color: var(--accent);
}
.tb-proxy {
  gap: 10px;
}
.tb-spacer {
  flex: 1;
}
.proxy-status {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--text-secondary);
  font-family: var(--font-mono);
}
.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--text-faint);
}
.proxy-status.ok .dot {
  background: var(--success);
}
.proxy-status.ok {
  color: var(--success);
}
.proxy-status.warn .dot {
  background: var(--warning);
}
.proxy-status.warn {
  color: var(--warning);
}
.proxy-status.err .dot {
  background: var(--danger);
}
.proxy-status.err {
  color: var(--danger);
}
</style>
