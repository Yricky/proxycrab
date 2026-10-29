<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useBackend } from "../api";
import type { AssetMetadata } from "../api/types";
import { resourceMatches } from "../utils/bundle";
import { validateAssetPath } from "../utils/asset-path";
const props = defineProps<{ initial: string[] }>();
const emit = defineEmits<{ save: [rules: string[]]; cancel: [] }>();
const text = ref(props.initial.join("\n")),
  assets = ref<AssetMetadata[]>([]),
  loading = ref(true),
  error = ref("");
const rules = computed(() => [
  ...new Set(
    text.value
      .split(/\r?\n/)
      .map((s) => s.trim())
      .filter(Boolean),
  ),
]);
const directories = computed(() => {
  const result = new Set<string>();
  for (const a of assets.value) {
    const parts = a.id.split("/");
    for (let i = 1; i < parts.length; i++) result.add(parts.slice(0, i).join("/") + "/");
  }
  return [...result].sort();
});
const selected = computed(() =>
  resourceMatches(
    rules.value,
    assets.value.map((a) => a.id),
  ),
);
const totalSize = computed(() =>
  assets.value.filter((a) => selected.value.includes(a.id)).reduce((sum, a) => sum + a.size, 0),
);
const invalid = computed(() =>
  rules.value.find(
    (rule) =>
      validateAssetPath(rule.endsWith("/") ? rule.slice(0, -1) : rule) ||
      !resourceMatches(
        [rule],
        assets.value.map((a) => a.id),
      ).length,
  ),
);
function add(event: Event): void {
  const select = event.target as HTMLSelectElement;
  if (select.value) text.value = [...rules.value, select.value].join("\n");
  select.value = "";
}
onMounted(async () => {
  try {
    assets.value = await useBackend().listAssets();
  } catch (e) {
    error.value = String(e);
  } finally {
    loading.value = false;
  }
});
</script>
<template>
  <div class="asset-editor">
    <p>每行一个完整 asset ID 或以 / 结尾的目录前缀，允许留空。</p>
    <select class="select" aria-label="添加随附资源" :disabled="loading || !!error" @change="add">
      <option value="">从工作区添加文件或目录…</option>
      <optgroup label="目录前缀">
        <option v-for="dir in directories" :key="dir" :value="dir">{{ dir }}</option>
      </optgroup>
      <optgroup label="资源">
        <option v-for="asset in assets" :key="asset.id" :value="asset.id">{{ asset.id }}</option>
      </optgroup>
    </select>
    <textarea
      class="input"
      v-model="text"
      aria-label="随附资源规则"
      rows="4"
      placeholder="mock/login/&#10;certs/demo.pem"
      :disabled="loading"
    />
    <p v-if="loading">正在读取资源…</p>
    <p v-else-if="error" class="error">{{ error }}</p>
    <p v-else-if="invalid" class="error">资源 ID 无效或未匹配：{{ invalid }}</p>
    <details v-else>
      <summary>
        当前匹配 {{ selected.length }} 个资源 · {{ (totalSize / 1024).toFixed(1) }} KB
      </summary>
      <code v-for="id in selected" :key="id">{{ id }}</code>
    </details>
    <footer>
      <button class="btn" @click="emit('cancel')">取消</button
      ><button class="btn" :disabled="loading || !!error || !!invalid" @click="emit('save', rules)">
        确认
      </button>
    </footer>
  </div>
</template>
<style scoped>
.asset-editor {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px;
  border: 1px solid var(--border);
  border-radius: 4px;
}
p {
  margin: 0;
  font-size: 12px;
}
textarea,
select {
  width: 100%;
  font: inherit;
}
textarea,
code {
  font-family: monospace;
}
code {
  display: block;
  overflow-wrap: anywhere;
  font-size: 12px;
}
details {
  max-height: 160px;
  overflow: auto;
  font-size: 12px;
}
footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
.error {
  color: var(--danger);
}
</style>
