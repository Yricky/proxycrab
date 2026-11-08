<script setup lang="ts">
import { computed, type Component } from "vue";
import { Io5Ban, Io5Checkmark, Io5HandLeft } from "vue-icons-plus/io5";
import { LiaStarOfLifeSolid } from "vue-icons-plus/lia";
import type { ApiActionView, PermissionMode } from "../api/types";
import { t } from "../i18n";
import AppTooltip from "./AppTooltip.vue";
import {
  ACTION_META,
  FUNCTION_GROUPS,
  IMPACT_GROUPS,
  OTHER_GROUP,
  type PermissionGroupDimension,
  functionGroupOf,
  impactGroupOf,
} from "../utils/permission-meta";

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

interface ModeOption {
  value: PermissionMode;
  label: string;
  description: string;
  icon: Component;
}

const ALL_MODES: ModeOption[] = [
  { value: "allow", label: "允许", description: "直接执行", icon: Io5Checkmark },
  { value: "approval", label: "审批", description: "桌面确认后执行", icon: Io5HandLeft },
  { value: "deny", label: "阻止", description: "直接拒绝", icon: Io5Ban },
];
const modes = computed(() =>
  ALL_MODES.filter((option) =>
    (props.allowedModes ?? ["allow", "approval", "deny"]).includes(option.value),
  ).map((option) => ({ ...option, label: t(option.label), description: t(option.description) })),
);

interface ActionGroup {
  key: string;
  label: string;
  actions: ApiActionView[];
}

const groups = computed<ActionGroup[]>(() => {
  const dimension = props.dimension ?? "function";
  const defs: ActionGroup[] = [
    ...(dimension === "impact" ? IMPACT_GROUPS : FUNCTION_GROUPS).map((group) => ({
      key: group.key,
      label: group.label,
      actions: [],
    })),
    { ...OTHER_GROUP, actions: [] },
  ];
  const byKey = new Map(defs.map((group) => [group.key, group]));
  for (const action of props.catalog) {
    const key = !ACTION_META[action.id]
      ? OTHER_GROUP.key
      : dimension === "impact"
        ? impactGroupOf(action.id)
        : functionGroupOf(action.route_template).key;
    byKey.get(key)?.actions.push(action);
  }
  return defs.filter((group) => group.actions.length > 0);
});

function actionLabel(action: ApiActionView): string {
  const label = ACTION_META[action.id]?.label;
  return label ? t(label) : action.route_template;
}

function groupMode(actions: ApiActionView[]): PermissionMode | typeof MIXED {
  const first = props.modelValue[actions[0]?.id ?? ""];
  return first && actions.every((action) => props.modelValue[action.id] === first) ? first : MIXED;
}

function actionMode(action: ApiActionView): PermissionMode {
  return props.modelValue[action.id] ?? action.default_mode;
}

function updateAction(actionId: string, mode: PermissionMode): void {
  emit("update:modelValue", {
    ...props.modelValue,
    [actionId]: mode,
  });
}

function updateGroup(actions: ApiActionView[], mode: PermissionMode): void {
  const next = { ...props.modelValue };
  for (const action of actions) next[action.id] = mode;
  emit("update:modelValue", next);
}
</script>

<template>
  <div class="permission-editor">
    <section v-for="group in groups" :key="group.key" class="permission-group">
      <header class="group-header">
        <div>
          <strong>{{ t(group.label) }}</strong>
          <span>{{ t("{n} 项", group.actions.length) }}</span>
        </div>
        <div
          class="mode-group"
          role="group"
          :aria-label="t('{label}整组权限', { label: t(group.label) })"
        >
          <button
            v-for="mode in modes"
            :key="mode.value"
            type="button"
            class="mode-btn"
            :class="[`m-${mode.value}`, { active: groupMode(group.actions) === mode.value }]"
            :title="
              t('{label}：{description}（应用到整组）', {
                label: mode.label,
                description: mode.description,
              })
            "
            :aria-pressed="groupMode(group.actions) === mode.value"
            :disabled="disabled"
            @click="updateGroup(group.actions, mode.value)"
          >
            <component :is="mode.icon" :size="12" />
          </button>
        </div>
      </header>
      <div v-for="action in group.actions" :key="action.id" class="permission-row">
        <span class="method mono">{{ action.method }}</span>
        <div class="action-copy">
          <span>{{ actionLabel(action) }}</span>
          <code>{{ action.route_template }}</code>
        </div>
        <div class="permission-controls">
          <AppTooltip
            v-if="ACTION_META[action.id]?.redaction"
            :title="t('支持脱敏')"
            :detail="t('绑定脱敏脚本后生效')"
          >
            <span class="redaction-indicator" :aria-label="t('支持脱敏：绑定脱敏脚本后生效')">
              <LiaStarOfLifeSolid :size="15" />
            </span>
          </AppTooltip>
          <div
            class="mode-group"
            role="group"
            :aria-label="t('{label}权限', { label: actionLabel(action) })"
          >
            <button
              v-for="mode in modes"
              :key="mode.value"
              type="button"
              class="mode-btn"
              :class="[`m-${mode.value}`, { active: actionMode(action) === mode.value }]"
              :title="
                t('{label}：{description}', { label: mode.label, description: mode.description })
              "
              :aria-pressed="actionMode(action) === mode.value"
              :disabled="disabled"
              @click="updateAction(action.id, mode.value)"
            >
              <component :is="mode.icon" :size="12" />
            </button>
          </div>
        </div>
      </div>
    </section>
  </div>
</template>

<style scoped>
.permission-editor {
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.permission-group {
  border-top: 1px solid var(--border);
}
.group-header {
  min-height: 44px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 7px 0;
}
.group-header > div {
  display: flex;
  align-items: baseline;
  gap: 7px;
}
.group-header strong {
  font-size: 12px;
}
.group-header span {
  color: var(--text-faint);
  font-size: 10px;
}
.permission-row {
  min-height: 42px;
  display: grid;
  grid-template-columns: 58px minmax(0, 1fr) auto;
  align-items: center;
  gap: 10px;
  padding: 5px 0 5px 8px;
  border-top: 1px solid color-mix(in srgb, var(--border) 70%, transparent);
}
.permission-row:hover {
  background: var(--bg-hover);
}
.method {
  font-size: 10px;
  font-weight: 700;
  color: var(--text-secondary);
}
.action-copy {
  min-width: 0;
  display: flex;
  align-items: baseline;
  gap: 10px;
}
.action-copy > span {
  flex: none;
  font-size: 12px;
}
.action-copy code {
  min-width: 0;
  overflow: hidden;
  color: var(--text-faint);
  font: 10px var(--font-mono);
  text-overflow: ellipsis;
  white-space: nowrap;
  user-select: text;
}
.permission-controls {
  display: flex;
  align-items: center;
  justify-self: end;
  gap: 8px;
}
.redaction-indicator {
  display: inline-flex;
  color: var(--text-secondary);
}
.mode-group {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  padding: 2px;
  border: 1px solid var(--border);
  border-radius: 999px;
  background: var(--bg-panel);
  justify-self: end;
}
.mode-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 18px;
  border: 0;
  border-radius: 999px;
  background: transparent;
  color: var(--text-faint);
  cursor: pointer;
}
.mode-btn:hover:not(:disabled):not(.active) {
  color: var(--text);
  background: var(--bg-hover);
}
.mode-btn.active.m-allow {
  background: color-mix(in srgb, var(--success) 18%, transparent);
  color: var(--success);
}
.mode-btn.active.m-approval {
  background: color-mix(in srgb, var(--warning) 18%, transparent);
  color: var(--warning);
}
.mode-btn.active.m-deny {
  background: color-mix(in srgb, var(--danger) 18%, transparent);
  color: var(--danger);
}
.mode-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}
@media (max-width: 680px) {
  .permission-row {
    grid-template-columns: 52px minmax(0, 1fr) auto;
  }
  .action-copy {
    display: block;
  }
  .action-copy code {
    display: block;
    margin-top: 2px;
  }
}
</style>
