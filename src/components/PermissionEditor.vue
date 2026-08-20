<script setup lang="ts">
import { computed } from "vue";
import type { ApiActionView, PermissionMode } from "../api/types";
import {
  ACTION_LABELS,
  FUNCTION_GROUPS,
  IMPACT_GROUPS,
  OTHER_GROUP,
  type PermissionGroupDimension,
  functionGroupOf,
  impactGroupOf,
} from "../utils/permission-meta";
import CustomSelect, { type CustomSelectOption } from "./CustomSelect.vue";

const MIXED = "mixed";
const props = defineProps<{
  catalog: ApiActionView[];
  modelValue: Record<string, PermissionMode>;
  disabled?: boolean;
  dimension?: PermissionGroupDimension;
  allowedModes?: PermissionMode[];
}>();
const emit = defineEmits<{
  "update:modelValue": [value: Record<string, PermissionMode>];
}>();

const allModeOptions: CustomSelectOption[] = [
  { value: "allow", label: "允许", description: "直接执行" },
  { value: "approval", label: "审批", description: "桌面确认后执行" },
  { value: "deny", label: "阻止", description: "直接拒绝" },
];
const modeOptions = computed(() =>
  allModeOptions.filter((option) =>
    (props.allowedModes ?? ["allow", "approval", "deny"]).includes(
      option.value as PermissionMode,
    ),
  ),
);
const groupOptions = computed<CustomSelectOption[]>(() => [
  { value: MIXED, label: "混合", disabled: true },
  ...modeOptions.value,
]);

interface ActionGroup {
  key: string;
  label: string;
  actions: ApiActionView[];
}

const groups = computed<ActionGroup[]>(() => {
  const dimension = props.dimension ?? "function";
  const defs: ActionGroup[] =
    dimension === "impact"
      ? IMPACT_GROUPS.map((group) => ({ key: group.key, label: group.label, actions: [] }))
      : [
          ...FUNCTION_GROUPS.map((group) => ({ key: group.key, label: group.label, actions: [] })),
          { ...OTHER_GROUP, actions: [] },
        ];
  const byKey = new Map(defs.map((group) => [group.key, group]));
  for (const action of props.catalog) {
    const key =
      dimension === "impact" ? impactGroupOf(action) : functionGroupOf(action.route_template).key;
    byKey.get(key)?.actions.push(action);
  }
  return defs.filter((group) => group.actions.length > 0);
});

function actionLabel(action: ApiActionView): string {
  return ACTION_LABELS[action.id] ?? action.route_template;
}

function groupMode(actions: ApiActionView[]): PermissionMode | typeof MIXED {
  const first = props.modelValue[actions[0]?.id ?? ""];
  return first && actions.every((action) => props.modelValue[action.id] === first)
    ? first
    : MIXED;
}

function updateAction(actionId: string, mode: string): void {
  emit("update:modelValue", {
    ...props.modelValue,
    [actionId]: mode as PermissionMode,
  });
}

function updateGroup(actions: ApiActionView[], mode: string): void {
  if (mode === MIXED) return;
  const next = { ...props.modelValue };
  for (const action of actions) next[action.id] = mode as PermissionMode;
  emit("update:modelValue", next);
}

function modeClass(mode: PermissionMode | typeof MIXED): string {
  return `mode-${mode}`;
}
</script>

<template>
  <div class="permission-editor">
    <section v-for="group in groups" :key="group.key" class="permission-group">
      <header class="group-header">
        <div>
          <strong>{{ group.label }}</strong>
          <span>{{ group.actions.length }} 项</span>
        </div>
        <CustomSelect
          class="mode-select group-mode"
          :class="modeClass(groupMode(group.actions))"
          :model-value="groupMode(group.actions)"
          :options="groupOptions"
          :disabled="disabled"
          :aria-label="`${group.label}整组权限`"
          @update:model-value="updateGroup(group.actions, $event)"
        />
      </header>
      <div
        v-for="action in group.actions"
        :key="action.id"
        class="permission-row"
      >
        <span class="method mono">{{ action.method }}</span>
        <div class="action-copy">
          <span>{{ actionLabel(action) }}</span>
          <code>{{ action.route_template }}</code>
        </div>
        <CustomSelect
          class="mode-select"
          :class="modeClass(modelValue[action.id] ?? action.default_mode)"
          :model-value="modelValue[action.id] ?? action.default_mode"
          :options="modeOptions"
          :disabled="disabled"
          :aria-label="`${actionLabel(action)}权限`"
          @update:model-value="updateAction(action.id, $event)"
        />
      </div>
    </section>
  </div>
</template>

<style scoped>
.permission-editor { display: flex; flex-direction: column; gap: 14px; }
.permission-group { border-top: 1px solid var(--border); }
.group-header { min-height: 44px; display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 7px 0; }
.group-header > div { display: flex; align-items: baseline; gap: 7px; }
.group-header strong { font-size: 12px; }
.group-header span { color: var(--text-faint); font-size: 10px; }
.permission-row { min-height: 42px; display: grid; grid-template-columns: 58px minmax(0, 1fr) 126px; align-items: center; gap: 10px; padding: 5px 0 5px 8px; border-top: 1px solid color-mix(in srgb, var(--border) 70%, transparent); }
.permission-row:hover { background: var(--bg-hover); }
.method { font-size: 10px; font-weight: 700; color: var(--text-secondary); }
.action-copy { min-width: 0; display: flex; align-items: baseline; gap: 10px; }
.action-copy > span { flex: none; font-size: 12px; }
.action-copy code { min-width: 0; overflow: hidden; color: var(--text-faint); font: 10px var(--font-mono); text-overflow: ellipsis; white-space: nowrap; user-select: text; }
.mode-select { width: 126px; justify-self: end; }
.group-mode { width: 126px; }
.mode-allow { --mode-color: var(--success); }
.mode-approval { --mode-color: var(--warning); }
.mode-deny { --mode-color: var(--danger); }
.mode-mixed { --mode-color: var(--text-faint); }
.mode-select :deep(.select-trigger) { border-left: 3px solid var(--mode-color); }
@media (max-width: 680px) {
  .permission-row { grid-template-columns: 52px minmax(0, 1fr) 112px; }
  .mode-select, .group-mode { width: 112px; }
  .action-copy { display: block; }
  .action-copy code { display: block; margin-top: 2px; }
}
</style>
