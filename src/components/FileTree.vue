<script setup lang="ts" generic="T">
import { computed, ref, watch } from "vue";
import {
  Io5Add,
  Io5ChevronDown,
  Io5ChevronForward,
  Io5DocumentOutline,
  Io5FolderOutline,
} from "vue-icons-plus/io5";
import { fileTreeRows, revealFilePath, type FileTreeEntry } from "../utils/file-tree";

const props = defineProps<{
  entries: readonly FileTreeEntry<T>[];
  selected?: string | null;
  emptyHint?: string;
  canCreate?: boolean;
  creatingIn?: string | null;
  createDisabled?: boolean;
}>();
const emit = defineEmits<{ select: [entry: FileTreeEntry<T>]; create: [directory: string] }>();
defineSlots<{
  create?(): unknown;
  suffix?(props: { entry: FileTreeEntry<T> }): unknown;
  actions?(props: { entry: FileTreeEntry<T> }): unknown;
}>();
const collapsed = ref(new Set<string>());
const rows = computed(() => fileTreeRows(props.entries, collapsed.value));
watch(
  () => [props.selected, props.entries] as const,
  () => {
    if (props.selected) revealFilePath(collapsed.value, props.selected);
  },
  { immediate: true },
);

function toggle(path: string): void {
  if (collapsed.value.has(path)) collapsed.value.delete(path);
  else collapsed.value.add(path);
}
</script>

<template>
  <div class="file-tree">
    <button
      v-if="canCreate"
      type="button"
      class="file-tree-root-add"
      title="在根目录中新增文件"
      aria-label="在根目录中新增文件"
      :disabled="createDisabled"
      @click="emit('create', '')"
    >
      <Io5Add :size="16" />
    </button>
    <slot v-if="creatingIn === ''" name="create" />
    <div v-if="!rows.length" class="empty-hint file-tree-empty">{{ emptyHint }}</div>
    <template v-for="row in rows" :key="`${row.entry ? 'file' : 'dir'}:${row.path}`">
      <div
        class="file-tree-row"
        :class="{ active: !!row.entry && selected === row.path }"
        :style="{ paddingLeft: `${8 + row.depth * 14}px` }"
      >
        <button
          type="button"
          class="file-tree-item"
          :aria-expanded="row.entry ? undefined : !collapsed.has(row.path)"
          :aria-current="row.entry && selected === row.path ? 'true' : undefined"
          :title="row.entry?.title ?? row.path"
          @click="row.entry ? emit('select', row.entry) : toggle(row.path)"
        >
          <span class="file-tree-chevron">
            <component
              v-if="!row.entry"
              :is="collapsed.has(row.path) ? Io5ChevronForward : Io5ChevronDown"
              :size="12"
            />
          </span>
          <component :is="row.entry ? Io5DocumentOutline : Io5FolderOutline" :size="14" />
          <span class="file-tree-name">{{ row.label }}</span>
          <slot v-if="row.entry" name="suffix" :entry="row.entry" />
        </button>
        <button
          v-if="!row.entry && canCreate"
          type="button"
          class="btn icon file-tree-directory-add"
          :title="`在 ${row.path} 中新增文件`"
          :aria-label="`在 ${row.path} 中新增文件`"
          :disabled="createDisabled"
          @click="
            collapsed.delete(row.path);
            emit('create', row.path);
          "
        >
          <Io5Add :size="14" />
        </button>
        <span v-if="row.entry && $slots.actions" class="file-tree-actions">
          <slot name="actions" :entry="row.entry" />
        </span>
      </div>
      <slot v-if="!row.entry && creatingIn === row.path" name="create" />
    </template>
  </div>
</template>

<style scoped>
.file-tree {
  min-height: 0;
  overflow: auto;
  font-size: 12px;
}
.file-tree-root-add {
  display: flex;
  justify-content: center;
  align-items: center;
  width: calc(100% - 16px);
  height: 26px;
  margin: 4px 8px;
  border: 1px dashed var(--border);
  background: transparent;
  color: inherit;
  cursor: pointer;
}
.file-tree-root-add:hover {
  background: var(--bg-hover);
}
.file-tree-directory-add {
  flex: none;
}
.file-tree-empty {
  padding: var(--space-3);
  text-align: center;
}
.file-tree-row {
  min-height: 26px;
  display: flex;
  align-items: center;
  gap: var(--space-1);
  padding: 0 5px 0 8px;
}
.file-tree-item {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 5px;
  height: 26px;
  padding: 0;
  border: 0;
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: left;
  cursor: pointer;
}
.file-tree-item > svg,
.file-tree-chevron {
  flex: none;
}
.file-tree-chevron {
  width: 12px;
  display: flex;
}
.file-tree-row:focus-within {
  outline: 1px solid var(--accent);
  outline-offset: -1px;
}
.file-tree-row:hover {
  background: var(--bg-hover);
}
.file-tree-row.active {
  background: var(--bg-selected);
}
.file-tree-name {
  min-width: 0;
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.file-tree-actions {
  display: none;
  flex: none;
  align-items: center;
  gap: 1px;
}
.file-tree-row:hover .file-tree-actions,
.file-tree-row:focus-within .file-tree-actions,
.file-tree-row.active .file-tree-actions {
  display: flex;
}
</style>
