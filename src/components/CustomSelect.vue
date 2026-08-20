<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import { Io5Checkmark, Io5ChevronDown } from "vue-icons-plus/io5";

export interface CustomSelectOption {
  value: string;
  label: string;
  description?: string;
  disabled?: boolean;
}

const props = withDefaults(
  defineProps<{
    modelValue: string | null;
    options: CustomSelectOption[];
    placeholder?: string;
    disabled?: boolean;
    ariaLabel?: string;
    /** 菜单弹出方向，默认向下；底部状态栏等场景可设为 up。 */
    placement?: "down" | "up";
  }>(),
  { placeholder: "请选择", disabled: false, ariaLabel: "选择", placement: "down" },
);

const emit = defineEmits<{ "update:modelValue": [value: string] }>();
const root = ref<HTMLElement | null>(null);
const trigger = ref<HTMLButtonElement | null>(null);
const open = ref(false);
const activeIndex = ref(-1);

const selected = computed(() =>
  props.options.find((option) => option.value === props.modelValue),
);

function firstEnabled(start: number, direction: 1 | -1): number {
  if (props.options.length === 0) return -1;
  let index = start;
  for (let count = 0; count < props.options.length; count += 1) {
    index = (index + direction + props.options.length) % props.options.length;
    if (!props.options[index]?.disabled) return index;
  }
  return -1;
}

function openList(direction: 1 | -1 = 1): void {
  if (props.disabled) return;
  open.value = true;
  const selectedIndex = props.options.findIndex(
    (option) => option.value === props.modelValue && !option.disabled,
  );
  activeIndex.value =
    selectedIndex >= 0 ? selectedIndex : firstEnabled(direction === 1 ? -1 : 0, direction);
}

function closeList(focus = false): void {
  open.value = false;
  if (focus) void nextTick(() => trigger.value?.focus());
}

function choose(option: CustomSelectOption): void {
  if (option.disabled) return;
  emit("update:modelValue", option.value);
  closeList(true);
}

function move(direction: 1 | -1): void {
  const next = firstEnabled(activeIndex.value, direction);
  if (next >= 0) activeIndex.value = next;
}

function onKeydown(event: KeyboardEvent): void {
  if (props.disabled) return;
  if (!open.value) {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      openList(event.key === "ArrowDown" ? 1 : -1);
    } else if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      openList();
    }
    return;
  }
  if (event.key === "ArrowDown" || event.key === "ArrowUp") {
    event.preventDefault();
    move(event.key === "ArrowDown" ? 1 : -1);
  } else if (event.key === "Enter" || event.key === " ") {
    event.preventDefault();
    const option = props.options[activeIndex.value];
    if (option) choose(option);
  } else if (event.key === "Escape" || event.key === "Tab") {
    if (event.key === "Escape") event.preventDefault();
    closeList(event.key === "Escape");
  }
}

function onDocumentPointerDown(event: PointerEvent): void {
  if (!root.value?.contains(event.target as Node)) closeList();
}

watch(
  () => props.disabled,
  (disabled) => {
    if (disabled) closeList();
  },
);

watch(open, (isOpen) => {
  if (isOpen) document.addEventListener("pointerdown", onDocumentPointerDown);
  else document.removeEventListener("pointerdown", onDocumentPointerDown);
});
onBeforeUnmount(() => document.removeEventListener("pointerdown", onDocumentPointerDown));
</script>

<template>
  <div ref="root" class="custom-select" :class="{ open, disabled }">
    <button
      ref="trigger"
      type="button"
      class="select-trigger"
      role="combobox"
      aria-haspopup="listbox"
      :aria-label="ariaLabel"
      :aria-expanded="open"
      :disabled="disabled"
      @click="open ? closeList() : openList()"
      @keydown="onKeydown"
    >
      <span class="select-copy">
        <span :class="{ placeholder: !selected }">{{ selected?.label ?? placeholder }}</span>
        <small v-if="selected?.description">{{ selected.description }}</small>
      </span>
      <Io5ChevronDown :size="13" />
    </button>
    <div v-if="open" class="select-menu" :class="{ 'menu-up': placement === 'up' }" role="listbox" :aria-label="ariaLabel">
      <button
        v-for="(option, index) in options"
        :key="option.value"
        type="button"
        class="select-option"
        :class="{ active: index === activeIndex, selected: option.value === modelValue }"
        role="option"
        :aria-selected="option.value === modelValue"
        :aria-disabled="option.disabled || undefined"
        :disabled="option.disabled"
        @pointerenter="activeIndex = index"
        @click="choose(option)"
      >
        <span class="select-copy">
          <span>{{ option.label }}</span>
          <small v-if="option.description">{{ option.description }}</small>
        </span>
        <Io5Checkmark v-if="option.value === modelValue" :size="14" />
      </button>
    </div>
  </div>
</template>

<style scoped>
.custom-select { position: relative; min-width: 0; }
.select-trigger { width: 100%; min-height: 30px; display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 4px 8px 4px 10px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--bg-panel); color: var(--text); font: inherit; text-align: left; cursor: pointer; outline: none; }
.select-trigger:hover:not(:disabled) { border-color: var(--border-strong); background: var(--bg-hover); }
.select-trigger:focus-visible, .open .select-trigger { border-color: var(--accent); }
.select-trigger > svg { flex: none; color: var(--text-secondary); transition: transform 0.12s; }
.open .select-trigger > svg { transform: rotate(180deg); }
.select-copy { min-width: 0; display: flex; flex-direction: column; }
.select-copy > span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.select-copy small { overflow: hidden; color: var(--text-faint); font-size: 10px; text-overflow: ellipsis; white-space: nowrap; }
.placeholder { color: var(--text-faint); }
.select-menu { position: absolute; z-index: 100006; top: calc(100% + 4px); left: 0; right: 0; min-width: 150px; max-height: 280px; overflow-y: auto; padding: 4px; border: 1px solid var(--border-strong); border-radius: var(--radius-md); background: var(--bg-elevated); box-shadow: var(--shadow-popup); }
.select-menu.menu-up { top: auto; bottom: calc(100% + 4px); }
.select-option { width: 100%; min-height: 30px; display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 5px 7px; border: 0; border-radius: var(--radius-sm); background: transparent; color: var(--text); font: inherit; text-align: left; cursor: pointer; }
.select-option.active:not(:disabled) { background: var(--bg-hover); }
.select-option.selected { color: var(--accent); }
.select-option:disabled { color: var(--text-faint); cursor: default; }
.select-option > svg { flex: none; }
.disabled { opacity: 0.55; }
</style>
