<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { Io5Checkmark, Io5Copy, Io5DownloadOutline, Io5Warning } from "vue-icons-plus/io5";
import { useBackend } from "../api";
import type { AssetMetadata } from "../api/types";
import { appStore } from "../stores/app";
import { copyText } from "../utils/clipboard";
import { fileLanguage } from "../utils/file-kind";
import { formatBytes } from "../utils/format";
import MonacoEditor from "./MonacoEditor.vue";
import { t } from "../i18n";

const props = defineProps<{ asset: AssetMetadata }>();
const backend = useBackend();

/** 自动加载上限；更大的文件需要显式确认，硬上限之外只提示下载。 */
const AUTO_LIMIT = 2 * 1024 * 1024;
const HARD_LIMIT = 32 * 1024 * 1024;
const GENERIC_TYPES = [
  "",
  "application/octet-stream",
  "binary/octet-stream",
  "application/binary",
  "application/unknown",
];

const bytes = ref<Uint8Array<ArrayBuffer> | null>(null);
const loading = ref(false);
const error = ref("");
const objectUrl = ref<string | null>(null);
const pretty = ref(true);
const copied = ref(false);
const downloading = ref(false);
let version = 0;
let copyTimer: number | undefined;

const mediaType = computed(() =>
  (props.asset.content_type.split(";")[0] ?? "").trim().toLowerCase(),
);
const readable = computed(() => typeof backend.readAsset === "function");

/** 能否预览按 mimetype 判定；通用二进制类型在读入后嗅探是否为文本。 */
const declared = computed<"image" | "audio" | "video" | "text" | "generic" | "unsupported">(() => {
  const media = mediaType.value;
  if (media.startsWith("image/")) return "image";
  if (media.startsWith("audio/")) return "audio";
  if (media.startsWith("video/")) return "video";
  if (GENERIC_TYPES.includes(media)) return "generic";
  if (media.startsWith("text/")) return "text";
  if (
    [
      "json",
      "xml",
      "javascript",
      "ecmascript",
      "yaml",
      "graphql",
      "lua",
      "x-sh",
      "shellscript",
      "x-python",
      "x-www-form-urlencoded",
    ].some((kind) => media.includes(kind))
  )
    return "text";
  return "unsupported";
});
const sniffed = computed<"text" | "unsupported" | null>(() => {
  if (declared.value !== "generic" || !bytes.value) return null;
  return bytes.value.subarray(0, 1024).includes(0) ? "unsupported" : "text";
});
const kind = computed(() =>
  declared.value === "generic" ? (sniffed.value ?? "generic") : declared.value,
);
const loadable = computed(() => kind.value !== "unsupported");
const tooBig = computed(() => props.asset.size > HARD_LIMIT);
const needsConfirm = computed(
  () =>
    readable.value &&
    loadable.value &&
    !bytes.value &&
    !loading.value &&
    !error.value &&
    props.asset.size > AUTO_LIMIT &&
    !tooBig.value,
);

const text = computed(() => (bytes.value ? new TextDecoder().decode(bytes.value) : ""));
const language = computed(() => {
  const media = mediaType.value;
  if (media.includes("json")) return "json";
  if (media.includes("html")) return "html";
  if (media.includes("xml") || media === "image/svg+xml") return "xml";
  if (media.includes("javascript")) return "javascript";
  if (media.includes("css")) return "css";
  if (media.includes("yaml")) return "yaml";
  return fileLanguage(props.asset.id);
});
/** 只有 JSON 类内容才有「格式化 / 原文」的意义。 */
const prettyCapable = computed(
  () => mediaType.value.includes("json") || /^[\s]*[[{]/.test(text.value),
);
const editorValue = computed(() => {
  const value = text.value;
  if (!pretty.value) return value;
  if (!mediaType.value.includes("json") && !/^[\s]*[[{]/.test(value)) return value;
  try {
    return JSON.stringify(JSON.parse(value), null, 2);
  } catch {
    return value;
  }
});
const filename = computed(() => props.asset.id.split("/").pop() || "asset");

function release(): void {
  if (objectUrl.value) {
    URL.revokeObjectURL(objectUrl.value);
    objectUrl.value = null;
  }
}

async function load(force = false): Promise<void> {
  release();
  bytes.value = null;
  error.value = "";
  if (!readable.value || !loadable.value || tooBig.value) return;
  if (!force && props.asset.size > AUTO_LIMIT) return;
  const token = ++version;
  loading.value = true;
  try {
    const data = await backend.readAsset!(props.asset.id);
    if (token !== version) return;
    bytes.value = data;
    if (["image", "audio", "video"].includes(declared.value))
      objectUrl.value = URL.createObjectURL(
        new Blob([data], { type: mediaType.value || "application/octet-stream" }),
      );
  } catch (cause) {
    if (token === version) error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    if (token === version) loading.value = false;
  }
}

async function copy(): Promise<void> {
  try {
    await copyText(editorValue.value);
    copied.value = true;
    if (copyTimer !== undefined) window.clearTimeout(copyTimer);
    copyTimer = window.setTimeout(() => (copied.value = false), 1200);
  } catch {
    appStore.toast(t("复制失败"), "error");
  }
}

async function download(): Promise<void> {
  if (!backend.saveFile || downloading.value) return;
  downloading.value = true;
  try {
    const data = bytes.value ?? (await backend.readAsset!(props.asset.id));
    const writer = await backend.saveFile({ filename: filename.value });
    if (!writer) return;
    await writer.append(data);
    await writer.markAsFinish();
  } catch (cause) {
    appStore.toast(t("下载失败：{message}", { message: cause instanceof Error ? cause.message : String(cause) }), "error");
  } finally {
    downloading.value = false;
  }
}

watch(
  () => [props.asset.id, props.asset.size] as const,
  () => {
    void load();
  },
  { immediate: true },
);
onBeforeUnmount(() => {
  version++;
  release();
  if (copyTimer !== undefined) window.clearTimeout(copyTimer);
});
</script>

<template>
  <section class="asset-pane">
    <div class="asset-toolbar">
      <span class="asset-name mono" :title="asset.id">{{ asset.id }}</span>
      <span
        class="asset-meta"
        :title="`${mediaType || t('未知类型')} · ${formatBytes(asset.size)} · SHA-256 ${asset.sha256}`"
        >{{ mediaType || t("未知类型") }} · {{ formatBytes(asset.size) }} ·
        <span class="mono">{{ asset.sha256.slice(0, 8) }}</span></span
      >
      <span class="asset-actions">
        <button
          v-if="readable"
          class="btn icon"
          type="button"
          :disabled="downloading"
          :title="downloading ? t('正在下载…') : t('下载资源')"
          :aria-label="downloading ? t('正在下载…') : t('下载资源')"
          @click="download"
        >
          <Io5DownloadOutline :size="13" />
        </button>
        <template v-if="kind === 'text' && bytes && prettyCapable">
          <button class="btn tool-btn" :class="{ on: pretty }" type="button" @click="pretty = true">
            {{ t("格式化") }}
          </button>
          <button
            class="btn tool-btn"
            :class="{ on: !pretty }"
            type="button"
            @click="pretty = false"
          >
            {{ t("原文") }}
          </button>
        </template>
        <template v-if="kind === 'text' && bytes">
          <button
            class="btn icon"
            type="button"
            :title="copied ? t('已复制') : t('复制内容')"
            :aria-label="copied ? t('已复制') : t('复制内容')"
            @click="copy"
          >
            <Io5Checkmark v-if="copied" :size="13" class="text-success" />
            <Io5Copy v-else :size="13" />
          </button>
        </template>
      </span>
    </div>

    <div class="asset-body">
      <p v-if="!readable" class="asset-state">{{ t("当前后端不提供资源内容，仅显示元数据。") }}</p>
      <p v-else-if="kind === 'unsupported'" class="asset-state">
        {{ t("该 mimetype（{type}）不支持预览。", { type: mediaType || t("未知") }) }}
      </p>
      <p v-else-if="tooBig" class="asset-state">
        {{
          t("体积 {size} 超过预览上限 {limit}，请下载后查看。", {
            size: formatBytes(asset.size),
            limit: formatBytes(HARD_LIMIT),
          })
        }}
      </p>
      <div v-else-if="needsConfirm" class="asset-state asset-limit">
        <Io5Warning :size="16" />
        <span>{{ t("体积 {size} 超过自动加载限制", { size: formatBytes(asset.size) }) }}</span>
        <button class="btn primary" type="button" @click="load(true)">{{ t("仍然加载") }}</button>
      </div>
      <p v-else-if="loading" class="asset-state">{{ t("加载中…") }}</p>
      <p v-else-if="error" class="asset-state asset-error">
        <Io5Warning :size="14" /><span>{{ error }}</span>
      </p>
      <div v-else-if="objectUrl" class="asset-media">
        <img v-if="kind === 'image'" :src="objectUrl" :alt="asset.id" />
        <audio v-else-if="kind === 'audio'" :src="objectUrl" controls />
        <video v-else-if="kind === 'video'" :src="objectUrl" controls />
      </div>
      <div v-else-if="kind === 'text' && bytes" class="asset-editor">
        <MonacoEditor :model-value="editorValue" :language="language" readonly />
      </div>
      <p v-else class="asset-state">{{ t("暂无内容。") }}</p>
    </div>
  </section>
</template>

<style scoped>
.asset-pane {
  flex: 1;
  min-height: 0;
  min-width: 0;
  display: flex;
  flex-direction: column;
}
.asset-toolbar {
  flex: none;
  min-width: 0;
  min-height: 42px;
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: 6px 10px;
  border-bottom: 1px solid var(--border);
  font-size: 12px;
}
.asset-name {
  flex: 1 1 auto;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 600;
}
.asset-meta {
  flex: 0 1 auto;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 11px;
  color: var(--text-secondary);
}
.asset-actions {
  flex: none;
  display: flex;
  align-items: center;
  gap: 6px;
}
.tool-btn {
  flex: none;
  padding: 2px 8px;
  font-size: 11px;
}
.tool-btn.on {
  background: var(--accent);
  border-color: var(--accent);
  color: var(--accent-text);
}
.asset-body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: auto;
}
.asset-state {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0;
  padding: 14px;
  color: var(--text-secondary);
  font-size: 12px;
}
.asset-limit {
  flex-wrap: wrap;
}
.asset-error {
  color: var(--danger);
}
.asset-media {
  flex: 1;
  min-height: 0;
  display: flex;
  align-items: flex-start;
  justify-content: center;
  padding: 12px;
  overflow: auto;
}
.asset-media img,
.asset-media video {
  max-width: 100%;
  max-height: 100%;
  object-fit: contain;
}
.asset-editor {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
</style>
