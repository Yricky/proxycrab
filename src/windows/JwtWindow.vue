<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import {
  Io5Checkmark,
  Io5CheckmarkCircle,
  Io5CloseCircle,
  Io5Copy,
  Io5Trash,
  Io5Warning,
} from "vue-icons-plus/io5";
import MonacoEditor from "../components/MonacoEditor.vue";
import { appStore } from "../stores/app";
import {
  JWT_ALGORITHMS,
  JWT_ALGORITHM_OPTIONS,
  decodeToken,
  verifySignature,
  type DecodedJwt,
  type VerifyOutcome,
} from "../utils/jwt";

const token = ref("");
const verifyOn = ref(false);
const alg = ref("");
const keyText = ref("");
const algAuto = ref(false);

const decoded = computed<DecodedJwt>(() => decodeToken(token.value));

/** 解码出的 JSON（无效 JSON 时退回原文显示）。 */
function prettyJson(value: unknown, fallback: string): string {
  if (value === null || value === undefined) return fallback;
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return fallback;
  }
}

const headerView = computed(() => prettyJson(decoded.value.headerJson, decoded.value.headerText));
const payloadView = computed(() => prettyJson(decoded.value.payloadJson, decoded.value.payloadText));

const isHmac = computed(() => alg.value.startsWith("HS"));
const keyPlaceholder = computed(() =>
  isHmac.value ? "HMAC 密钥文本（或 kty=oct 的 JWK JSON）" : "粘贴 PEM 公钥或 JWK JSON",
);

const outcome = ref<VerifyOutcome | null>(null);

// 从 header 自动识别算法（与 jwt.io 一致）
watch(
  () => decoded.value.alg,
  (headerAlg) => {
    if (headerAlg && JWT_ALGORITHM_OPTIONS.includes(headerAlg)) {
      alg.value = headerAlg;
      algAuto.value = true;
    } else if (!headerAlg) {
      algAuto.value = false;
    }
  },
);

// header 内嵌 jwk 时自动载入公钥并开始验签（与 jwt.io 一致）
watch(
  () => decoded.value.headerJson,
  (header) => {
    const jwk =
      header && typeof header === "object" ? (header as { jwk?: unknown }).jwk : undefined;
    if (jwk && typeof jwk === "object" && jwk !== null && "kty" in jwk) {
      keyText.value = JSON.stringify(jwk);
      verifyOn.value = true;
    }
  },
);

let verifyTimer: number | undefined;
let verifySeq = 0;

function scheduleVerify(): void {
  if (verifyTimer !== undefined) window.clearTimeout(verifyTimer);
  verifyTimer = window.setTimeout(() => void runVerify(), 250);
}

async function runVerify(): Promise<void> {
  const seq = ++verifySeq;
  if (!verifyOn.value) {
    outcome.value = null;
    return;
  }
  outcome.value = await verifySignature(token.value, alg.value, keyText.value);
  if (seq !== verifySeq) return;
}

watch([token, verifyOn, alg, keyText], () => scheduleVerify());

onBeforeUnmount(() => {
  if (verifyTimer !== undefined) window.clearTimeout(verifyTimer);
  ++verifySeq;
});

function clearAll(): void {
  token.value = "";
  keyText.value = "";
  verifyOn.value = false;
  outcome.value = null;
  algAuto.value = false;
}

let copiedKey: string | null = null;
let copiedTimer: number | undefined;

async function copy(key: string, value: string): Promise<void> {
  if (!value) return;
  try {
    await navigator.clipboard.writeText(value);
    copiedKey = key;
    if (copiedTimer !== undefined) window.clearTimeout(copiedTimer);
    copiedTimer = window.setTimeout(() => (copiedKey = null), 1200);
  } catch {
    appStore.toast("复制失败", "error");
  }
}

const resultText = computed(() => outcome.value?.message ?? "");
const resultClass = computed(() => `r-${outcome.value?.status ?? "idle"}`);
</script>

<template>
  <div class="jwt-root">
    <!-- Encoded 输入区 -->
    <div class="jwt-encoded">
      <div class="jwt-head">
        <span class="jwt-label">Encoded</span>
        <span class="jwt-spacer" />
        <button class="btn icon" title="清空" @click="clearAll">
          <Io5Trash :size="14" />
        </button>
      </div>
      <textarea
        v-model="token"
        class="input jwt-token mono"
        placeholder="粘贴 JWT 令牌，自动解码…"
        spellcheck="false"
      />
      <div v-if="decoded.error" class="jwt-error text-danger">{{ decoded.error }}</div>
    </div>

    <div class="jwt-columns">
      <!-- 左侧：Header / Payload 解码卡片 -->
      <div class="jwt-left">
        <section class="jwt-card">
          <div class="jwt-head">
            <span class="jwt-label">Header</span>
            <span v-if="decoded.alg" class="jwt-alg-tag">{{ decoded.alg }}</span>
            <span class="jwt-spacer" />
            <button
              class="btn icon"
              title="复制解码后的 Header"
              @click="copy('header', headerView)"
            >
              <Io5Checkmark v-if="copiedKey === 'header'" :size="13" class="text-success" />
              <Io5Copy v-else :size="13" />
            </button>
          </div>
          <div v-if="decoded.ok && headerView" class="jwt-editor">
            <MonacoEditor :model-value="headerView" language="json" readonly />
          </div>
          <div v-else class="jwt-empty text-faint">—</div>
          <div class="jwt-raw mono">{{ decoded.headerRaw || "—" }}</div>
        </section>

        <section class="jwt-card">
          <div class="jwt-head">
            <span class="jwt-label">Payload</span>
            <span class="jwt-spacer" />
            <button
              class="btn icon"
              title="复制解码后的 Payload"
              @click="copy('payload', payloadView)"
            >
              <Io5Checkmark v-if="copiedKey === 'payload'" :size="13" class="text-success" />
              <Io5Copy v-else :size="13" />
            </button>
          </div>
          <div v-if="decoded.ok && payloadView" class="jwt-editor">
            <MonacoEditor :model-value="payloadView" language="json" readonly />
          </div>
          <div v-else class="jwt-empty text-faint">—</div>
          <div class="jwt-raw mono">{{ decoded.payloadRaw || "—" }}</div>
        </section>
      </div>

      <!-- 右侧：签名与验签 -->
      <section class="jwt-card jwt-signature">
        <div class="jwt-head">
          <span class="jwt-label">Signature</span>
          <span class="jwt-spacer" />
          <button
            class="btn icon"
            title="复制签名原文"
            @click="copy('signature', decoded.signatureRaw)"
          >
            <Io5Checkmark v-if="copiedKey === 'signature'" :size="13" class="text-success" />
            <Io5Copy v-else :size="13" />
          </button>
        </div>
        <div class="jwt-raw mono jwt-sig-raw">{{ decoded.signatureRaw || "—" }}</div>

        <div class="jwt-verify-row">
          <label class="jwt-switch-label">
            <button
              class="switch"
              :class="{ on: verifyOn }"
              title="验证签名"
              @click="verifyOn = !verifyOn"
            />
            <span class="text-secondary">验证签名</span>
          </label>
        </div>

        <template v-if="verifyOn">
          <div class="jwt-field">
            <div class="jwt-label text-secondary">算法</div>
            <div class="jwt-alg-row">
              <select v-model="alg" class="select jwt-alg-select">
                <option v-for="a in JWT_ALGORITHMS" :key="a" :value="a">{{ a }}</option>
                <option value="none">none</option>
              </select>
              <span v-if="algAuto" class="jwt-autohint text-faint" title="从 header 自动识别">自动</span>
            </div>
          </div>

          <div class="jwt-field">
            <div class="jwt-label text-secondary">{{ isHmac ? "密钥 (Secret)" : "公钥 (PEM / JWK)" }}</div>
            <textarea
              v-model="keyText"
              class="input jwt-key mono"
              :placeholder="keyPlaceholder"
              spellcheck="false"
            />
          </div>

          <div class="jwt-result" :class="resultClass">
            <Io5CheckmarkCircle v-if="outcome?.status === 'verified'" :size="16" />
            <Io5CloseCircle v-else-if="outcome?.status === 'invalid'" :size="16" />
            <Io5Warning v-else-if="outcome?.status === 'key-error' || outcome?.status === 'unsupported'" :size="16" />
            <Io5CheckmarkCircle v-else-if="outcome?.status === 'unsecured'" :size="16" />
            <span class="jwt-result-text">{{ resultText || "输入密钥后自动验证" }}</span>
          </div>
        </template>
      </section>
    </div>
  </div>
</template>

<style scoped>
.jwt-root {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding: var(--space-3);
  gap: var(--space-2);
  user-select: text;
}

.jwt-encoded {
  flex: none;
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.jwt-head {
  flex: none;
  display: flex;
  align-items: center;
  gap: var(--space-2);
  min-height: 22px;
}

.jwt-label {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-primary);
}

.jwt-spacer {
  flex: 1;
}

.jwt-token {
  resize: none;
  height: 64px;
  white-space: pre-wrap;
  word-break: break-all;
}

.jwt-error {
  font-size: 11px;
  flex: none;
}

.jwt-columns {
  flex: 1;
  min-height: 0;
  display: flex;
  gap: var(--space-2);
}

.jwt-left {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.jwt-card {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--bg-panel);
  overflow: hidden;
  padding: var(--space-2);
  gap: var(--space-1);
}

.jwt-alg-tag {
  font-family: var(--font-mono);
  font-size: 10px;
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 14%, transparent);
  padding: 1px 6px;
  border-radius: 8px;
  line-height: 16px;
}

.jwt-editor {
  flex: 1;
  min-height: 0;
  display: flex;
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  overflow: hidden;
}

.jwt-empty {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 12px;
  border: 1px dashed var(--border);
  border-radius: var(--radius-sm);
}

.jwt-raw {
  flex: none;
  font-size: 10px;
  line-height: 15px;
  color: var(--text-faint);
  background: var(--bg-app);
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  padding: 3px 6px;
  white-space: nowrap;
  overflow-x: auto;
  word-break: break-all;
  max-height: 34px;
}

.jwt-signature {
  flex: 0 0 340px;
}

.jwt-sig-raw {
  word-break: break-all;
}

.jwt-verify-row {
  flex: none;
  display: flex;
  align-items: center;
  margin-top: var(--space-1);
}

.jwt-switch-label {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: 12px;
  cursor: pointer;
}

.jwt-field {
  flex: none;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.jwt-alg-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.jwt-alg-select {
  flex: 1;
}

.jwt-autohint {
  font-size: 10px;
  flex: none;
}

.jwt-key {
  resize: none;
  height: 84px;
  white-space: pre-wrap;
  word-break: break-all;
}

.jwt-result {
  flex: none;
  display: flex;
  align-items: flex-start;
  gap: 6px;
  font-size: 12px;
  padding: 6px 8px;
  border-radius: var(--radius-sm);
  line-height: 16px;
}

.jwt-result-text {
  word-break: break-word;
}

.r-idle,
.r-waiting {
  background: var(--bg-active);
  color: var(--text-secondary);
}

.r-verified,
.r-unsecured {
  background: color-mix(in srgb, var(--success) 12%, transparent);
  color: var(--success);
}

.r-invalid {
  background: color-mix(in srgb, var(--danger) 12%, transparent);
  color: var(--danger);
}

.r-key-error,
.r-unsupported,
.r-token-error {
  background: color-mix(in srgb, var(--warning) 14%, transparent);
  color: var(--warning);
}
</style>
