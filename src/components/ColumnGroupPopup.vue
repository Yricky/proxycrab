<script setup lang="ts">
import { Io5ArrowDown, Io5ArrowUp, Io5Search } from "vue-icons-plus/io5";
import type { ColumnGroupCache } from "../stores/column-groups";
import type { ColumnGroupEntry, ColumnGroupSort } from "../utils/column-groups";

defineProps<{
  cache: ColumnGroupCache;
  entries: ColumnGroupEntry[];
  currentInput: string;
}>();

const emit = defineEmits<{
  select: [entry: ColumnGroupEntry];
  "update:search": [value: string];
  "update:sort": [value: ColumnGroupSort];
  "toggle-direction": [];
}>();

function updateSearch(event: Event): void {
  emit("update:search", (event.target as HTMLInputElement).value);
}

function updateSort(event: Event): void {
  emit("update:sort", (event.target as HTMLSelectElement).value as ColumnGroupSort);
}
</script>

<template>
  <div class="cgp-popup">
    <div class="cgp-controls">
      <label class="cgp-search">
        <Io5Search :size="13" />
        <input
          class="input mono"
          :value="cache.search"
          placeholder="过滤分组值"
          spellcheck="false"
          @input="updateSearch"
        />
      </label>
      <select class="input cgp-sort" :value="cache.sort" @change="updateSort">
        <option value="count">数量</option>
        <option value="value">字典序</option>
      </select>
      <button
        class="btn icon cgp-direction"
        :title="cache.descending ? '降序' : '升序'"
        @click="emit('toggle-direction')"
      >
        <Io5ArrowDown v-if="cache.descending" :size="13" />
        <Io5ArrowUp v-else :size="13" />
      </button>
    </div>

    <div v-if="cache.loading" class="cgp-progress" aria-label="正在计算分组">
      <span />
    </div>

    <div class="cgp-list">
      <button
        v-for="entry in entries"
        :key="entry.key"
        class="cgp-item"
        :class="{ selected: entry.exactPattern === currentInput }"
        :disabled="entry.exactPattern === null"
        @click="emit('select', entry)"
      >
        <span class="cgp-value mono" :title="entry.label">{{ entry.label }}</span>
        <span class="cgp-count mono">{{ entry.count }}</span>
      </button>
      <div v-if="entries.length === 0 && !cache.loading" class="cgp-empty">
        暂无匹配分组
      </div>
    </div>
  </div>
</template>

<style scoped>
.cgp-popup {
  position: absolute;
  left: 0;
  top: calc(100% + 4px);
  width: 340px;
  overflow: hidden;
  background: var(--bg-elevated);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-popup);
  z-index: 1000;
}
.cgp-controls {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 76px 28px;
  align-items: center;
  gap: 5px;
  padding: 6px;
}
.cgp-search {
  position: relative;
  min-width: 0;
  color: var(--text-faint);
}
.cgp-search svg {
  position: absolute;
  left: 8px;
  top: 50%;
  transform: translateY(-50%);
  pointer-events: none;
}
.cgp-search input {
  width: 100%;
  height: 27px;
  padding-left: 26px;
}
.cgp-sort {
  height: 27px;
  padding: 0 5px;
}
.cgp-direction {
  width: 28px;
  height: 27px;
}
.cgp-progress {
  height: 2px;
  overflow: hidden;
  background: color-mix(in srgb, var(--accent) 16%, transparent);
}
.cgp-progress span {
  display: block;
  width: 42%;
  height: 100%;
  background: var(--accent);
  animation: cgp-loading 1s ease-in-out infinite;
}
.cgp-list {
  max-height: min(390px, calc(100vh - 205px));
  overflow-y: auto;
  padding: 4px;
  border-top: 1px solid var(--border);
}
.cgp-item {
  width: 100%;
  min-height: 30px;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 4px 8px;
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text);
  font: inherit;
  text-align: left;
  cursor: pointer;
}
.cgp-item:hover:not(:disabled) {
  background: var(--bg-hover);
}
.cgp-item.selected {
  background: var(--bg-selected);
  color: var(--accent);
}
.cgp-item:disabled {
  color: var(--text-faint);
  cursor: default;
}
.cgp-value {
  min-width: 0;
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.cgp-count {
  flex: none;
  color: var(--text-faint);
}
.cgp-empty {
  padding: 18px 8px;
  color: var(--text-faint);
  text-align: center;
  font-size: 11px;
}
@keyframes cgp-loading {
  from {
    transform: translateX(-110%);
  }
  to {
    transform: translateX(340%);
  }
}
@media (prefers-reduced-motion: reduce) {
  .cgp-progress span {
    animation-duration: 2s;
  }
}
</style>
