<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useBackend } from "../api";
import { logsStore } from "../stores/logs";
import { reportError } from "../stores/app";
import { Io5CaretDown, Io5Close, Io5Trash } from "vue-icons-plus/io5";

const backend = useBackend();
const history = ref<string[]>([]);
const historyOpen = ref(false);
const inputEl = ref<HTMLInputElement | null>(null);

onMounted(async () => {
  try {
    history.value = await backend.getFilterHistory();
  } catch (error) {
    reportError(error, "获取过滤历史失败");
  }
});

async function apply(script?: string): Promise<void> {
  if (script !== undefined) logsStore.filterScript = script;
  await logsStore.applyFilter(logsStore.filterScript);
  history.value = await backend.getFilterHistory();
  historyOpen.value = false;
}

async function clearFilter(): Promise<void> {
  logsStore.filterScript = "";
  await apply("");
  inputEl.value?.focus();
}

async function removeHistoryItem(item: string, event: MouseEvent): Promise<void> {
  event.stopPropagation();
  try {
    history.value = await backend.removeFilterHistory(item);
  } catch (error) {
    reportError(error, "删除历史失败");
  }
}

async function clearHistory(): Promise<void> {
  try {
    history.value = await backend.removeFilterHistory(null);
    historyOpen.value = false;
  } catch (error) {
    reportError(error, "清空历史失败");
  }
}

function toggleHistory(): void {
  historyOpen.value = !historyOpen.value;
}
</script>

<template>
  <div class="filter-bar">
    <div class="fb-input-wrap">
      <input
        ref="inputEl"
        v-model="logsStore.filterScript"
        class="input fb-input mono"
        :class="{ active: logsStore.filterActive }"
        placeholder="Lua 过滤脚本，回车应用，如：return entry.req.uri.host == &quot;example.com&quot;"
        spellcheck="false"
        @keyup.enter="apply()"
        @keyup.esc="clearFilter"
      />
      <button
        v-if="logsStore.filterScript"
        class="btn icon fb-clear"
        title="清除过滤"
        @click="clearFilter"
      >
        <Io5Close :size="13" />
      </button>
    </div>
    <div class="fb-history">
      <button class="btn icon" title="过滤历史" @click="toggleHistory()">
        <Io5CaretDown :size="14" />
      </button>
      <div v-if="historyOpen" class="fb-dropdown" @mousedown.prevent>
        <div v-if="history.length === 0" class="empty-hint">暂无历史记录</div>
        <div
          v-for="item in history"
          :key="item"
          class="fb-item mono"
          :class="{ current: item === logsStore.appliedFilter }"
          @click="apply(item)"
        >
          <span class="fb-item-text" :title="item">{{ item }}</span>
          <button class="btn icon" title="删除" @click="removeHistoryItem(item, $event)">
            <Io5Close :size="12" />
          </button>
        </div>
        <div v-if="history.length > 0" class="fb-footer">
          <button class="btn icon" title="清空历史" @click="clearHistory">
            <Io5Trash :size="13" />
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.filter-bar {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 8px 10px;
  flex: none;
}
.fb-input-wrap {
  position: relative;
  flex: 1;
  display: flex;
}
.fb-input {
  flex: 1;
  padding-right: 28px;
}
.fb-input.active {
  border-color: var(--accent);
  background: var(--bg-selected);
}
.fb-clear {
  position: absolute;
  right: 4px;
  top: 50%;
  transform: translateY(-50%);
}
.fb-history {
  position: relative;
}
.fb-dropdown {
  position: absolute;
  right: 0;
  top: calc(100% + 4px);
  width: 420px;
  max-height: 280px;
  overflow-y: auto;
  background: var(--bg-elevated);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-popup);
  z-index: 1000;
  padding: 4px;
}
.fb-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 5px 8px;
  border-radius: var(--radius-sm);
  cursor: pointer;
}
.fb-item:hover {
  background: var(--bg-hover);
}
.fb-item.current {
  background: var(--bg-selected);
}
.fb-item-text {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  user-select: text;
}
.fb-footer {
  display: flex;
  justify-content: flex-end;
  border-top: 1px solid var(--border);
  margin-top: 4px;
  padding-top: 4px;
}
</style>
