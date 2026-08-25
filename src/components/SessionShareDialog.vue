<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useBackend } from "../api";
import type { SessionMetadata } from "../api/types";
import { appStore, reportError } from "../stores/app";
import { copyText } from "../utils/clipboard";
import {
  DEFAULT_SHARE_HOURS,
  buildSessionShareLinks,
  parseShareHours,
} from "../utils/session-share";

const props = defineProps<{
  session: SessionMetadata | null;
}>();
const emit = defineEmits<{ close: [] }>();

const backend = useBackend();
const hoursText = ref(String(DEFAULT_SHARE_HOURS));
const links = ref<string[]>([]);
const generating = ref(false);
const expiresAt = ref<number | null>(null);

const hours = computed(() => parseShareHours(hoursText.value));
const linkText = computed(() => links.value.join("\n"));

watch(
  () => props.session?.id,
  () => {
    hoursText.value = String(DEFAULT_SHARE_HOURS);
    links.value = [];
    expiresAt.value = null;
  },
);

async function generate(): Promise<void> {
  if (!props.session || hours.value === null || generating.value) return;
  generating.value = true;
  try {
    const [share, status, addresses] = await Promise.all([
      backend.createSessionShare(props.session.id, hours.value),
      backend.getHttpServiceStatus(),
      backend.listLocalIps(),
    ]);
    links.value = buildSessionShareLinks(addresses, status.port, share.token);
    expiresAt.value = share.expires_at;
    if (links.value.length === 0) {
      appStore.toast("没有可用于分享的本机 IPv4 地址", "info");
    }
  } catch (error) {
    reportError(error, "生成分享链接失败");
  } finally {
    generating.value = false;
  }
}

async function copyAll(): Promise<void> {
  if (!linkText.value) return;
  try {
    await copyText(linkText.value);
    appStore.toast("分享链接已复制", "success");
  } catch (error) {
    reportError(error, "复制分享链接失败");
  }
}

function formatExpiry(value: number): string {
  return new Date(value).toLocaleString();
}
</script>

<template>
  <Teleport to="body">
    <div v-if="session" class="share-mask" @click.self="emit('close')">
      <section class="share-dialog" role="dialog" aria-modal="true" aria-label="Session 链接分享">
        <header>
          <div>
            <h2>链接分享</h2>
            <p>{{ session.name }}</p>
          </div>
          <button class="btn icon" aria-label="关闭" @click="emit('close')">×</button>
        </header>

        <label class="share-hours">
          <span>有效时间（小时）</span>
          <input
            v-model="hoursText"
            class="input mono"
            type="number"
            min="1"
            max="720"
            step="1"
          />
          <small v-if="hours === null">请输入 1～720 的整数</small>
        </label>

        <div v-if="links.length" class="share-result">
          <textarea class="input mono" :value="linkText" :rows="Math.max(3, links.length)" readonly />
          <span v-if="expiresAt" class="share-expiry">有效至 {{ formatExpiry(expiresAt) }}</span>
        </div>

        <footer>
          <button class="btn" @click="emit('close')">关闭</button>
          <button v-if="links.length" class="btn" @click="copyAll">复制全部</button>
          <button class="btn primary" :disabled="hours === null || generating" @click="generate">
            {{ generating ? "生成中…" : "生成分享链接" }}
          </button>
        </footer>
      </section>
    </div>
  </Teleport>
</template>

<style scoped>
.share-mask { position: fixed; inset: 0; z-index: 100001; display: grid; place-items: center; padding: 24px; background: rgba(0, 0, 0, .38); }
.share-dialog { width: min(620px, 100%); padding: 18px; border: 1px solid var(--border-strong); border-radius: var(--radius-lg); background: var(--bg-elevated); box-shadow: var(--shadow-window); }
header, footer { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
header h2 { margin: 0; font-size: 16px; }
header p { margin: 4px 0 0; color: var(--text-secondary); font-size: 12px; }
.share-hours { display: grid; grid-template-columns: 1fr 120px; align-items: center; gap: 8px 12px; margin: 18px 0; font-size: 12px; }
.share-hours small { grid-column: 2; color: var(--danger); }
.share-result { display: flex; flex-direction: column; gap: 7px; margin-bottom: 18px; }
.share-result textarea { min-height: 84px; resize: vertical; white-space: pre; }
.share-expiry { color: var(--text-secondary); font-size: 11px; }
footer { justify-content: flex-end; }
</style>
