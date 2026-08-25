<script setup lang="ts">
import { ref } from "vue";
import App from "../App.vue";
import { createHttpBackend, verifyCliAccess } from "../api/http-backend";
import { installBackend } from "../api/runtime-backend";
import ShareLanding from "./ShareLanding.vue";

const sharePage = window.location.pathname === "/session";

const token = ref("");
const error = ref("");
const submitting = ref(false);
const ready = ref(false);

async function connect(): Promise<void> {
  const value = token.value.trim();
  if (!value || submitting.value) return;
  submitting.value = true;
  error.value = "";
  try {
    const bootstrap = await verifyCliAccess(window.location.origin, value);
    installBackend(createHttpBackend(window.location.origin, value, bootstrap));
    token.value = "";
    ready.value = true;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    submitting.value = false;
  }
}
</script>

<template>
  <ShareLanding v-if="sharePage" />
  <App v-else-if="ready" />
  <main v-else class="cli-landing">
    <form class="login-card" @submit.prevent="connect">
      <div class="brand">ProxyCrab</div>
      <h1>连接 CLI</h1>
      <p>输入 CLI 启动时输出的本次运行 Access Token。</p>
      <input
        v-model="token"
        class="input mono"
        type="password"
        autocomplete="off"
        spellcheck="false"
        autofocus
        placeholder="pcrab_ui_…"
      />
      <span v-if="error" class="error">{{ error }}</span>
      <button class="btn primary" type="submit" :disabled="!token.trim() || submitting">
        {{ submitting ? "正在连接…" : "连接" }}
      </button>
    </form>
  </main>
</template>

<style scoped>
.cli-landing { height: 100%; display: grid; place-items: center; padding: 24px; background: var(--bg-app); }
.login-card { width: min(420px, 100%); display: flex; flex-direction: column; gap: 14px; padding: 28px; border: 1px solid var(--border); border-radius: var(--radius-lg); background: var(--bg-panel); box-shadow: var(--shadow-window); }
.brand { color: var(--accent); font-size: 12px; font-weight: 700; letter-spacing: .08em; text-transform: uppercase; }
h1 { margin: 0; font-size: 22px; }
p { margin: -5px 0 2px; color: var(--text-secondary); font-size: 13px; }
.error { color: var(--danger); font-size: 12px; }
.btn { align-self: flex-end; min-width: 88px; }
</style>
