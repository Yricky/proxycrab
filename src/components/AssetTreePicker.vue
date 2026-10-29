<script setup lang="ts">
import { computed, ref } from "vue";
import type { AssetMetadata } from "../api/types";
import { useBackend } from "../api";
import { selectFile } from "../api/select-file";
import { validateAssetPath } from "../utils/asset-path";
import { reportError } from "../stores/app";
import FileCreateForm from "./FileCreateForm.vue";
import FileTree from "./FileTree.vue";

const props = defineProps<{ assets: AssetMetadata[] }>();
const selected = defineModel<string | null>("selected", { required: true });
const emit = defineEmits<{ uploaded: [asset: AssetMetadata] }>();
const backend = useBackend();
const creatingIn = ref<string | null>(null);
const file = ref<File | null>(null);
const filename = ref("");
const busy = ref(false);
const error = ref("");
async function beginUpload(directory: string): Promise<void> {
  if (!backend.uploadAsset || busy.value) return;
  busy.value = true;
  try {
    const picked = await selectFile();
    if (!picked) return;
    file.value = picked;
    filename.value = picked.name;
    error.value = "";
    creatingIn.value = directory;
  } catch (cause) {
    reportError(cause, "选择文件失败");
  } finally {
    busy.value = false;
  }
}
async function upload(): Promise<void> {
  if (!file.value || creatingIn.value === null || !backend.uploadAsset || busy.value) return;
  const id = [creatingIn.value, filename.value].filter(Boolean).join("/");
  error.value = !filename.value ? "文件名不能为空" : (validateAssetPath(id) ?? "");
  if (error.value) return;
  busy.value = true;
  try {
    const asset = await backend.uploadAsset(id, file.value);
    emit("uploaded", asset);
    selected.value = asset.id;
    creatingIn.value = null;
    file.value = null;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    busy.value = false;
  }
}
const entries = computed(() =>
  props.assets.map((asset) => ({
    path: asset.id,
    title: `${asset.id} · ${asset.content_type} · ${formatSize(asset.size)}`,
    data: asset,
  })),
);

function formatSize(size: number): string {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${(size / 1024 / 1024).toFixed(1)} MB`;
}
</script>

<template>
  <FileTree
    class="mono"
    :entries="entries"
    :selected="selected"
    empty-hint="工作区暂无资源"
    :can-create="!!backend.uploadAsset"
    :creating-in="creatingIn"
    :create-disabled="busy"
    @create="beginUpload"
    @select="selected = $event.path"
  >
    <template #create>
      <FileCreateForm
        v-if="creatingIn !== null"
        v-model="filename"
        :key="creatingIn"
        :directory="creatingIn"
        :busy="busy"
        :error="error"
        confirm-text="上传"
        @submit="upload"
        @cancel="
          creatingIn = null;
          file = null;
        "
      />
    </template>
    <template #suffix="{ entry }">
      <span class="asset-size text-faint">{{ formatSize(entry.data.size) }}</span>
    </template>
  </FileTree>
</template>

<style scoped>
.asset-size {
  flex: none;
}
</style>
