<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useBackend } from "../api";
import { appStore, reportError } from "../stores/app";
import { logsStore } from "../stores/logs";
import MonacoEditor from "../components/MonacoEditor.vue";
import { Io5Save } from "vue-icons-plus/io5";

const props = defineProps<{ kind: "column" | "request" | "response"; name: string }>();

const backend = useBackend();

const content = ref("");
const loaded = ref(false);
const saving = ref(false);
const dirty = ref(false);

const kindLabel = computed(() =>
  props.kind === "column" ? "列脚本" : props.kind === "request" ? "请求拦截器" : "响应拦截器",
);

onMounted(async () => {
  try {
    const script =
      props.kind === "column"
        ? await backend.getColumnScript(props.name)
        : await backend.getInterceptor(props.kind, props.name);
    content.value = script.content;
    loaded.value = true;
  } catch (error) {
    reportError(error, "加载脚本失败");
  }
  window.addEventListener("keydown", onKeydown);
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKeydown);
});

function onContentChange(value: string): void {
  content.value = value;
  dirty.value = true;
}

async function save(): Promise<void> {
  if (saving.value || !loaded.value) return;
  saving.value = true;
  try {
    if (props.kind === "column") {
      await backend.updateColumnScript(props.name, { content: content.value });
      await logsStore.refreshView();
    } else {
      await backend.updateInterceptor(props.kind, props.name, { content: content.value });
    }
    dirty.value = false;
    appStore.toast("已保存", "success");
  } catch (error) {
    // 后端会做 Lua 语法检查，错误信息原样展示
    reportError(error, "保存失败");
  } finally {
    saving.value = false;
  }
}

function onKeydown(event: KeyboardEvent): void {
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "s") {
    event.preventDefault();
    save();
  }
}
</script>

<template>
  <div class="script-editor">
    <div class="toolbar">
      <span class="script-name mono" :title="name">{{ name }}</span>
      <span class="badge">{{ kindLabel }}</span>
      <span class="toolbar-spacer" />
      <button class="btn primary" :disabled="saving || !loaded" @click="save">
        <Io5Save :size="14" />
        {{ saving ? "保存中…" : "保存" }}
      </button>
    </div>
    <div v-if="!loaded" class="empty-hint">加载中…</div>
    <MonacoEditor
      v-else
      :model-value="content"
      language="lua"
      @update:model-value="onContentChange"
    />
    <div class="statusbar text-faint">
      <span>Lua 5.4 沙箱 · 10 万指令上限 · 16 MiB 内存上限</span>
      <span v-if="dirty" class="dirty-flag">未保存</span>
    </div>
  </div>
</template>

<style scoped>
.script-editor {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.toolbar {
  flex: none;
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: 8px 12px;
  border-bottom: 1px solid var(--border);
}
.script-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 600;
}
.toolbar-spacer {
  flex: 1;
}

.statusbar {
  flex: none;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-2);
  padding: 4px 12px;
  border-top: 1px solid var(--border);
  font-size: 11px;
}
.dirty-flag {
  color: var(--warning);
  font-weight: 600;
}
</style>
