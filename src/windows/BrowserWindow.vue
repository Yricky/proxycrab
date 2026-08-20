<script setup lang="ts">
import { computed, ref } from "vue";
import {
  Io5ArrowBack,
  Io5ArrowForward,
  Io5GlobeOutline,
  Io5OpenOutline,
  Io5Refresh,
} from "vue-icons-plus/io5";
import { openUrl } from "@tauri-apps/plugin-opener";

const props = defineProps<{ url: string }>();

/**
 * 自维护的导航历史栈。
 * 注意：跨域 iframe 无法读取 contentWindow.location，因此页面内部点击链接
 * 产生的跳转无法反映到地址栏，地址栏只跟踪从地址栏发起（或初始打开）的导航。
 */
const history = ref<string[]>([normalizeUrl(props.url)]);
const historyIndex = ref(0);
const addressInput = ref(history.value[0]);
/** 强制 iframe 重新挂载（刷新用）。 */
const frameKey = ref(0);

const currentUrl = computed(() => history.value[historyIndex.value]);
const canGoBack = computed(() => historyIndex.value > 0);
const canGoForward = computed(() => historyIndex.value < history.value.length - 1);

function normalizeUrl(input: string): string {
  const trimmed = input.trim();
  if (!trimmed) return "about:blank";
  return /^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(trimmed) ? trimmed : `https://${trimmed}`;
}

function navigate(input: string): void {
  const url = normalizeUrl(input);
  // 截断前进分支后追加新地址。
  history.value = [...history.value.slice(0, historyIndex.value + 1), url];
  historyIndex.value = history.value.length - 1;
  addressInput.value = url;
}

function goBack(): void {
  if (!canGoBack.value) return;
  historyIndex.value -= 1;
  addressInput.value = currentUrl.value;
}

function goForward(): void {
  if (!canGoForward.value) return;
  historyIndex.value += 1;
  addressInput.value = currentUrl.value;
}

function refresh(): void {
  frameKey.value += 1;
}

async function openExternal(): Promise<void> {
  await openUrl(currentUrl.value);
}

function onAddressKeydown(event: KeyboardEvent): void {
  if (event.key === "Enter") navigate(addressInput.value);
  // Escape 恢复为当前地址，丢弃未提交的输入。
  if (event.key === "Escape") addressInput.value = currentUrl.value;
}
</script>

<template>
  <div class="browser-root">
    <div class="browser-toolbar">
      <button
        class="nav-btn"
        :disabled="!canGoBack"
        title="后退"
        @click="goBack"
      >
        <Io5ArrowBack :size="14" />
      </button>
      <button
        class="nav-btn"
        :disabled="!canGoForward"
        title="前进"
        @click="goForward"
      >
        <Io5ArrowForward :size="14" />
      </button>
      <button class="nav-btn" title="刷新" @click="refresh">
        <Io5Refresh :size="14" />
      </button>
      <div class="address-bar">
        <Io5GlobeOutline class="address-icon" :size="12" />
        <input
          v-model="addressInput"
          class="address-input mono"
          type="text"
          spellcheck="false"
          placeholder="输入网址，回车访问"
          @keydown="onAddressKeydown"
        />
      </div>
      <button class="nav-btn" title="在系统浏览器中打开" @click="openExternal">
        <Io5OpenOutline :size="14" />
      </button>
    </div>
    <iframe
      :key="frameKey"
      class="browser-frame"
      :src="currentUrl"
      title="browser"
    />
  </div>
</template>

<style scoped>
.browser-root {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.browser-toolbar {
  flex: none;
  display: flex;
  align-items: center;
  gap: var(--space-1);
  padding: 4px 8px;
  border-bottom: 1px solid var(--border);
  background: var(--bg-panel);
}

.nav-btn {
  flex: none;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
}

.nav-btn:hover:not(:disabled) {
  background: var(--bg-hover);
  color: var(--text);
}

.nav-btn:disabled {
  color: var(--text-faint);
  opacity: 0.5;
  cursor: default;
}

.address-bar {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: 0 8px;
  height: 26px;
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  background: var(--bg-app);
  color: var(--text-faint);
}

.address-bar:focus-within {
  border-color: var(--accent);
}

.address-icon {
  flex: none;
}

.address-input {
  flex: 1;
  min-width: 0;
  border: 0;
  outline: none;
  background: transparent;
  color: var(--text);
  font-size: 12px;
}

.browser-frame {
  flex: 1;
  min-height: 0;
  width: 100%;
  border: 0;
  background: var(--bg-panel);
}
</style>
