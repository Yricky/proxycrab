<script setup lang="ts">
import { ref } from "vue";
import { Io5ArrowDown, Io5ArrowUp, Io5Copy, Io5Trash } from "vue-icons-plus/io5";
import { appStore, reportError } from "../stores/app";

type Mode = "standard" | "urlsafe";

const mode = ref<Mode>("standard");
const raw = ref("");
const encoded = ref("");
const decodeError = ref("");

function bytesToBinaryString(bytes: Uint8Array): string {
  let result = "";
  for (const byte of bytes) result += String.fromCharCode(byte);
  return result;
}

function encodeText(text: string, urlSafe: boolean): string {
  const base64 = btoa(bytesToBinaryString(new TextEncoder().encode(text)));
  return urlSafe ? base64.replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "") : base64;
}

function decodeText(text: string, urlSafe: boolean): string {
  let base64 = text.trim();
  if (urlSafe) {
    base64 = base64.replace(/-/g, "+").replace(/_/g, "/");
    const remainder = base64.length % 4;
    if (remainder) base64 += "=".repeat(4 - remainder);
  }
  const binary = atob(base64);
  const bytes = Uint8Array.from(binary, (ch) => ch.charCodeAt(0));
  return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
}

function encode(): void {
  decodeError.value = "";
  encoded.value = encodeText(raw.value, mode.value === "urlsafe");
}

function decode(): void {
  decodeError.value = "";
  try {
    raw.value = decodeText(encoded.value, mode.value === "urlsafe");
  } catch {
    decodeError.value = "解码失败：输入不是合法的 Base64 内容";
  }
}

function clearAll(): void {
  raw.value = "";
  encoded.value = "";
  decodeError.value = "";
}

async function copyResult(): Promise<void> {
  if (!encoded.value) return;
  try {
    await navigator.clipboard.writeText(encoded.value);
    appStore.toast("已复制到剪贴板", "success");
  } catch (error) {
    reportError(error, "复制失败");
  }
}
</script>

<template>
  <div class="b64-root">
    <div class="b64-toolbar">
      <span class="text-secondary">编码模式</span>
      <select v-model="mode" class="select">
        <option value="standard">Standard</option>
        <option value="urlsafe">URL-safe</option>
      </select>
    </div>

    <div class="b64-field">
      <div class="b64-label text-secondary">原始值</div>
      <textarea
        v-model="raw"
        class="input b64-text mono"
        placeholder="输入要编码的文本"
        spellcheck="false"
      />
    </div>

    <div class="b64-buttons">
      <button class="btn primary" @click="encode"><Io5ArrowDown :size="14" /> 编码</button>
      <button class="btn" @click="decode"><Io5ArrowUp :size="14" /> 解码</button>
      <button class="btn" @click="clearAll"><Io5Trash :size="14" /> 清空</button>
      <span class="b64-spacer" />
      <button class="btn" :disabled="!encoded" @click="copyResult">
        <Io5Copy :size="14" /> 复制结果
      </button>
    </div>

    <div class="b64-field">
      <div class="b64-label text-secondary">编码值</div>
      <textarea
        v-model="encoded"
        class="input b64-text mono"
        placeholder="Base64 结果"
        spellcheck="false"
      />
      <div v-if="decodeError" class="b64-error text-danger">{{ decodeError }}</div>
    </div>
  </div>
</template>

<style scoped>
.b64-root {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding: var(--space-3);
  gap: var(--space-2);
}

.b64-toolbar {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  flex: none;
}

.b64-field {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.b64-label {
  font-size: 12px;
  flex: none;
}

.b64-text {
  flex: 1;
  min-height: 0;
  resize: none;
  user-select: text;
  white-space: pre-wrap;
  word-break: break-all;
}

.b64-buttons {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  flex: none;
}

.b64-spacer {
  flex: 1;
}

.b64-error {
  font-size: 12px;
  flex: none;
}
</style>
