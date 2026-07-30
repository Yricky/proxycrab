<script setup lang="ts">
import { appStore, type ThemeMode } from "../stores/app";
import { Io5Desktop, Io5Moon, Io5Sunny } from "vue-icons-plus/io5";

const modes: { value: ThemeMode; label: string; icon: unknown }[] = [
  { value: "light", label: "浅色", icon: Io5Sunny },
  { value: "dark", label: "深色", icon: Io5Moon },
  { value: "system", label: "跟随系统", icon: Io5Desktop },
];
</script>

<template>
  <div class="theme-toggle">
    <button
      v-for="mode in modes"
      :key="mode.value"
      class="tt-btn"
      :class="{ active: appStore.theme === mode.value }"
      :title="mode.label"
      @click="appStore.setTheme(mode.value)"
    >
      <component :is="mode.icon" :size="14" />
    </button>
  </div>
</template>

<style scoped>
.theme-toggle {
  display: inline-flex;
  padding: 2px;
  gap: 2px;
  background: var(--bg-active);
  border-radius: var(--radius-md);
}
.tt-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 22px;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
}
.tt-btn:hover {
  color: var(--text);
}
.tt-btn.active {
  background: var(--bg-panel);
  color: var(--accent);
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.15);
}
</style>
