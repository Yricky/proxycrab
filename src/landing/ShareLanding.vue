<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";
import App from "../App.vue";
import { createShareBackend, SHARE_INVALID_EVENT, verifyShareAccess } from "../api/share-backend";
import { installBackend } from "../api/runtime-backend";
import type { ManagerError } from "../api/types";

const ready = ref(false);
const error = ref("");

function invalidate(event: Event): void {
  const detail = (event as CustomEvent<ManagerError>).detail;
  error.value = detail?.message || "分享链接已失效";
  ready.value = false;
}

onMounted(async () => {
  window.addEventListener(SHARE_INVALID_EVENT, invalidate);
  const token = new URLSearchParams(window.location.search).get("token")?.trim() ?? "";
  if (!token) {
    error.value = "分享链接缺少 token";
    return;
  }
  try {
    const bootstrap = await verifyShareAccess(window.location.origin, token);
    installBackend(createShareBackend(window.location.origin, token, bootstrap));
    document.title = `${bootstrap.session.name} — ProxyCrab`;
    ready.value = true;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  }
});

onBeforeUnmount(() => window.removeEventListener(SHARE_INVALID_EVENT, invalidate));
</script>

<template>
  <App v-if="ready" />
  <main v-else class="share-state">
    <div class="share-card">
      <div class="brand">ProxyCrab</div>
      <h1>{{ error ? "无法查看 Session" : "正在加载 Session…" }}</h1>
      <p v-if="error">{{ error }}</p>
    </div>
  </main>
</template>

<style scoped>
.share-state { height: 100%; display: grid; place-items: center; padding: 24px; background: var(--bg-app); }
.share-card { width: min(440px, 100%); padding: 28px; border: 1px solid var(--border); border-radius: var(--radius-lg); background: var(--bg-panel); box-shadow: var(--shadow-window); }
.brand { margin-bottom: 12px; color: var(--accent); font-size: 12px; font-weight: 700; letter-spacing: .08em; text-transform: uppercase; }
h1 { margin: 0; font-size: 20px; }
p { margin: 12px 0 0; color: var(--danger); font-size: 13px; }
</style>
