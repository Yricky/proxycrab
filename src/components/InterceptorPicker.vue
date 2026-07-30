<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import { Io5Close, Io5Search } from "vue-icons-plus/io5";
import { useBackend } from "../api";
import type { InterceptorKind, InterceptorLibraryItem } from "../api/types";
import { reportError } from "../stores/app";

const props = defineProps<{
  visible: boolean;
  kind: InterceptorKind;
  usedNames: string[];
  atCapacity: boolean;
  x: number;
  y: number;
}>();

const emit = defineEmits<{
  close: [];
  select: [name: string];
}>();

const backend = useBackend();
const items = ref<InterceptorLibraryItem[]>([]);
const query = ref("");
const loading = ref(false);
const input = ref<HTMLInputElement | null>(null);

const available = computed(() => {
  const used = new Set(props.usedNames);
  const needle = query.value.trim().toLocaleLowerCase();
  return items.value.filter(
    (item) =>
      !used.has(item.name) && (!needle || item.name.toLocaleLowerCase().includes(needle)),
  );
});

const pickerStyle = computed(() => ({
  left: `${Math.max(8, Math.min(props.x, window.innerWidth - 292))}px`,
  top: `${Math.max(8, Math.min(props.y, window.innerHeight - 342))}px`,
}));

async function load(): Promise<void> {
  loading.value = true;
  try {
    items.value = (await backend.listInterceptors(props.kind)).items;
  } catch (error) {
    reportError(error, "加载拦截器脚本失败");
  } finally {
    loading.value = false;
  }
}

function choose(name: string): void {
  if (!props.atCapacity) emit("select", name);
}

function onKeydown(event: KeyboardEvent): void {
  if (event.key === "Escape") emit("close");
  if (event.key === "Enter" && available.value.length === 1) choose(available.value[0].name);
}

watch(
  () => [props.visible, props.kind] as const,
  async ([visible]) => {
    if (!visible) return;
    query.value = "";
    await load();
    await nextTick();
    input.value?.focus();
  },
  { immediate: true },
);
</script>

<template>
  <Teleport to="body">
    <div v-if="visible" class="picker-mask" @mousedown.self="emit('close')" @keydown="onKeydown">
      <div class="interceptor-picker" :style="pickerStyle">
        <div class="picker-head">
          <span>{{ kind === "request" ? "添加请求拦截器" : "添加响应拦截器" }}</span>
          <button class="btn icon" title="关闭" @click="emit('close')">
            <Io5Close :size="13" />
          </button>
        </div>
        <label class="picker-search">
          <Io5Search :size="14" />
          <input
            ref="input"
            v-model="query"
            placeholder="搜索全局脚本"
            spellcheck="false"
          />
        </label>
        <div class="picker-list">
          <div v-if="atCapacity" class="picker-state">本侧已达到 12 个拦截器上限</div>
          <div v-else-if="loading" class="picker-state">加载中…</div>
          <div v-else-if="available.length === 0" class="picker-state">
            {{ query ? "没有匹配的可用脚本" : "没有可添加的脚本" }}
          </div>
          <button
            v-for="item in available"
            v-else
            :key="item.name"
            class="picker-item"
            @click="choose(item.name)"
          >
            <span class="mono">{{ item.name }}</span>
            <span class="text-faint">{{ item.usage_count }} 个会话使用</span>
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.picker-mask {
  position: fixed;
  inset: 0;
  z-index: 100003;
}
.interceptor-picker {
  position: fixed;
  width: 284px;
  max-height: 334px;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-lg);
  background: var(--bg-elevated);
  box-shadow: var(--shadow-popup);
  animation: picker-in 0.14s ease-out;
}
.picker-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 7px 8px 5px 11px;
  font-size: 12px;
  font-weight: 600;
}
.picker-search {
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 0 8px 6px;
  padding: 5px 8px;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  color: var(--text-faint);
  background: var(--bg-panel);
}
.picker-search:focus-within {
  border-color: var(--accent);
}
.picker-search input {
  flex: 1;
  min-width: 0;
  padding: 0;
  border: 0;
  outline: 0;
  background: transparent;
  color: var(--text);
  font: inherit;
}
.picker-list {
  min-height: 72px;
  overflow-y: auto;
  padding: 3px 5px 6px;
  border-top: 1px solid var(--border);
}
.picker-item {
  width: 100%;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 7px 8px;
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text);
  cursor: pointer;
  text-align: left;
}
.picker-item:hover {
  background: var(--bg-hover);
}
.picker-item .mono {
  overflow: hidden;
  text-overflow: ellipsis;
}
.picker-item .text-faint {
  flex: none;
  font-size: 10px;
}
.picker-state {
  padding: 20px 10px;
  color: var(--text-faint);
  text-align: center;
  font-size: 12px;
}
@keyframes picker-in {
  from {
    opacity: 0;
    transform: translateY(-3px);
  }
}
</style>
