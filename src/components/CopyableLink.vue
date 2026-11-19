<script setup lang="ts">
import { Io5Copy } from "vue-icons-plus/io5";
import { useBackend } from "../api";
import { t } from "../i18n";
import { appStore, reportError } from "../stores/app";
import { copyText } from "../utils/clipboard";

const props = defineProps<{ url: string; copyTitle?: string }>();

const backend = useBackend();

async function copy(): Promise<void> {
  try {
    await copyText(props.url);
    appStore.toast(t("链接已复制"), "success");
  } catch (error) {
    reportError(error, t("复制失败"));
  }
}

async function open(): Promise<void> {
  try {
    await backend.openExternal(props.url);
  } catch (error) {
    reportError(error, t("打开链接失败"));
  }
}
</script>

<template>
  <div class="copyable-link">
    <a :href="url" class="mono" @click.prevent="open">{{ url }}</a>
    <button class="btn icon" type="button" :title="copyTitle ?? t('复制链接')" @click="copy">
      <Io5Copy :size="14" />
    </button>
  </div>
</template>

<style scoped>
.copyable-link {
  display: flex;
  align-items: center;
  min-width: 0;
  height: 32px;
  padding-left: 10px;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--bg-panel);
}

.copyable-link a {
  min-width: 0;
  flex: 1;
  overflow: hidden;
  color: var(--accent);
  text-overflow: ellipsis;
  white-space: nowrap;
  user-select: text;
}

.copyable-link a:hover {
  text-decoration: underline;
}

.copyable-link .btn {
  margin: 2px;
  flex: none;
}
</style>
