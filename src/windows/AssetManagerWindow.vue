<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useBackend } from "../api";
import type { AssetMetadata } from "../api/types";
import { reportError } from "../stores/app";
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
    reportError(error, "加载工作区资源失败");
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
    <div class="asset-manager-toolbar">
      <span>{{ assets.length }} 个资源</span
      ><button class="btn compact" :disabled="loading" @click="refresh">
        {{ loading ? "加载中…" : "刷新" }}
      </button>
    </div>
    <div class="asset-manager-content">
      <AssetTreePicker
        v-model:selected="selected"
        :assets="assets"
        class="asset-manager-tree"
        @uploaded="uploaded"
      />
      <dl v-if="current" class="asset-manager-detail">
        <dt>路径</dt>
        <dd>{{ current.id }}</dd>
        <dt>类型</dt>
        <dd>{{ current.content_type }}</dd>
        <dt>大小</dt>
        <dd>{{ current.size.toLocaleString() }} B</dd>
        <dt>SHA-256</dt>
        <dd class="mono">{{ current.sha256 }}</dd>
      </dl>
      <div v-else class="empty-hint asset-manager-detail">选择资源查看信息</div>
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
  padding: 8px 12px;
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
.asset-manager-detail {
  flex: 1;
  min-width: 0;
  margin: 0;
  padding: 12px;
  overflow: auto;
  font-size: 12px;
}
dt {
  color: var(--text-secondary);
  margin-bottom: 4px;
}
dd {
  margin: 0 0 14px;
  overflow-wrap: anywhere;
}
</style>
