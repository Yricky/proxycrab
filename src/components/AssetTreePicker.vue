<script setup lang="ts">
import { computed, ref } from "vue";
import {
  Io5ChevronDown,
  Io5ChevronForward,
  Io5DocumentOutline,
  Io5FolderOutline,
} from "vue-icons-plus/io5";
import type { AssetMetadata } from "../api/types";

const props = defineProps<{ assets: AssetMetadata[] }>();
const selected = defineModel<string | null>("selected", { required: true });

interface AssetNode {
  name: string;
  path: string;
  depth: number;
  children: AssetNode[];
  asset: AssetMetadata | null;
}

function buildTree(assets: AssetMetadata[]): AssetNode[] {
  const root: AssetNode = {
    name: "",
    path: "",
    depth: -1,
    children: [],
    asset: null,
  };
  for (const asset of assets) {
    const parts = asset.id.split("/");
    let node = root;
    parts.forEach((part, index) => {
      const path = parts.slice(0, index + 1).join("/");
      let child = node.children.find((item) => item.name === part);
      if (!child) {
        child = { name: part, path, depth: index, children: [], asset: null };
        node.children.push(child);
      }
      if (index === parts.length - 1) child.asset = asset;
      node = child;
    });
  }
  const sort = (nodes: AssetNode[]) => {
    nodes.sort((a, b) =>
      a.asset === null && b.asset !== null
        ? -1
        : a.asset !== null && b.asset === null
          ? 1
          : a.name.localeCompare(b.name),
    );
    nodes.forEach((node) => sort(node.children));
  };
  sort(root.children);
  return root.children;
}

const tree = computed(() => buildTree(props.assets));
const collapsed = ref(new Set<string>());

const visibleNodes = computed(() => {
  const result: AssetNode[] = [];
  const walk = (nodes: AssetNode[]) => {
    for (const node of nodes) {
      result.push(node);
      if (node.asset === null && !collapsed.value.has(node.path)) {
        walk(node.children);
      }
    }
  };
  walk(tree.value);
  return result;
});

function toggle(path: string): void {
  const next = new Set(collapsed.value);
  if (next.has(path)) next.delete(path);
  else next.add(path);
  collapsed.value = next;
}

function formatSize(size: number): string {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${(size / 1024 / 1024).toFixed(1)} MB`;
}
</script>

<template>
  <div class="asset-tree">
    <div v-if="visibleNodes.length === 0" class="empty-hint text-faint">
      工作区暂无资源
    </div>
    <button
      v-for="node in visibleNodes"
      :key="node.path"
      class="asset-row mono"
      :class="{ active: node.asset !== null && selected === node.asset.id }"
      :style="{ paddingLeft: `${8 + node.depth * 14}px` }"
      :title="
        node.asset
          ? `${node.asset.content_type} · ${formatSize(node.asset.size)}`
          : node.path
      "
      @click="node.asset === null ? toggle(node.path) : (selected = node.asset.id)"
    >
      <template v-if="node.asset === null">
        <Io5ChevronDown v-if="!collapsed.has(node.path)" :size="12" />
        <Io5ChevronForward v-else :size="12" />
        <Io5FolderOutline :size="13" />
      </template>
      <Io5DocumentOutline v-else :size="13" />
      <span class="asset-name">{{ node.name }}</span>
      <span v-if="node.asset" class="asset-size text-faint">{{
        formatSize(node.asset.size)
      }}</span>
    </button>
  </div>
</template>

<style scoped>
.asset-tree {
  overflow: auto;
  font-size: 12px;
}

.asset-row {
  display: flex;
  align-items: center;
  gap: 4px;
  width: 100%;
  height: 22px;
  border: none;
  background: transparent;
  color: var(--text);
  cursor: pointer;
  text-align: left;
}

.asset-row:hover {
  background: var(--bg-hover);
}

.asset-row.active {
  background: var(--bg-selected);
}

.asset-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.asset-size {
  margin-left: auto;
  padding-right: 6px;
}

.empty-hint {
  padding: 8px;
}
</style>
