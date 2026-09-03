<script setup lang="ts">
import QRCode from "qrcode";
import { onBeforeUnmount, ref, watch } from "vue";
import { Io5Download, Io5Trash } from "vue-icons-plus/io5";

type ErrorCorrectionLevel = "L" | "M" | "Q" | "H";

const text = ref("");
const level = ref<ErrorCorrectionLevel>("M");
const qrCodeDataUrl = ref("");
const generateError = ref("");

let debounceTimer: ReturnType<typeof setTimeout> | undefined;
let generation = 0;

async function generate(): Promise<void> {
  const current = ++generation;
  const value = text.value;
  if (!value) {
    qrCodeDataUrl.value = "";
    generateError.value = "";
    return;
  }
  try {
    // 按模块数动态计算输出宽度，保证内容较多时每个模块仍有足够像素。
    const modules = QRCode.create(value, {
      errorCorrectionLevel: level.value,
    }).modules.size;
    const width = Math.max((modules + 2) * 6, 256);
    const dataUrl = await QRCode.toDataURL(value, {
      errorCorrectionLevel: level.value,
      margin: 1,
      width,
    });
    if (current !== generation) return;
    qrCodeDataUrl.value = dataUrl;
    generateError.value = "";
  } catch {
    if (current !== generation) return;
    qrCodeDataUrl.value = "";
    generateError.value = "生成失败：内容过长，请缩短文本或降低容错等级";
  }
}

watch([text, level], () => {
  if (debounceTimer !== undefined) clearTimeout(debounceTimer);
  debounceTimer = setTimeout(() => {
    debounceTimer = undefined;
    void generate();
  }, 200);
});

function download(): void {
  if (!qrCodeDataUrl.value) return;
  const link = document.createElement("a");
  link.href = qrCodeDataUrl.value;
  link.download = "qrcode.png";
  link.click();
}

function clearAll(): void {
  text.value = "";
}

onBeforeUnmount(() => {
  if (debounceTimer !== undefined) clearTimeout(debounceTimer);
});
</script>

<template>
  <div class="qr-root">
    <div class="qr-field">
      <div class="qr-label text-secondary">内容</div>
      <textarea
        v-model="text"
        class="input qr-text mono"
        placeholder="输入要生成二维码的文本或链接"
        spellcheck="false"
      />
    </div>

    <div class="qr-buttons">
      <button class="btn" :disabled="!qrCodeDataUrl" @click="download">
        <Io5Download :size="14" /> 下载 PNG
      </button>
      <button class="btn" :disabled="!text" @click="clearAll">
        <Io5Trash :size="14" /> 清空
      </button>
      <span class="qr-spacer" />
      <span class="text-secondary">容错等级</span>
      <select v-model="level" class="select">
        <option value="L">L（约 7%）</option>
        <option value="M">M（约 15%）</option>
        <option value="Q">Q（约 25%）</option>
        <option value="H">H（约 30%）</option>
      </select>
    </div>

    <div class="qr-preview">
      <img v-if="qrCodeDataUrl" :src="qrCodeDataUrl" alt="二维码" />
      <span v-else-if="generateError" class="text-danger">{{ generateError }}</span>
      <span v-else class="text-faint">输入内容后自动生成二维码</span>
    </div>
  </div>
</template>

<style scoped>
.qr-root {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding: var(--space-3);
  gap: var(--space-2);
}

.qr-field {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.qr-label {
  font-size: 12px;
  flex: none;
}

.qr-text {
  flex: 1;
  min-height: 0;
  resize: none;
  user-select: text;
  white-space: pre-wrap;
  word-break: break-all;
}

.qr-buttons {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  flex: none;
}

.qr-spacer {
  flex: 1;
}

.qr-preview {
  flex: none;
  display: grid;
  place-items: center;
  align-self: center;
  width: 272px;
  height: 272px;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: #fff;
  overflow: hidden;
  font-size: 12px;
  padding: 0 8px;
  text-align: center;
}

.qr-preview img {
  display: block;
  width: 256px;
  height: 256px;
  /* 高分辨率 PNG 缩放到固定显示尺寸时保持边缘锐利。 */
  image-rendering: pixelated;
}
</style>
