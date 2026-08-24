<script setup lang="ts">
import { computed, nextTick, ref } from "vue";
import { Io5Checkmark, Io5Copy, Io5Key, Io5Warning } from "vue-icons-plus/io5";
import { useBackend } from "../api";
import type { CreatedApiKey, PermissionIdentitySummary } from "../api/types";
import { appStore, reportError } from "../stores/app";
import { copyText as writeClipboardText } from "../utils/clipboard";

const props = defineProps<{ identities: PermissionIdentitySummary[] }>();
const emit = defineEmits<{ close: [createdId?: string] }>();
const backend = useBackend();
const nameInput = ref<HTMLInputElement | null>(null);
const name = ref("");
const creating = ref(false);
const created = ref<CreatedApiKey | null>(null);
const acknowledged = ref(false);

const validationError = computed(() => {
  const value = name.value.trim();
  if (!value) return "请输入名称";
  if (Array.from(value).length > 64) return "名称不能超过 64 个字符";
  if (props.identities.some((identity) => identity.kind === "api_key" && identity.name === value)) {
    return "名称已存在";
  }
  return null;
});

async function create(): Promise<void> {
  if (validationError.value || creating.value) {
    void nextTick(() => nameInput.value?.focus());
    return;
  }
  creating.value = true;
  try {
    created.value = await backend.createHttpApiKey(name.value.trim());
  } catch (error) {
    reportError(error, "创建 API Key 失败");
  } finally {
    creating.value = false;
  }
}

async function copy(): Promise<void> {
  if (!created.value) return;
  try {
    await writeClipboardText(created.value.api_key);
    appStore.toast("API Key 已复制", "success");
  } catch (error) {
    reportError(error, "复制 API Key 失败");
  }
}

function close(): void {
  if (created.value && !acknowledged.value) return;
  emit("close", created.value?.identity.id);
}
</script>

<template>
  <Teleport to="body">
    <div class="key-mask" @keydown.esc="close">
      <div class="key-dialog" role="dialog" aria-modal="true" aria-labelledby="key-title">
        <template v-if="!created">
          <header>
            <Io5Key :size="18" />
            <div>
              <strong id="key-title">新建 API Key</strong>
              <span>新密钥将使用默认的最小权限模板</span>
            </div>
          </header>
          <label class="key-field">
            <span>名称</span>
            <input
              ref="nameInput"
              v-model="name"
              class="input"
              autofocus
              placeholder="例如：本地脚本"
              @keyup.enter="create"
            />
          </label>
          <span v-if="name && validationError" class="validation-error">{{
            validationError
          }}</span>
          <footer>
            <button class="btn" :disabled="creating" @click="close">取消</button>
            <button
              class="btn primary"
              :disabled="Boolean(validationError) || creating"
              @click="create"
            >
              {{ creating ? "创建中…" : "创建" }}
            </button>
          </footer>
        </template>

        <template v-else>
          <header>
            <Io5Checkmark :size="18" class="created-icon" />
            <div>
              <strong id="key-title">API Key 已创建</strong>
              <span>{{ created.identity.name }}</span>
            </div>
          </header>
          <div class="secret-warning">
            <Io5Warning :size="17" />
            <span>这是唯一一次显示完整密钥。关闭后无法再次查看或恢复。</span>
          </div>
          <div class="secret-row">
            <code>{{ created.api_key }}</code>
            <button class="btn" @click="copy">
              <Io5Copy :size="14" />
              复制
            </button>
          </div>
          <label class="acknowledge">
            <input v-model="acknowledged" type="checkbox" />
            <span>我已保存此 API Key</span>
          </label>
          <footer>
            <button class="btn primary" :disabled="!acknowledged" @click="close">
              完成
            </button>
          </footer>
        </template>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.key-mask { position: fixed; inset: 0; z-index: 100007; display: grid; place-items: center; background: rgba(0, 0, 0, 0.38); }
.key-dialog { width: min(480px, calc(100vw - 32px)); padding: 16px; border: 1px solid var(--border-strong); border-radius: var(--radius-lg); background: var(--bg-elevated); box-shadow: var(--shadow-window); }
header { display: flex; align-items: flex-start; gap: 10px; margin-bottom: 16px; }
header > svg { flex: none; margin-top: 2px; color: var(--accent); }
header .created-icon { color: var(--success); }
header div { display: flex; flex-direction: column; }
header strong { font-size: 14px; }
header span { color: var(--text-secondary); font-size: 11px; }
.key-field { display: flex; flex-direction: column; gap: 5px; color: var(--text-secondary); font-size: 11px; }
.key-field .input { color: var(--text); font-size: 13px; }
.validation-error { display: block; margin-top: 5px; color: var(--danger); font-size: 11px; }
.secret-warning { display: flex; align-items: flex-start; gap: 8px; margin-bottom: 12px; padding: 8px 10px; border-left: 3px solid var(--warning); background: color-mix(in srgb, var(--warning) 10%, transparent); color: var(--warning); font-size: 11px; }
.secret-warning svg { flex: none; }
.secret-row { display: flex; align-items: stretch; gap: 8px; }
.secret-row code { flex: 1; min-width: 0; overflow-wrap: anywhere; padding: 9px 10px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--bg-app); font: 11px/1.5 var(--font-mono); user-select: text; }
.acknowledge { display: flex; align-items: center; gap: 7px; margin-top: 13px; font-size: 12px; cursor: pointer; }
.acknowledge input { accent-color: var(--accent); }
footer { display: flex; justify-content: flex-end; gap: 8px; margin-top: 18px; }
</style>
