<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { Io5Checkmark, Io5Copy, Io5DownloadOutline, Io5Warning } from "vue-icons-plus/io5";
import { useBackend } from "../api";
import { t } from "../i18n";
import {
  BodyFetchError,
  DEFAULT_BODY_MAX_SIZE,
  UI_BODY_MAX_SIZE,
  type BodyTarget,
  type LoadedBody,
} from "../api/body";
import type { BodySide } from "../api/generated/BodySide";
import type { BodyPayload } from "../api/generated/BodyPayload";
import type { HeaderItem } from "../api/generated/HeaderItem";
import { appStore } from "../stores/app";
import { confirmDialog } from "../stores/dialog";
import { copyText as writeClipboardText } from "../utils/clipboard";
import { formatBytes } from "../utils/format";
import { bodyFilename } from "../utils/body-filename";
import { inspectGrpc, inspectProtobuf } from "../utils/protobuf";
import MonacoEditor from "./MonacoEditor.vue";
import SseBodyViewer from "./SseBodyViewer.vue";
import HexViewer from "./HexViewer.vue";
import { useHexViewerMenu } from "../utils/hex-viewer-menu";

const props = defineProps<{
  label: string;
  body: BodyPayload;
  headers: HeaderItem[];
  side: BodySide;
  target: BodyTarget;
  revision?: number;
  /** body.size 未知（如拦截器快照）时置为 true，体积在加载后按实际结果显示。 */
  unknownSize?: boolean;
}>();

const backend = useBackend();
const {
  selection: hexSelection,
  rightPane,
  contextMenu: hexContextMenu,
  copied: hexCopied,
} = useHexViewerMenu();
const loaded = ref<LoadedBody | null>(null);
const loading = ref(false);
const downloading = ref(false);
const error = ref<string | null>(null);
const reportedStoredSize = ref<number | null>(null);
const pretty = ref(true);
const aggregated = ref(false);
const copied = ref<string | null>(null);
const structured = ref("");
const objectUrl = ref<string | null>(null);
let loadVersion = 0;
let copiedTimer: number | undefined;

function header(name: string): string {
  return props.headers.find((item) => item.name.toLowerCase() === name)?.value ?? "";
}

function declaredMediaType(): string {
  return (loaded.value?.contentType ?? header("content-type"))
    .split(";", 1)[0]
    .trim()
    .toLowerCase();
}

function sniffMediaType(bytes: Uint8Array): string | null {
  const ascii = (start: number, end: number) => String.fromCharCode(...bytes.subarray(start, end));
  if (
    bytes.length >= 8 &&
    bytes.slice(0, 8).every((v, i) => v === [137, 80, 78, 71, 13, 10, 26, 10][i])
  )
    return "image/png";
  if (bytes[0] === 0xff && bytes[1] === 0xd8) return "image/jpeg";
  if (ascii(0, 6) === "GIF87a" || ascii(0, 6) === "GIF89a") return "image/gif";
  if (ascii(0, 4) === "RIFF" && ascii(8, 12) === "WEBP") return "image/webp";
  if (ascii(0, 3) === "ID3" || (bytes[0] === 0xff && (bytes[1] & 0xe0) === 0xe0))
    return "audio/mpeg";
  if (ascii(0, 4) === "RIFF" && ascii(8, 12) === "WAVE") return "audio/wav";
  if (ascii(0, 4) === "OggS") return "audio/ogg";
  if (ascii(0, 4) === "fLaC") return "audio/flac";
  if (bytes.length >= 12 && ascii(4, 8) === "ftyp") return "video/mp4";
  if (bytes[0] === 0x1a && bytes[1] === 0x45 && bytes[2] === 0xdf && bytes[3] === 0xa3)
    return "video/webm";
  return null;
}

const mediaType = computed(() => {
  const declared = declaredMediaType();
  if (/^(image|audio|video)\//.test(declared)) return declared;
  return loaded.value ? (sniffMediaType(loaded.value.bytes) ?? declared) : declared;
});

const previewKind = computed(() => {
  const media = mediaType.value;
  if (media === "text/event-stream") return "sse";
  if (media.startsWith("image/")) return "image";
  if (media.startsWith("audio/")) return "audio";
  if (media.startsWith("video/")) return "video";
  if (media === "application/grpc" || media === "application/grpc+proto") return "grpc";
  if (media.includes("protobuf") || media === "application/x-protobuf") return "protobuf";
  if (
    media.startsWith("text/") ||
    ["json", "xml", "javascript", "ecmascript", "x-www-form-urlencoded", "graphql", "yaml"].some(
      (kind) => media.includes(kind),
    )
  )
    return "text";
  return "unsupported";
});

const storedSize = computed(() => (props.body.type === "empty" ? 0 : props.body.size));
const sizeKnown = computed(
  () => !props.unknownSize || reportedStoredSize.value !== null || loaded.value !== null,
);
const effectiveStoredSize = computed(
  () =>
    reportedStoredSize.value ??
    (props.unknownSize ? (loaded.value?.decodedSize ?? 0) : storedSize.value),
);
const embeddedIdentity = computed(() => {
  if (props.body.type === "text") return props.body.content;
  if (props.body.type === "json") return JSON.stringify(props.body.content);
  return "";
});
const headerIdentity = computed(
  () => `${header("content-type")}\n${header("content-encoding")}\n${header("grpc-encoding")}`,
);
const canRequestFull = computed(() => effectiveStoredSize.value <= UI_BODY_MAX_SIZE);
const overDefaultLimit = computed(
  () => !loaded.value && effectiveStoredSize.value > DEFAULT_BODY_MAX_SIZE,
);

const decodedText = computed(() => {
  if (!loaded.value) return "";
  return new TextDecoder().decode(loaded.value.bytes);
});

const editorValue = computed(() => {
  if (previewKind.value === "protobuf" || previewKind.value === "grpc") return structured.value;
  const text = decodedText.value;
  if (pretty.value && (mediaType.value.includes("json") || /^[\s]*[\[{]/.test(text))) {
    try {
      return JSON.stringify(JSON.parse(text), null, 2);
    } catch {
      return text;
    }
  }
  return text;
});

const editorLanguage = computed(() => {
  const media = mediaType.value;
  if (previewKind.value === "protobuf" || previewKind.value === "grpc" || media.includes("json"))
    return "json";
  if (media.includes("html")) return "html";
  if (media.includes("xml") || media === "image/svg+xml") return "xml";
  if (media.includes("javascript") || media.includes("ecmascript")) return "javascript";
  if (media.includes("css")) return "css";
  return "plaintext";
});

function revokeObjectUrl(): void {
  if (objectUrl.value) URL.revokeObjectURL(objectUrl.value);
  objectUrl.value = null;
}

async function preparePreview(value: LoadedBody): Promise<void> {
  revokeObjectUrl();
  structured.value = "";
  if (["image", "audio", "video"].includes(previewKind.value)) {
    const buffer = value.bytes.slice().buffer as ArrayBuffer;
    objectUrl.value = URL.createObjectURL(new Blob([buffer], { type: mediaType.value }));
  } else if (previewKind.value === "protobuf") {
    structured.value = inspectProtobuf(value.bytes);
  } else if (previewKind.value === "grpc") {
    try {
      structured.value = await inspectGrpc(value.bytes, header("grpc-encoding").toLowerCase());
    } catch (cause) {
      structured.value = t("gRPC 解析失败：{error}", { error: String(cause) });
    }
  }
}

async function load(maxSize = DEFAULT_BODY_MAX_SIZE): Promise<void> {
  if (props.body.type === "empty") return;
  const version = ++loadVersion;
  loading.value = true;
  error.value = null;
  try {
    const value = await backend.fetchBody(props.target, props.side, maxSize);
    if (version !== loadVersion) return;
    loaded.value = value;
    await preparePreview(value);
  } catch (cause) {
    if (version !== loadVersion) return;
    loaded.value = null;
    revokeObjectUrl();
    if (cause instanceof BodyFetchError && cause.code === "body_too_large") {
      reportedStoredSize.value = cause.actualSize ?? null;
    } else {
      error.value = cause instanceof Error ? cause.message : String(cause);
    }
  } finally {
    if (version === loadVersion) loading.value = false;
  }
}

async function confirmFullLoad(): Promise<void> {
  if (!effectiveStoredSize.value || !canRequestFull.value) return;
  const accepted = await confirmDialog({
    title: t("加载完整 Body"),
    message: t("Body 实际体积为 {size}，加载会占用较多内存，是否继续？", {
      size: formatBytes(effectiveStoredSize.value),
    }),
    confirmText: t("继续加载"),
  });
  if (accepted) await load(effectiveStoredSize.value);
}

async function copy(key: string, value: string): Promise<void> {
  try {
    await writeClipboardText(value);
    copied.value = key;
    if (copiedTimer !== undefined) window.clearTimeout(copiedTimer);
    copiedTimer = window.setTimeout(() => (copied.value = null), 1200);
  } catch {
    appStore.toast(t("复制失败"), "error");
  }
}

async function download(): Promise<void> {
  if (!backend.downloadBody || downloading.value) return;
  downloading.value = true;
  try {
    await backend.downloadBody(
      props.target,
      props.side,
      bodyFilename(props.target, props.side, props.headers),
    );
  } catch (cause) {
    appStore.toast(
      t("Body 下载失败：{message}", {
        message: cause instanceof Error ? cause.message : String(cause),
      }),
      "error",
    );
  } finally {
    downloading.value = false;
  }
}

const bodyIdentity = computed(() =>
  JSON.stringify([
    props.target.kind,
    props.target.id,
    props.target.kind === "interceptor" ? props.target.executionId : undefined,
    props.target.kind !== "breakpoint" ? props.target.sessionId : undefined,
    props.side,
    headerIdentity.value,
    props.revision,
  ]),
);

watch(
  () => [bodyIdentity.value, props.body.type, storedSize.value, embeddedIdentity.value],
  ([identity], previous) => {
    ++loadVersion;
    if (
      previewKind.value !== "sse" ||
      identity !== previous?.[0] ||
      props.body.type === "empty" ||
      storedSize.value > DEFAULT_BODY_MAX_SIZE
    ) {
      loaded.value = null;
    }
    reportedStoredSize.value = null;
    error.value = null;
    revokeObjectUrl();
    if (props.body.type !== "empty") {
      if (!props.unknownSize && storedSize.value > DEFAULT_BODY_MAX_SIZE) {
        reportedStoredSize.value = storedSize.value;
      } else {
        void load();
      }
    }
  },
  { immediate: true },
);

onBeforeUnmount(() => {
  ++loadVersion;
  revokeObjectUrl();
  if (copiedTimer !== undefined) window.clearTimeout(copiedTimer);
});
</script>

<template>
  <section class="body-viewer">
    <div class="body-head">
      <span>{{ label }}</span>
      <span v-if="body.type !== 'empty' && sizeKnown" class="size-note">
        {{ t("实际体积 {size}", { size: formatBytes(effectiveStoredSize) }) }}
      </span>
      <span class="body-actions">
        <button
          v-if="previewKind === 'sse'"
          class="tool-btn"
          :class="{ on: aggregated }"
          :aria-pressed="aggregated"
          @click="aggregated = !aggregated"
        >
          {{ t("聚合查看") }}
        </button>
        <button
          v-else-if="body.type !== 'empty' && backend.downloadBody"
          class="btn icon"
          :disabled="downloading"
          :title="downloading ? t('正在下载 Body…') : t('下载 Body')"
          :aria-label="downloading ? t('正在下载 Body') : t('下载 Body')"
          @click="download"
        >
          <Io5DownloadOutline :size="13" />
        </button>
        <template v-if="loaded && previewKind === 'text'">
          <button class="tool-btn" :class="{ on: pretty }" @click="pretty = true">
            {{ t("格式化") }}
          </button>
          <button class="tool-btn" :class="{ on: !pretty }" @click="pretty = false">
            {{ t("原文") }}
          </button>
          <button class="btn icon" :title="t('复制 Body')" @click="copy('body', editorValue)">
            <Io5Checkmark v-if="copied === 'body'" :size="13" class="text-success" />
            <Io5Copy v-else :size="13" />
          </button>
        </template>
      </span>
    </div>

    <div v-if="body.type === 'empty'" class="body-state">{{ t("无 Body") }}</div>
    <div v-else-if="loading && !(loaded && previewKind === 'sse')" class="body-state">
      {{ t("加载中…") }}
    </div>
    <div v-else-if="overDefaultLimit" class="body-state body-limit">
      <Io5Warning :size="18" />
      <span>{{ t("Body 超过自动加载限制") }}</span>
      <button v-if="canRequestFull" class="btn primary" @click="confirmFullLoad">
        {{ t("加载完整 Body（{size}）", { size: formatBytes(effectiveStoredSize) }) }}
      </button>
      <span v-else>{{ t("Body 实际体积超过 1 GiB，无法在此加载") }}</span>
    </div>
    <div v-else-if="error" class="body-state body-limit">
      <Io5Warning :size="18" />
      <span>{{ error }}</span>
      <button class="btn" @click="load()">{{ t("重试") }}</button>
    </div>
    <SseBodyViewer
      v-else-if="loaded && previewKind === 'sse'"
      :text="decodedText"
      :aggregated="aggregated"
    />
    <div v-else-if="loaded && previewKind === 'image'" class="media-preview">
      <img :src="objectUrl ?? ''" :alt="t('Body 图片预览')" />
    </div>
    <div v-else-if="loaded && previewKind === 'audio'" class="media-preview">
      <audio :src="objectUrl ?? ''" controls />
    </div>
    <div v-else-if="loaded && previewKind === 'video'" class="media-preview">
      <video :src="objectUrl ?? ''" controls />
    </div>
    <div
      v-else-if="
        loaded && (previewKind === 'text' || previewKind === 'protobuf' || previewKind === 'grpc')
      "
      class="body-editor"
    >
      <MonacoEditor :model-value="editorValue" :language="editorLanguage" readonly />
    </div>
    <HexViewer
      v-else-if="loaded"
      ref="hexViewer"
      v-model:right-pane="rightPane"
      :bytes="loaded.bytes"
      @selection-change="hexSelection = $event"
      @contextmenu="hexContextMenu"
      @copied="hexCopied"
    />
  </section>
</template>

<style scoped>
.body-viewer {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--bg-panel);
  overflow: hidden;
}
.body-head {
  flex: none;
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 24px;
  padding: 4px 8px;
  border-bottom: 1px solid var(--border);
  background: var(--bg-app);
  color: var(--text-secondary);
  font-size: 11px;
  font-weight: 600;
}
.size-note {
  color: var(--text-faint);
  font-family: var(--font-mono);
  font-size: 10px;
  font-weight: 400;
}
.body-actions {
  margin-left: auto;
  display: inline-flex;
  align-items: center;
  gap: 3px;
}
.tool-btn {
  border: 1px solid var(--border);
  background: var(--bg-panel);
  color: var(--text-secondary);
  font: inherit;
  font-size: 10px;
  padding: 1px 8px;
  cursor: pointer;
}
.tool-btn.on {
  background: var(--accent);
  border-color: var(--accent);
  color: var(--accent-text);
}
.body-state {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  color: var(--text-faint);
  font-size: 11px;
  padding: 16px;
  text-align: center;
}
.body-limit {
  flex-direction: column;
}
.body-editor {
  flex: 1;
  min-height: 0;
  display: flex;
}
.media-preview {
  flex: 1;
  min-height: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: auto;
  padding: 12px;
  background: color-mix(in srgb, var(--bg-app) 75%, #000);
}
.media-preview img,
.media-preview video {
  max-width: 100%;
  max-height: 100%;
  object-fit: contain;
}
.media-preview audio {
  width: min(520px, 90%);
}
</style>
