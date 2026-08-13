<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import {
  Io5Checkmark,
  Io5CheckmarkCircle,
  Io5CloseCircle,
  Io5Copy,
  Io5Sparkles,
  Io5Trash,
  Io5Warning,
} from "vue-icons-plus/io5";
import MonacoEditor from "../components/MonacoEditor.vue";
import { appStore } from "../stores/app";
import {
  JWT_ALGORITHMS,
  JWT_ALGORITHM_OPTIONS,
  base64UrlEncode,
  decodeToken,
  verifySignature,
  type DecodedJwt,
  type VerifyOutcome,
} from "../utils/jwt";

/** jwt.io 官方示例令牌（HS256，密钥 "your-256-bit-secret"）。 */
const SAMPLE_TOKEN =
  "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9." +
  "eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ." +
  "SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
const SAMPLE_SECRET = "your-256-bit-secret";

const textEncoder = new TextEncoder();

const token = ref("");
const alg = ref("");
const algAuto = ref(false);
const keyText = ref("");
const secretB64 = ref(false);

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

// 编辑器内容经 ref 同步：用户编辑回写令牌时跳过一次回声，避免重排版式打断输入
const headerEditorText = ref("");
const payloadEditorText = ref("");
let skipHeaderSync = false;
let skipPayloadSync = false;

watch(
  headerView,
  (v) => {
    if (skipHeaderSync) {
      skipHeaderSync = false;
      return;
    }
    headerEditorText.value = v;
  },
  { immediate: true },
);
watch(
  payloadView,
  (v) => {
    if (skipPayloadSync) {
      skipPayloadSync = false;
      return;
    }
    payloadEditorText.value = v;
  },
  { immediate: true },
);

const headerEditError = ref("");
const payloadEditError = ref("");

/** 把编辑后的 JSON 重新 base64url 编码并回写到令牌对应分段（与 jwt.io 的双向编辑一致）。 */
function reencodeSegment(index: 0 | 1, text: string): boolean {
  const parts = token.value.trim().split(".");
  if (parts.length < 2) return false;
  try {
    const parsed: unknown = JSON.parse(text);
    parts[index] = base64UrlEncode(textEncoder.encode(JSON.stringify(parsed)));
    token.value = parts.join(".");
    return true;
  } catch {
    return false;
  }
}

function onHeaderEdit(text: string): void {
  headerEditError.value = reencodeSegment(0, text) ? "" : "JSON 语法错误，修改未回写";
  if (!headerEditError.value) skipHeaderSync = true;
}

function onPayloadEdit(text: string): void {
  payloadEditError.value = reencodeSegment(1, text) ? "" : "JSON 语法错误，修改未回写";
  if (!payloadEditError.value) skipPayloadSync = true;
}

const editorOptions = {
  lineNumbers: "off",
  folding: false,
  glyphMargin: false,
  lineDecorationsWidth: 8,
};

// ---------- Encoded 彩色分段高亮 ----------

const segments = computed(() => {
  const t = token.value;
  const i1 = t.indexOf(".");
  const i2 = i1 < 0 ? -1 : t.indexOf(".", i1 + 1);
  return {
    header: i1 < 0 ? t : t.slice(0, i1),
    payload: i1 < 0 ? "" : i2 < 0 ? t.slice(i1 + 1) : t.slice(i1 + 1, i2),
    signature: i2 < 0 ? "" : t.slice(i2 + 1),
    hasPayload: i1 >= 0,
    hasSignature: i2 >= 0,
  };
});

const tokenInputRef = ref<HTMLTextAreaElement | null>(null);
const highlightRef = ref<HTMLElement | null>(null);

function syncScroll(): void {
  const ta = tokenInputRef.value;
  const hl = highlightRef.value;
  if (!ta || !hl) return;
  hl.style.transform = `translate(${-ta.scrollLeft}px, ${-ta.scrollTop}px)`;
}

watch(token, () => void nextTick(syncScroll));

// ---------- 时间戳声明（exp / nbf / iat）提示 ----------

interface ClaimChip {
  key: string;
  text: string;
  cls: string;
  title: string;
}

function formatTime(sec: number): string {
  const d = new Date(sec * 1000);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

const claimChips = computed<ClaimChip[]>(() => {
  const payload = decoded.value.payloadJson;
  if (!payload || typeof payload !== "object") return [];
  const record = payload as Record<string, unknown>;
  const now = Date.now() / 1000;
  const chips: ClaimChip[] = [];
  const exp = record.exp;
  if (typeof exp === "number" && Number.isFinite(exp)) {
    const expired = exp < now;
    chips.push({
      key: "exp",
      text: `${formatTime(exp)} · ${expired ? "已过期" : "未过期"}`,
      cls: expired ? "jc-danger" : "jc-success",
      title: "过期时间",
    });
  }
  const nbf = record.nbf;
  if (typeof nbf === "number" && Number.isFinite(nbf)) {
    const pending = nbf > now;
    chips.push({
      key: "nbf",
      text: `${formatTime(nbf)} · ${pending ? "未生效" : "已生效"}`,
      cls: pending ? "jc-warn" : "",
      title: "生效时间",
    });
  }
  const iat = record.iat;
  if (typeof iat === "number" && Number.isFinite(iat)) {
    chips.push({ key: "iat", text: formatTime(iat), cls: "", title: "签发时间" });
  }
  return chips;
});

// ---------- 验签 ----------

const FORMULA_NAMES: Record<string, string> = {
  HS256: "HMACSHA256",
  HS384: "HMACSHA384",
  HS512: "HMACSHA512",
  RS256: "RSASHA256",
  RS384: "RSASHA384",
  RS512: "RSASHA512",
  PS256: "RSASSA-PSS-SHA256",
  PS384: "RSASSA-PSS-SHA384",
  PS512: "RSASSA-PSS-SHA512",
  ES256: "ECDSASHA256",
  ES384: "ECDSASHA384",
  ES512: "ECDSASHA512",
};

const isHmac = computed(() => alg.value.startsWith("HS"));
const formulaName = computed(() => FORMULA_NAMES[alg.value] ?? (alg.value || "ALGORITHM"));
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

// header 内嵌 jwk 时自动载入公钥（与 jwt.io 一致，载入后自动触发验签）
watch(
  () => decoded.value.headerJson,
  (header) => {
    const jwk =
      header && typeof header === "object" ? (header as { jwk?: unknown }).jwk : undefined;
    if (jwk && typeof jwk === "object" && jwk !== null && "kty" in jwk) {
      keyText.value = JSON.stringify(jwk);
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
  if (!token.value.trim()) {
    outcome.value = null;
    return;
  }
  const result = await verifySignature(token.value, alg.value, keyText.value, secretB64.value);
  if (seq !== verifySeq) return;
  outcome.value = result;
}

watch([token, alg, keyText, secretB64], () => scheduleVerify());

onBeforeUnmount(() => {
  if (verifyTimer !== undefined) window.clearTimeout(verifyTimer);
  ++verifySeq;
});

const resultText = computed(() => outcome.value?.message ?? "");
const resultClass = computed(() => `r-${outcome.value?.status ?? "idle"}`);

// ---------- 操作 ----------

function clearAll(): void {
  token.value = "";
  keyText.value = "";
  secretB64.value = false;
  outcome.value = null;
  algAuto.value = false;
  headerEditError.value = "";
  payloadEditError.value = "";
}

function loadSample(): void {
  clearAll();
  token.value = SAMPLE_TOKEN;
  keyText.value = SAMPLE_SECRET;
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
</script>

<template>
  <div class="jwt-root">
    <div class="jwt-cols">
      <!-- 左栏：Encoded（彩色分段，与 jwt.io 一致） -->
      <section class="jwt-encoded">
        <div class="jwt-head">
          <span class="jwt-label">Encoded</span>
          <span v-if="token" class="jwt-legend">
            <i class="dot dh" />Header<i class="dot dp" />Payload<i class="dot ds" />Signature
          </span>
          <span class="jwt-spacer" />
          <button class="btn icon" title="载入示例令牌" @click="loadSample">
            <Io5Sparkles :size="13" />
          </button>
          <button class="btn icon" title="复制令牌" @click="copy('token', token)">
            <Io5Checkmark v-if="copiedKey === 'token'" :size="13" class="text-success" />
            <Io5Copy v-else :size="13" />
          </button>
          <button class="btn icon" title="清空" @click="clearAll">
            <Io5Trash :size="13" />
          </button>
        </div>
        <div class="jwt-tokenbox">
          <pre ref="highlightRef" class="jwt-hl mono" aria-hidden="true"><span class="c-h">{{ segments.header }}</span><template v-if="segments.hasPayload"><span class="c-dot">.</span><span class="c-p">{{ segments.payload }}</span></template><template
  v-if="segments.hasSignature"><span class="c-dot">.</span><span class="c-s">{{ segments.signature }}</span></template><span>&#8203;</span>
</pre>
          <textarea ref="tokenInputRef" v-model="token" class="jwt-token mono" placeholder="粘贴 JWT 令牌，自动解码…"
            spellcheck="false" @scroll="syncScroll" />
        </div>
        <div v-if="decoded.error" class="jwt-error text-danger">{{ decoded.error }}</div>
      </section>

      <!-- 右栏：Decoded -->
      <div class="jwt-decoded">
        <div class="jwt-decoded-head">
          <span class="jwt-label">Decoded</span>
          <span class="jwt-spacer" />
          <span v-if="algAuto" class="jwt-autohint" title="从 header 自动识别">自动</span>
          <select v-model="alg" class="select jwt-alg">
            <option value="" disabled>算法</option>
            <option v-for="a in JWT_ALGORITHMS" :key="a" :value="a">{{ a }}</option>
            <option value="none">none</option>
          </select>
        </div>

        <section class="jwt-sec jwt-sec-header">
          <div class="jwt-head">
            <span class="jwt-label jwt-sec-title">Header</span>
            <span v-if="decoded.alg" class="jwt-alg-tag">{{ decoded.alg }}</span>
            <span class="jwt-spacer" />
            <button class="btn icon" title="复制解码后的 Header" @click="copy('header', headerView)">
              <Io5Checkmark v-if="copiedKey === 'header'" :size="12" class="text-success" />
              <Io5Copy v-else :size="12" />
            </button>
          </div>
          <div v-if="decoded.ok" class="jwt-editor">
            <MonacoEditor :model-value="headerEditorText" language="json" :options="editorOptions"
              @update:model-value="onHeaderEdit" />
          </div>
          <div v-else class="jwt-empty text-faint">粘贴令牌后自动解码</div>
          <div v-if="headerEditError" class="jwt-edit-error text-warning">{{ headerEditError }}</div>
        </section>

        <section class="jwt-sec jwt-sec-payload">
          <div class="jwt-head">
            <span class="jwt-label jwt-sec-title">Payload</span>
            <div v-if="claimChips.length" class="jwt-claims">
              <span v-for="c in claimChips" :key="c.key" class="jwt-claim" :class="c.cls" :title="c.title">
                <span class="jc-key mono">{{ c.key }}</span>{{ c.text }}
              </span>
            </div>
            <span class="jwt-spacer" />
            <button class="btn icon" title="复制解码后的 Payload" @click="copy('payload', payloadView)">
              <Io5Checkmark v-if="copiedKey === 'payload'" :size="12" class="text-success" />
              <Io5Copy v-else :size="12" />
            </button>
          </div>
          <div v-if="decoded.ok" class="jwt-editor">
            <MonacoEditor :model-value="payloadEditorText" language="json" :options="editorOptions"
              @update:model-value="onPayloadEdit" />
          </div>
          <div v-else class="jwt-empty text-faint">粘贴令牌后自动解码</div>
          <div v-if="payloadEditError" class="jwt-edit-error text-warning">{{ payloadEditError }}</div>
        </section>

        <section class="jwt-sec jwt-sec-sig">
          <div class="jwt-head">
            <span class="jwt-label jwt-sec-title">Signature</span>
            <div class="jwt-result" :class="resultClass">
              <Io5CheckmarkCircle v-if="outcome?.status === 'verified'" :size="12" />
              <Io5CloseCircle v-else-if="outcome?.status === 'invalid'" :size="12" />
              <Io5Warning
                v-else-if="outcome?.status === 'key-error' || outcome?.status === 'unsupported' || outcome?.status === 'unsecured'"
                :size="15" />
              <span class="jwt-result-text">{{ resultText || "输入密钥后自动验证签名" }}</span>
            </div>
            <span class="jwt-spacer" />
            <button class="btn icon" title="复制签名原文" @click="copy('signature', decoded.signatureRaw)">
              <Io5Checkmark v-if="copiedKey === 'signature'" :size="12" class="text-success" />
              <Io5Copy v-else :size="12" />
            </button>
          </div>

          <div v-if="decoded.ok && decoded.unsecured" class="jwt-unsecured text-warning">
            <Io5Warning :size="13" />
            <span>该令牌未签名（alg=none 或无签名段）</span>
          </div>
          <template v-else>
            <div class="jwt-formula mono">
              <div class="jf-line">{{ formulaName }}(</div>
              <div class="jf-line jf-indent">
                base64UrlEncode(<span class="c-h">header</span>) + <span class="jf-dim">"."</span> +
              </div>
              <div class="jf-line jf-indent">base64UrlEncode(<span class="c-p">payload</span>),</div>
              <textarea v-model="keyText" class="jwt-key mono" :placeholder="keyPlaceholder" spellcheck="false" />
              <div class="jf-line">)</div>
            </div>
            <label v-if="isHmac" class="jwt-b64 text-secondary">
              <input v-model="secretB64" type="checkbox" />
              secret base64 encoded
            </label>
          </template>
        </section>
      </div>
    </div>
  </div>
</template>

<style scoped>
.jwt-root {
  /* jwt.io 分段配色：Header 红 / Payload 紫 / Signature 青 */
  --jwt-h: #d6004e;
  --jwt-p: #b829e8;
  --jwt-s: #0090c8;

  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding: var(--space-3);
  user-select: text;
}

[data-theme="dark"] .jwt-root {
  --jwt-h: #fb4d85;
  --jwt-p: #d63aff;
  --jwt-s: #00b9f1;
}

.jwt-cols {
  flex: 1;
  min-height: 0;
  display: flex;
  gap: var(--space-3);
}

/* 区块之间不用卡片描边，靠留白 + 底色对比表达层级 */
.jwt-encoded,
.jwt-sec {
  display: flex;
  flex-direction: column;
  min-width: 0;
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

/* ---------- Encoded ---------- */

.jwt-encoded {
  flex: 0 0 40%;
  min-height: 0;
}

.jwt-legend {
  display: inline-flex;
  align-items: center;
  font-size: 10px;
  color: var(--text-faint);
  white-space: nowrap;
}

.dot {
  display: inline-block;
  width: 7px;
  height: 7px;
  border-radius: 50%;
  margin: 0 4px 0 8px;
}

.dot.dh {
  background: var(--jwt-h);
  margin-left: 0;
}

.dot.dp {
  background: var(--jwt-p);
}

.dot.ds {
  background: var(--jwt-s);
}

.jwt-tokenbox {
  position: relative;
  flex: 1;
  min-height: 0;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--bg-panel);
  overflow: hidden;
  transition: border-color 0.12s;
}

.jwt-tokenbox:focus-within {
  border-color: var(--accent);
}

.jwt-hl,
.jwt-token {
  position: absolute;
  inset: 0;
  margin: 0;
  padding: 6px 10px;
  font-family: var(--font-mono);
  font-size: 12px;
  line-height: 17px;
  letter-spacing: 0;
  white-space: pre-wrap;
  word-break: break-all;
  overflow-wrap: break-word;
}

.jwt-hl {
  overflow: hidden;
  pointer-events: none;
  color: var(--text-faint);
}

.jwt-token {
  border: none;
  outline: none;
  resize: none;
  background: transparent;
  color: transparent;
  caret-color: var(--text);
  width: 100%;
  height: 100%;
}

.jwt-token::placeholder {
  color: var(--text-faint);
}

.c-h {
  color: var(--jwt-h);
}

.c-p {
  color: var(--jwt-p);
}

.c-s {
  color: var(--jwt-s);
}

.c-dot {
  color: var(--text-faint);
}

.jwt-error {
  flex: none;
  font-size: 11px;
}

/* ---------- Decoded ---------- */

.jwt-decoded {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  /* 与 Encoded 栏之间仅用一道发丝线分隔 */
  border-left: 1px solid var(--border);
  padding-left: var(--space-3);
}

.jwt-decoded-head {
  flex: none;
  display: flex;
  align-items: center;
  gap: var(--space-2);
  min-height: 22px;
  padding: 0 2px;
}

.jwt-alg {
  flex: none;
  width: 108px;
  padding: 2px 6px;
  font-size: 12px;
}

.jwt-autohint {
  font-size: 10px;
  color: var(--text-faint);
}

.jwt-sec {
  flex: none;
}

.jwt-sec-title {
  font-weight: 600;
}

.jwt-sec-payload {
  flex: 1;
  min-height: 0;
}

.jwt-sec-payload .jwt-editor,
.jwt-sec-payload .jwt-empty {
  flex: 1;
  min-height: 0;
}

.jwt-sec-header .jwt-editor {
  height: 144px;
}

.jwt-alg-tag {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: 10px;
  line-height: 16px;
  padding: 0 6px;
  border-radius: 8px;
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 14%, transparent);
}

.jwt-editor {
  display: flex;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--bg-panel);
  overflow: hidden;
}

.jwt-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 12px;
  padding: var(--space-3);
}

.jwt-edit-error {
  flex: none;
  font-size: 11px;
}

.text-warning {
  color: var(--warning);
}

/* 时间戳声明提示 */
.jwt-claims {
  flex: none;
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}

.jwt-claim {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: 10px;
  line-height: 16px;
  padding: 0 6px;
  border-radius: 8px;
  background: var(--bg-active);
  color: var(--text-secondary);
}

.jwt-claim.jc-success {
  background: color-mix(in srgb, var(--success) 12%, transparent);
  color: var(--success);
}

.jwt-claim.jc-danger {
  background: color-mix(in srgb, var(--danger) 12%, transparent);
  color: var(--danger);
}

.jwt-claim.jc-warn {
  background: color-mix(in srgb, var(--warning) 14%, transparent);
  color: var(--warning);
}

.jc-key {
  font-size: 10px;
  font-weight: 600;
  opacity: 0.85;
}

/* ---------- Signature ---------- */

.jwt-formula {
  flex: none;
  font-size: 11px;
  line-height: 16px;
  color: var(--text-secondary);
  background: var(--bg-panel);
  border-radius: var(--radius-md);
  padding: 8px 10px;
}

.jf-line {
  white-space: pre;
}

.jf-indent {
  padding-left: 12px;
}

.jf-dim {
  color: var(--text-faint);
}

.jwt-key {
  resize: none;
  height: 56px;
  margin: 3px 0 3px 12px;
  width: calc(100% - 12px);
  padding: 5px 8px;
  border: none;
  border-radius: var(--radius-sm);
  background: var(--bg-app);
  color: var(--text);
  font-family: var(--font-mono);
  outline: none;
  white-space: pre-wrap;
  word-break: break-all;
  font-size: 11px;
  transition: box-shadow 0.12s;
}

.jwt-key:focus {
  box-shadow: inset 0 0 0 1px var(--accent);
}

.jwt-key::placeholder {
  color: var(--text-faint);
}

.jwt-b64 {
  flex: none;
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: 11px;
  cursor: pointer;
}

.jwt-b64 input {
  margin: 0;
  accent-color: var(--accent);
}

.jwt-unsecured {
  flex: none;
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  padding: 6px 8px;
  border-radius: var(--radius-sm);
  background: color-mix(in srgb, var(--warning) 14%, transparent);
}

.jwt-result {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: 10px;
  line-height: 16px;
  padding: 0 6px;
  border-radius: 8px;
}

.jwt-result-text {
  word-break: break-word;
}

.r-idle,
.r-waiting {
  background: var(--bg-active);
  color: var(--text-secondary);
  font-weight: 400;
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
