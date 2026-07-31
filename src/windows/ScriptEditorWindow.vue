<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useBackend } from "../api";
import type { HttpApiChange } from "../api/types";
import { appStore, reportError } from "../stores/app";
import { HTTP_API_CHANGE_EVENT } from "../stores/http-api-sync";
import { logsStore } from "../stores/logs";
import MonacoEditor from "../components/MonacoEditor.vue";
import { Io5Save } from "vue-icons-plus/io5";

const props = defineProps<{ kind: "column" | "request" | "response"; name: string }>();

const backend = useBackend();

const content = ref("");
const loaded = ref(false);
const saving = ref(false);
const dirty = ref(false);
const externalChanged = ref(false);
const externalMessage = ref("");

const kindLabel = computed(() =>
  props.kind === "column" ? "列脚本" : props.kind === "request" ? "请求拦截器" : "响应拦截器",
);

async function load(showError: boolean): Promise<void> {
  try {
    const script =
      props.kind === "column"
        ? await backend.getColumnScript(props.name)
        : await backend.getInterceptor(props.kind, props.name);
    content.value = script.content;
    loaded.value = true;
    dirty.value = false;
    externalChanged.value = false;
    externalMessage.value = "";
  } catch (error) {
    externalChanged.value = true;
    externalMessage.value = "脚本已被外部删除，请从管理器重新打开。";
    if (showError) reportError(error, "加载脚本失败");
  }
}

function onHttpApiChange(event: Event): void {
  const { resources } = (event as CustomEvent<HttpApiChange>).detail;
  const relevant =
    resources.includes("all") ||
    (props.kind === "column"
      ? resources.includes("column_scripts")
      : resources.includes("interceptors"));
  if (!relevant) return;
  if (dirty.value) {
    externalChanged.value = true;
    externalMessage.value = "脚本已被外部修改，未保存内容仍保留在编辑器中。";
    return;
  }
  void load(false);
}

onMounted(() => {
  void load(true);
  window.addEventListener("keydown", onKeydown);
  window.addEventListener(HTTP_API_CHANGE_EVENT, onHttpApiChange);
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKeydown);
  window.removeEventListener(HTTP_API_CHANGE_EVENT, onHttpApiChange);
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
      window.dispatchEvent(new CustomEvent("interceptors-changed"));
    }
    dirty.value = false;
    externalChanged.value = false;
    externalMessage.value = "";
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
    <div v-if="externalChanged" class="external-warning">
      <span>{{ externalMessage }}</span>
      <button class="btn compact" @click="load(false)">重新加载</button>
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
.external-warning {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid var(--warning);
  color: var(--warning);
  font-size: 12px;
  flex: none;
}
.external-warning span {
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
