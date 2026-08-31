<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { proxyStore } from "../stores/proxy";
import { appStore, type ThemeMode } from "../stores/app";
import { approvalsStore } from "../stores/approvals";
import { skillStore } from "../stores/skill";
import { useBackend } from "../api";
import AppTooltip from "./AppTooltip.vue";
import {
  openBase64,
  openAgentsPresets,
  openApprovals,
  openCertManager,
  openColumnManager,
  openFilterManager,
  openJwt,
  openRequestInterceptorManager,
  openResponseInterceptorManager,
  openRoutingManager,
  openSettings,
  openSkillInstall,
  openSystemLogs,
} from "../windows/launcher";
import { IoHandRight } from "vue-icons-plus/io";
import {
  Io5ArrowDown,
  Io5ArrowUp,
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
  Io5ShieldCheckmark,
  Io5Stop,
  Io5Flash,
  Io5Sunny,
  Io5Warning,
} from "vue-icons-plus/io5";

type ToolbarMenu = "ai" | "scripts" | "tools" | "system";

const backend = useBackend();

const aiTooltipTitle = computed(() => {
  if (skillStore.status === "not_installed") return "skill未安装";
  if (skillStore.status === "mismatched") return "本机skill与当前应用版本不一致";
  return undefined;
});
const aiTooltipDetail = computed(() =>
  skillStore.status === "mismatched" ? "可能导致预期外的行为，建议重新安装" : undefined,
);

const toolbarMenus = ref<HTMLElement | null>(null);
const activeMenu = ref<ToolbarMenu | null>(null);
const ipSegment = ref<HTMLElement | null>(null);
const ipMenuOpen = ref(false);

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

function toggleIpMenu(): void {
  ipMenuOpen.value = !ipMenuOpen.value;
  if (ipMenuOpen.value) {
    void proxyStore.refreshLocalIps();
  }
}

function onPortInput(event: Event): void {
  proxyStore.setPortText((event.target as HTMLInputElement).value);
}

function onToggle(): void {
  if (proxyStore.canToggle) {
    void proxyStore.toggle();
  }
}

function onDocumentPointerDown(event: PointerEvent): void {
  const target = event.target as Node;
  if (!toolbarMenus.value?.contains(target)) {
    activeMenu.value = null;
  }
  if (ipMenuOpen.value && !ipSegment.value?.contains(target)) {
    ipMenuOpen.value = false;
  }
}

function onDocumentKeydown(event: KeyboardEvent): void {
  if (event.key !== "Escape") return;
  activeMenu.value = null;
  ipMenuOpen.value = false;
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
      <div
        class="proxy-control"
        :class="
          proxyStore.running ? 'running' : proxyStore.canToggle ? 'ready' : 'off'
        "
      >
        <div ref="ipSegment" class="proxy-seg proxy-ip">
          <button
            class="proxy-ip-trigger"
            :aria-expanded="ipMenuOpen"
            :title="'查看本机 IP'"
            @click="toggleIpMenu"
          >
            <span class="mono">{{ proxyStore.displayIp }}</span>
            <Io5ChevronDown :size="11" />
          </button>
          <div v-if="ipMenuOpen" class="proxy-ip-menu">
            <div v-for="ip in proxyStore.ipOptions" :key="ip" class="proxy-ip-item">
              <span class="mono">{{ ip }}</span>
            </div>
          </div>
        </div>
        <div class="proxy-seg">
          <input
            class="proxy-port-input"
            :value="proxyStore.portText"
            :disabled="proxyStore.running || proxyStore.busy || !backend.host"
            maxlength="5"
            spellcheck="false"
            autocomplete="off"
            :title="'监听端口（1-65535）'"
            @input="onPortInput"
            @keydown.enter="onToggle"
          />
        </div>
        <div class="proxy-seg proxy-btn-seg">
          <button
            class="proxy-toggle-btn"
            :class="
              proxyStore.running ? 'running' : proxyStore.canToggle ? 'ready' : 'off'
            "
            :disabled="!proxyStore.canToggle"
            :title="proxyStore.running ? '停止代理' : '启动代理'"
            @click="onToggle"
          >
            <Io5Stop v-if="proxyStore.running" :size="14" />
            <Io5Play v-else :size="14" />
          </button>
        </div>
      </div>
    </div>

    <div class="tb-spacer" />

    <div ref="toolbarMenus" class="tb-group tb-menus">
      <button
        v-if="approvalsStore.count > 0"
        class="approval-trigger"
        :title="`${approvalsStore.count} 个管理接口请求待审批`"
        aria-label="打开管理接口审批"
        @click="openApprovals"
      >
        <IoHandRight :size="16" class="approval-hand" />
        <span>{{ approvalsStore.count }}</span>
      </button>
      <div class="tb-menu-wrap">
        <AppTooltip :title="aiTooltipTitle" :detail="aiTooltipDetail" immediate>
          <button
            class="tb-menu-trigger"
            :class="{
              active: activeMenu === 'ai',
              'skill-missing': skillStore.status === 'not_installed',
              'skill-mismatched': skillStore.status === 'mismatched',
            }"
            :aria-expanded="activeMenu === 'ai'"
            @click="toggleMenu('ai')"
          >
            <Io5Warning v-if="skillStore.status === 'mismatched'" :size="13" />
            AI
            <Io5ChevronDown :size="11" />
          </button>
        </AppTooltip>
        <div v-if="activeMenu === 'ai'" class="tb-menu">
          <button class="tb-menu-item" @click="runMenuAction(openAgentsPresets)">
            <Io5DocumentText :size="14" />
            <span>AGENTS.md 预设…</span>
          </button>
          <button
            v-if="backend.host?.skillInstaller"
            class="tb-menu-item"
            @click="runMenuAction(openSkillInstall)"
          >
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
          <button class="tb-menu-item" @click="runMenuAction(openRequestInterceptorManager)">
            <Io5ArrowDown :size="14" />
            <span>请求拦截器</span>
          </button>
          <button class="tb-menu-item" @click="runMenuAction(openResponseInterceptorManager)">
            <Io5ArrowUp :size="14" />
            <span>响应拦截器</span>
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
          <button class="tb-menu-item" @click="runMenuAction(openJwt)">
            <Io5ShieldCheckmark :size="14" />
            <span>JWT 解码 / 验签</span>
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
.approval-trigger {
  align-self: center;
  height: 24px;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  margin-right: 3px;
  padding: 0 7px;
  border: 0;
  border-radius: var(--radius-sm);
  background: color-mix(in srgb, var(--warning) 13%, transparent);
  color: var(--warning);
  font: inherit;
  font-size: 11px;
  font-weight: 700;
  cursor: pointer;
}
.approval-trigger:hover { background: color-mix(in srgb, var(--warning) 21%, transparent); }
.approval-hand { transform-origin: 50% 80%; animation: approval-wave 0.8s ease-in-out infinite alternate; }
@keyframes approval-wave {
  from { transform: rotate(-30deg); }
  to { transform: rotate(30deg); }
}
@media (prefers-reduced-motion: reduce) {
  .approval-hand { animation: none; }
}
.tb-menu-wrap {
  position: relative;
  display: flex;
  align-items: center;
}
.tb-menu-trigger {
  height: 24px;
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
.tb-menu-trigger.skill-missing,
.tb-menu-trigger.skill-missing:hover,
.tb-menu-trigger.skill-missing.active {
  background: var(--warning);
  color: #fff;
}
.tb-menu-trigger.skill-mismatched,
.tb-menu-trigger.skill-mismatched:hover,
.tb-menu-trigger.skill-mismatched.active {
  background: var(--danger);
  color: #fff;
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

/* ---------- proxy start/stop control ---------- */

.proxy-control {
  display: flex;
  align-items: stretch;
  height: 26px;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--bg-panel);
  transition: border-color 0.12s;
}
/* Border follows the start/stop button state. */
.proxy-control.ready {
  border-color: var(--accent);
}
.proxy-control.running {
  border-color: var(--danger);
}
.proxy-control.off {
  border-color: var(--border);
}
.proxy-seg {
  display: flex;
  align-items: center;
}
.proxy-seg + .proxy-seg {
  border-left: 1px solid var(--border);
}
.proxy-ip {
  position: relative;
}
.proxy-ip-trigger {
  height: 100%;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 0 7px 0 10px;
  border: none;
  background: transparent;
  color: var(--text);
  font: inherit;
  cursor: pointer;
  border-radius: calc(var(--radius-md) - 1px) 0 0 calc(var(--radius-md) - 1px);
  transition: background 0.12s;
}
.proxy-ip-trigger:hover:not(:disabled) {
  background: var(--bg-hover);
}
.proxy-ip-trigger:disabled {
  cursor: default;
}
.proxy-ip-trigger svg {
  flex: none;
  color: var(--text-secondary);
}
.proxy-port-input {
  width: 58px;
  height: 100%;
  padding: 0 8px;
  border: none;
  outline: none;
  background: transparent;
  color: var(--text);
  font-family: var(--font-mono);
  font-size: 12px;
  text-align: center;
}
.proxy-port-input:focus {
  background: var(--bg-hover);
}
.proxy-port-input:disabled {
  cursor: default;
}
.proxy-btn-seg {
  padding: 0;
}
.proxy-toggle-btn {
  width: 24px;
  height: 100%;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: 0 calc(var(--radius-md) - 1px) calc(var(--radius-md) - 1px) 0;
  color: #fff;
  cursor: pointer;
  transition: background 0.12s;
}
.proxy-toggle-btn.ready {
  background: var(--accent);
}
.proxy-toggle-btn.ready:hover {
  background: var(--accent-hover);
}
.proxy-toggle-btn.running {
  background: var(--danger);
}
.proxy-toggle-btn.running:hover {
  background: var(--danger-hover);
}
.proxy-toggle-btn.off {
  background: var(--bg-active);
  color: var(--text-faint);
  cursor: not-allowed;
}

.proxy-ip-menu {
  position: absolute;
  z-index: 100004;
  top: calc(100% + 4px);
  left: 0;
  min-width: 150px;
  max-height: 260px;
  overflow-y: auto;
  padding: 5px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-popup);
}
.proxy-ip-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 5px 9px;
  border-radius: var(--radius-sm);
  color: var(--text);
  font: inherit;
  font-size: 12px;
  cursor: default;
}
</style>
