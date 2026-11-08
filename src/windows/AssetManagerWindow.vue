<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useBackend } from "../api";
import { t } from "../i18n";
import type { AssetMetadata } from "../api/generated/AssetMetadata";
import { reportError } from "../stores/app";
import AssetPreview from "../components/AssetPreview.vue";
import AssetTreePicker from "../components/AssetTreePicker.vue";
const backend = useBackend();
const assets = ref<AssetMetadata[]>([]);
const selected = ref<string | null>(null);
const loading = ref(false);
const current = computed(() => assets.value.find((asset) => asset.id === selected.value));
async function refresh(): Promise<void> {
  if (!backend.listAssets || loading.value) return;
  loading.value = true;
  try {
    assets.value = await backend.listAssets();
  } catch (error) {
    reportError(error, t("加载工作区资源失败"));
  } finally {
    loading.value = false;
  }
}
function uploaded(asset: AssetMetadata): void {
  assets.value = [...assets.value.filter((item) => item.id !== asset.id), asset];
  void refresh();
}
onMounted(refresh);
</script>
<template>
  <div class="asset-manager">
    <div class="asset-manager-content">
      <AssetTreePicker
        v-model:selected="selected"
        :assets="assets"
        class="asset-manager-tree"
        @uploaded="uploaded"
      />
      <AssetPreview v-if="current" class="asset-manager-preview" :asset="current" />
      <div v-else class="empty-hint asset-manager-empty">{{ t("选择资源查看信息") }}</div>
    </div>
  </div>
</template>
<style scoped>
.asset-manager {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.asset-manager-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-height: 42px;
  padding: 6px 12px;
  border-bottom: 1px solid var(--border);
  font-size: 12px;
}
.asset-manager-content {
  display: flex;
  flex: 1;
  min-height: 0;
}
.asset-manager-tree {
  width: 280px;
  flex: none;
  border-right: 1px solid var(--border);
}
.asset-manager-preview {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
}
.asset-manager-empty {
  flex: 1;
  padding: 14px;
}
</style>
