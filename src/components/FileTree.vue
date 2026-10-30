<script setup lang="ts" generic="T">
import { computed, nextTick, ref, watch } from "vue";
import { Io5Add, Io5ChevronDown, Io5ChevronForward, Io5FolderOutline } from "vue-icons-plus/io5";
import { fileKind, type FileKind } from "../utils/file-kind";
import {
  fileTreeRows,
  revealFilePath,
  type FileTreeEntry,
  type FileTreeRow,
} from "../utils/file-tree";
import { FILE_KIND_ICONS } from "./fileIcons";
import VirtualList from "./VirtualList.vue";

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

/** 虚拟列表要求固定行高。 */
const ROW_HEIGHT = 26;

const collapsed = ref(new Set<string>());
const rows = computed(() => fileTreeRows(props.entries, collapsed.value));

/** 内联新建表单作为一行插入列表（与树行同高），而不是浮在列表上方。 */
type TreeItem =
  { kind: "row"; row: FileTreeRow<T> } | { kind: "create"; path: string; depth: number };

const items = computed<TreeItem[]>(() => {
  const list: TreeItem[] = rows.value.map((row) => ({ kind: "row" as const, row }));
  const directory = props.creatingIn;
  if (directory === null || directory === undefined) return list;
  const anchor =
    directory === ""
      ? -1
      : list.findIndex((item) => item.kind === "row" && item.row.path === directory);
  const at = anchor < 0 ? 0 : anchor + 1;
  const depth = anchor < 0 || list[anchor]?.kind !== "row" ? 0 : list[anchor].row.depth + 1;
  list.splice(at, 0, { kind: "create", path: directory, depth });
  return list;
});

const list = ref<{ scrollToIndex(index: number): void } | null>(null);
watch(
  () => [props.selected, props.entries] as const,
  () => {
    if (props.selected) revealFilePath(collapsed.value, props.selected);
  },
  { immediate: true },
);
watch(
  () => props.creatingIn,
  async (directory) => {
    if (directory === null || directory === undefined) return;
    if (directory) revealFilePath(collapsed.value, directory);
    await nextTick();
    const index = items.value.findIndex((item) => item.kind === "create");
    if (index >= 0) list.value?.scrollToIndex(index);
  },
);

function toggle(path: string): void {
  if (collapsed.value.has(path)) collapsed.value.delete(path);
  else collapsed.value.add(path);
}

/** 脚本窗口的 path 是不带扩展名的逻辑名（label 才带 .lua），所以优先按 label 判类型。 */
function iconKind(path: string, label: string): FileKind {
  const kind = fileKind(label);
  return kind === "file" ? fileKind(path) : kind;
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
    <div v-if="!items.length" class="empty-hint file-tree-empty">{{ emptyHint }}</div>
    <VirtualList v-else ref="list" class="file-tree-rows" :items="items" :item-height="ROW_HEIGHT">
      <template #default="{ item }">
        <div
          v-if="item.kind === 'create'"
          class="file-tree-create"
          :style="{ paddingLeft: `${8 + item.depth * 14}px` }"
        >
          <slot name="create" />
        </div>
        <div
          v-else
          class="file-tree-row"
          :class="{ active: !!item.row.entry && selected === item.row.path }"
          :style="{ paddingLeft: `${8 + item.row.depth * 14}px` }"
        >
          <button
            type="button"
            class="file-tree-item"
            :aria-expanded="item.row.entry ? undefined : !collapsed.has(item.row.path)"
            :aria-current="item.row.entry && selected === item.row.path ? 'true' : undefined"
            :title="item.row.entry?.title ?? item.row.path"
            @click="item.row.entry ? emit('select', item.row.entry) : toggle(item.row.path)"
          >
            <span class="file-tree-chevron">
              <component
                v-if="!item.row.entry"
                :is="collapsed.has(item.row.path) ? Io5ChevronForward : Io5ChevronDown"
                :size="12"
              />
            </span>
            <component
              :is="
                item.row.entry
                  ? FILE_KIND_ICONS[iconKind(item.row.path, item.row.label)]
                  : Io5FolderOutline
              "
              :size="14"
              class="file-tree-icon"
            />
            <span class="file-tree-name">{{ item.row.label }}</span>
            <slot v-if="item.row.entry" name="suffix" :entry="item.row.entry" />
          </button>
          <button
            v-if="!item.row.entry && canCreate"
            type="button"
            class="btn icon file-tree-directory-add"
            :title="`在 ${item.row.path} 中新增文件`"
            :aria-label="`在 ${item.row.path} 中新增文件`"
            :disabled="createDisabled"
            @click="
              collapsed.delete(item.row.path);
              emit('create', item.row.path);
            "
          >
            <Io5Add :size="14" />
          </button>
          <span v-if="item.row.entry && $slots.actions" class="file-tree-actions">
            <slot name="actions" :entry="item.row.entry" />
          </span>
        </div>
      </template>
    </VirtualList>
  </div>
</template>

<style scoped>
.file-tree {
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  font-size: 12px;
}
.file-tree-root-add {
  display: flex;
  justify-content: center;
  align-items: center;
  flex: none;
  width: calc(100% - 16px);
  height: 26px;
  margin: 4px 8px;
  border: 1px dashed var(--border);
  border-radius: var(--radius-md);
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
}
.file-tree-root-add:hover:not(:disabled) {
  background: var(--bg-hover);
  border-color: var(--border-strong);
  color: var(--text);
}
.file-tree-create {
  height: 26px;
  display: flex;
  align-items: center;
  overflow: hidden;
}
.file-tree-rows {
  flex: 1;
  min-height: 0;
}
.file-tree-directory-add {
  flex: none;
}
.file-tree-empty {
  padding: var(--space-3);
  text-align: center;
}
.file-tree-row {
  height: 26px;
  display: flex;
  align-items: center;
  gap: var(--space-1);
  padding: 0 5px 0 8px;
  overflow: hidden;
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
.file-tree-icon,
.file-tree-chevron {
  flex: none;
}
.file-tree-icon {
  color: var(--text-secondary);
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
