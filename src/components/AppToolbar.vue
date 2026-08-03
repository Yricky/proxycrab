<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";
import { proxyStore } from "../stores/proxy";
import { appStore, type ThemeMode } from "../stores/app";
import {
  openBase64,
  openAgentsPresets,
  openCertManager,
  openColumnManager,
  openFilterManager,
  openInterceptorManager,
  openRoutingManager,
  openSettings,
  openSkillInstall,
  openSystemLogs,
} from "../windows/launcher";
import {
  Io5Build,
  Io5Checkmark,
  Io5ChevronDown,
  Io5CodeSlash,
  Io5Desktop,
  Io5DocumentText,
  Io5Download,
  Io5Key,
  Io5List,
  Io5Moon,
  Io5Newspaper,
  Io5Play,
  Io5RadioButtonOn,
  Io5Settings,
  Io5Stop,
  Io5Flash,
  Io5Sunny,
} from "vue-icons-plus/io5";

type ToolbarMenu = "ai" | "scripts" | "tools" | "system";

const toolbarMenus = ref<HTMLElement | null>(null);
const activeMenu = ref<ToolbarMenu | null>(null);

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

function toggleMenu(menu: ToolbarMenu): void {
  activeMenu.value = activeMenu.value === menu ? null : menu;
}

function runMenuAction(action: () => void): void {
  activeMenu.value = null;
  action();
}

function setTheme(theme: ThemeMode): void {
  activeMenu.value = null;
  appStore.setTheme(theme);
}

function onDocumentPointerDown(event: PointerEvent): void {
  if (!toolbarMenus.value?.contains(event.target as Node)) {
    activeMenu.value = null;
  }
}

function onDocumentKeydown(event: KeyboardEvent): void {
  if (event.key === "Escape") activeMenu.value = null;
}

onMounted(() => {
  document.addEventListener("pointerdown", onDocumentPointerDown);
  document.addEventListener("keydown", onDocumentKeydown);
});

onBeforeUnmount(() => {
  document.removeEventListener("pointerdown", onDocumentPointerDown);
  document.removeEventListener("keydown", onDocumentKeydown);
});
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

    <div ref="toolbarMenus" class="tb-group tb-menus">
      <div class="tb-menu-wrap">
        <button
          class="tb-menu-trigger"
          :class="{ active: activeMenu === 'ai' }"
          :aria-expanded="activeMenu === 'ai'"
          @click="toggleMenu('ai')"
        >
          AI
          <Io5ChevronDown :size="11" />
        </button>
        <div v-if="activeMenu === 'ai'" class="tb-menu">
          <button class="tb-menu-item" @click="runMenuAction(openAgentsPresets)">
            <Io5DocumentText :size="14" />
            <span>AGENTS.md 预设…</span>
          </button>
          <button class="tb-menu-item" @click="runMenuAction(openSkillInstall)">
            <Io5Download :size="14" />
            <span>安装 ProxyCrab Skill…</span>
          </button>
        </div>
      </div>

      <div class="tb-menu-wrap">
        <button
          class="tb-menu-trigger"
          :class="{ active: activeMenu === 'scripts' }"
          :aria-expanded="activeMenu === 'scripts'"
          @click="toggleMenu('scripts')"
        >
          脚本
          <Io5ChevronDown :size="12" />
        </button>
        <div v-if="activeMenu === 'scripts'" class="tb-menu">
          <button class="tb-menu-item" @click="runMenuAction(openInterceptorManager)">
            <Io5Flash :size="14" />
            <span>拦截器</span>
          </button>
          <button class="tb-menu-item" @click="runMenuAction(openColumnManager)">
            <Io5List :size="14" />
            <span>自定义列</span>
          </button>
          <button class="tb-menu-item" @click="runMenuAction(openFilterManager)">
            <Io5CodeSlash :size="14" />
            <span>过滤脚本</span>
          </button>
          <button class="tb-menu-item" @click="runMenuAction(openRoutingManager)">
            <Io5RadioButtonOn :size="14" />
            <span>分流规则</span>
          </button>
        </div>
      </div>

      <div class="tb-menu-wrap">
        <button
          class="tb-menu-trigger"
          :class="{ active: activeMenu === 'tools' }"
          :aria-expanded="activeMenu === 'tools'"
          @click="toggleMenu('tools')"
        >
          小工具
          <Io5ChevronDown :size="12" />
        </button>
        <div v-if="activeMenu === 'tools'" class="tb-menu">
          <button class="tb-menu-item" @click="runMenuAction(openBase64)">
            <Io5Build :size="14" />
            <span>Base64 编解码</span>
          </button>
        </div>
      </div>

      <div class="tb-menu-wrap">
        <button
          class="tb-menu-trigger"
          :class="{ active: activeMenu === 'system' }"
          :aria-expanded="activeMenu === 'system'"
          @click="toggleMenu('system')"
        >
          系统
          <Io5ChevronDown :size="12" />
        </button>
        <div v-if="activeMenu === 'system'" class="tb-menu tb-menu-system">
          <button class="tb-menu-item" @click="runMenuAction(openCertManager)">
            <Io5Key :size="14" />
            <span>证书管理</span>
          </button>
          <button class="tb-menu-item" @click="runMenuAction(openSystemLogs)">
            <Io5Newspaper :size="14" />
            <span>系统日志</span>
          </button>
          <button class="tb-menu-item" @click="runMenuAction(openSettings)">
            <Io5Settings :size="14" />
            <span>设置</span>
          </button>
          <div class="tb-menu-divider" />
          <button class="tb-menu-item" @click="setTheme('light')">
            <Io5Sunny :size="14" />
            <span>浅色</span>
            <Io5Checkmark v-if="appStore.theme === 'light'" :size="14" class="tb-menu-check" />
          </button>
          <button class="tb-menu-item" @click="setTheme('dark')">
            <Io5Moon :size="14" />
            <span>深色</span>
            <Io5Checkmark v-if="appStore.theme === 'dark'" :size="14" class="tb-menu-check" />
          </button>
          <button class="tb-menu-item" @click="setTheme('system')">
            <Io5Desktop :size="14" />
            <span>跟随系统</span>
            <Io5Checkmark v-if="appStore.theme === 'system'" :size="14" class="tb-menu-check" />
          </button>
        </div>
      </div>
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
.tb-menus {
  align-self: stretch;
  gap: 2px;
}
.tb-menu-wrap {
  position: relative;
  display: flex;
  align-items: center;
}
.tb-menu-trigger {
  height: 28px;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 0 8px;
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-secondary);
  font: inherit;
  font-size: 12px;
  cursor: default;
  transition:
    color 0.12s,
    background 0.12s;
}
.tb-menu-trigger:hover,
.tb-menu-trigger.active {
  color: var(--text);
  background: var(--bg-hover);
}
.tb-menu {
  position: absolute;
  z-index: 100003;
  top: calc(100% - 2px);
  right: 0;
  min-width: 168px;
  padding: 5px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-popup);
}
.tb-menu-system {
  min-width: 176px;
}
.tb-menu-item {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 9px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text);
  font: inherit;
  font-size: 12px;
  text-align: left;
  cursor: pointer;
}
.tb-menu-item:hover {
  background: var(--bg-hover);
}
.tb-menu-item > svg {
  flex: none;
  color: var(--text-secondary);
}
.tb-menu-item > span {
  flex: 1;
}
.tb-menu-item .tb-menu-check {
  color: var(--accent);
}
.tb-menu-divider {
  height: 1px;
  margin: 5px 4px;
  background: var(--border);
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
