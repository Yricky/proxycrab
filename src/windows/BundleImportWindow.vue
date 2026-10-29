<script setup lang="ts">
import { computed, ref } from "vue";
import { useBackend } from "../api";
import type { BundleImportResult, BundleResolution } from "../api/types";
import { sessionsStore } from "../stores/sessions";
import { HTTP_API_CHANGE_EVENT } from "../stores/http-api-sync";
import { parseBundle } from "../utils/bundle";

const props = defineProps<{ file: File; preview: ReturnType<typeof parseBundle> }>();
const backend = useBackend();
const selected = ref<string[]>([]);
const result = ref<BundleImportResult | null>(null);
const choices = ref<Record<string, BundleResolution>>({});
const busy = ref(false),
  error = ref("");
const conflicts = computed(() => result.value?.files.filter((f) => f.status === "conflict") ?? []);
const canSubmit = computed(
  () =>
    !busy.value &&
    (result.value?.import_id
      ? conflicts.value.every(
          (f) =>
            choices.value[f.path]?.action &&
            (choices.value[f.path]?.action !== "rename" || choices.value[f.path]?.name?.trim()),
        )
      : selected.value.length > 0),
);
async function submit(): Promise<void> {
  if (!canSubmit.value) return;
  busy.value = true;
  error.value = "";
  try {
    const next = result.value?.import_id
      ? await backend.resolveBundle!(result.value.import_id, Object.values(choices.value))
      : await backend.importBundle!(props.file, selected.value);
    result.value = next;
    choices.value = {};
    for (const f of next.files.filter((f) => f.status === "conflict"))
      choices.value[f.path] = { path: f.path, fingerprint: f.fingerprint, action: "keep" };
    if (!next.import_id) {
      await sessionsStore.refresh();
      window.dispatchEvent(
        new CustomEvent(HTTP_API_CHANGE_EVENT, { detail: { resources: ["all"] } }),
      );
    }
  } catch (e) {
    result.value = null;
    choices.value = {};
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    busy.value = false;
  }
}
</script>
<template>
  <main class="bundle-import">
    <p class="filename">{{ file.name }}</p>
    <template v-if="!result">
      <label v-for="intent in preview.intents" :key="intent.id" class="intent-row">
        <input
          v-model="selected"
          type="checkbox"
          :value="intent.id"
          :disabled="busy || intent.type !== 'session'"
        />
        <span
          ><strong>{{ intent.name }}</strong
          ><small
            >{{
              intent.type === "session" ? "分享 Session 配置" : `不支持的意图：${intent.type}`
            }}
            · {{ intent.id }}</small
          ><small v-if="intent.assets.length">随附资源：{{ intent.assets.join("、") }}</small></span
        >
      </label>
    </template>
    <template v-else-if="result.import_id">
      <p>请选择冲突文件的处理方式。确认后整批导入，失败时回滚。</p>
      <small
        >相同 {{ result.files.filter((f) => f.status === "same").length }} · 新增
        {{ result.files.filter((f) => f.status === "new").length }} · 冲突
        {{ conflicts.length }}</small
      >
      <div v-for="fileStatus in conflicts" :key="fileStatus.path" class="conflict-row">
        <code>{{ fileStatus.path }}</code>
        <select class="select" v-model="choices[fileStatus.path]!.action" :disabled="busy">
          <option value="keep">复用本地</option>
          <option value="overwrite">覆盖</option>
          <option v-if="fileStatus.can_rename" value="rename">另存新名称</option>
        </select>
        <input
          v-if="choices[fileStatus.path]?.action === 'rename'"
          v-model="choices[fileStatus.path]!.name"
          class="input"
          placeholder="新脚本名称（不含 .lua）"
          :disabled="busy"
        />
      </div>
    </template>
    <template v-else
      ><p>已导入 {{ result.sessions.length }} 个 Session。</p>
      <p v-for="session in result.sessions" :key="session.id">
        {{ session.name }} · #{{ session.id }}
      </p></template
    >
    <p v-if="error" class="error">{{ error }}</p>
    <footer v-if="!result || result.import_id">
      <button class="btn primary" :disabled="!canSubmit" @click="submit">
        {{ busy ? "正在处理…" : result?.import_id ? "解决冲突并导入" : "导入所选意图" }}
      </button>
    </footer>
  </main>
</template>
<style scoped>
.bundle-import {
  padding: 14px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  overflow: auto;
  height: 100%;
}
p {
  margin: 0;
}
.filename,
small {
  color: var(--text-secondary);
}
.intent-row {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 8px 0;
  border-bottom: 1px solid var(--border);
}
.intent-row span {
  min-width: 0;
  overflow-wrap: anywhere;
}
small {
  display: block;
  margin-top: 4px;
}
.conflict-row {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: center;
  padding: 6px 0;
}
code {
  flex: 1;
  overflow-wrap: anywhere;
}
input,
select {
  font: inherit;
}
footer {
  margin-top: auto;
  display: flex;
  justify-content: flex-end;
  padding-top: 10px;
}
.error {
  color: var(--danger);
  white-space: pre-wrap;
}
</style>
