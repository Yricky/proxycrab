<script setup lang="ts">
import { computed, ref } from "vue";
import {
  Io5Checkmark,
  Io5ChevronDown,
  Io5ChevronForward,
  Io5FolderOutline,
} from "vue-icons-plus/io5";
import type { AssetMetadata } from "../api/generated/AssetMetadata";
import { fileTreeRows } from "../utils/file-tree";
import { fileKind } from "../utils/file-kind";
import { formatBytes } from "../utils/format";
import { t } from "../i18n";
import { FILE_KIND_ICONS } from "./fileIcons";
import VirtualList from "./VirtualList.vue";

const props = defineProps<{ assets: AssetMetadata[]; rules: string[]; disabled?: boolean }>();
const emit = defineEmits<{ rules: [rules: string[]] }>();

const collapsed = ref(new Set<string>());
const entries = computed(() => props.assets.map((asset) => ({ path: asset.id, data: asset })));
const rows = computed(() => fileTreeRows(entries.value, collapsed.value));

/** 祖先目录的规则会覆盖整棵子树，此时行内不可再单独取消。 */
function coveringRule(path: string): string | null {
  return props.rules.find((rule) => rule.endsWith("/") && path.startsWith(rule)) ?? null;
}
function state(path: string, isFile: boolean): "on" | "partial" | "covered" | "off" {
  if (isFile && props.rules.includes(path)) return "on";
  if (coveringRule(path)) return "covered";
  const own = `${path}/`;
  if (props.rules.includes(own)) return "on";
  if (props.rules.some((rule) => rule.startsWith(own))) return "partial";
  return "off";
}
function toggle(path: string, isFile: boolean): void {
  if (props.disabled || state(path, isFile) === "covered") return;
  if (isFile) {
    emit(
      "rules",
      props.rules.includes(path)
        ? props.rules.filter((rule) => rule !== path)
        : [...props.rules, path],
    );
    return;
  }
  const own = `${path}/`;
  emit(
    "rules",
    props.rules.includes(own)
      ? props.rules.filter((rule) => rule !== own)
      : [...props.rules.filter((rule) => !rule.startsWith(own)), own],
  );
}
const visible = computed(() =>
  rows.value.map((row) => {
    const checked = state(row.path, !!row.entry);
    return { ...row, checked, coveredBy: coveringRule(row.path) };
  }),
);

function toggleCollapse(path: string): void {
  if (collapsed.value.has(path)) collapsed.value.delete(path);
  else collapsed.value.add(path);
}
</script>

<template>
  <VirtualList class="asset-tree" :items="visible" :item-height="24">
    <template #default="{ item: row }">
      <div
        class="tree-row"
        :class="[row.checked, { static: disabled }]"
        :style="{ paddingLeft: 4 + row.depth * 14 + 'px' }"
        role="checkbox"
        :aria-checked="row.checked === 'partial' ? 'mixed' : row.checked !== 'off'"
        :aria-disabled="row.checked === 'covered' || disabled"
        :title="
          row.coveredBy
            ? t('已由 {name} 覆盖', { name: row.coveredBy })
            : row.entry
              ? `${row.path} · ${formatBytes(row.entry.data.size)}`
              : t('{path}/ 包含其下全部资源', { path: row.path })
        "
        tabindex="0"
        @click="toggle(row.path, !!row.entry)"
        @keydown.enter.prevent="toggle(row.path, !!row.entry)"
        @keydown.space.prevent="toggle(row.path, !!row.entry)"
      >
        <button
          v-if="!row.entry"
          type="button"
          class="tree-chevron"
          :aria-expanded="!collapsed.has(row.path)"
          :aria-label="collapsed.has(row.path) ? t('展开') : t('收起')"
          @click.stop="toggleCollapse(row.path)"
          @keydown.enter.stop.prevent="toggleCollapse(row.path)"
        >
          <component
            :is="collapsed.has(row.path) ? Io5ChevronForward : Io5ChevronDown"
            :size="11"
          />
        </button>
        <span v-else class="tree-chevron" />
        <span class="tree-check" :class="row.checked">
          <Io5Checkmark v-if="row.checked === 'on'" :size="10" />
          <span v-else-if="row.checked === 'partial'" class="dash" />
        </span>
        <component
          :is="row.entry ? FILE_KIND_ICONS[fileKind(row.path)] : Io5FolderOutline"
          :size="13"
          class="tree-icon"
        />
        <span class="tree-name">{{ row.label }}</span>
        <span v-if="row.entry" class="tree-size">{{ formatBytes(row.entry.data.size) }}</span>
      </div>
    </template>
  </VirtualList>
</template>

<style scoped>
.asset-tree {
  max-height: 240px;
  border-radius: var(--radius-sm);
}
.tree-row {
  display: flex;
  align-items: center;
  gap: 5px;
  min-height: 24px;
  padding-right: 4px;
  border-radius: var(--radius-sm);
  font-size: 12px;
  cursor: pointer;
}
.tree-row:hover {
  background: var(--bg-hover);
}
.tree-row.covered {
  cursor: default;
  color: var(--text-secondary);
}
.tree-row.static {
  cursor: default;
}
.tree-row:focus-visible {
  outline: 1px solid var(--accent);
  outline-offset: -1px;
}
.tree-chevron {
  flex: none;
  width: 12px;
  height: 12px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--text-faint);
  cursor: pointer;
}
.tree-check {
  flex: none;
  width: 14px;
  height: 14px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--border-strong);
  border-radius: 3px;
  background: var(--bg-panel);
  color: #fff;
}
.tree-check.on,
.tree-check.partial {
  border-color: var(--accent);
  background: var(--accent);
}
.tree-check.partial .dash {
  width: 8px;
  height: 2px;
  border-radius: 1px;
  background: #fff;
}
.tree-check.covered {
  border-color: var(--accent);
  background: color-mix(in srgb, var(--accent) 45%, transparent);
}
.tree-icon {
  flex: none;
  color: var(--text-secondary);
}
.tree-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.tree-size {
  flex: none;
  color: var(--text-faint);
  font-size: 11px;
}
</style>
