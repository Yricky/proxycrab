<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useBackend } from "../api";
import type { Script } from "../api/types";
import { scriptLabel } from "../utils/script-label";
import { reportError } from "../stores/app";
import CustomSelect from "./CustomSelect.vue";
const props = defineProps<{
  modelValue: string | null;
  disabled?: boolean;
  inheritLabel?: string;
  inheritedValueLabel?: string;
}>();
const emit = defineEmits<{ "update:modelValue": [value: string | null] }>();
const backend = useBackend();
const scripts = ref<Script[]>([]);
const INHERIT = "\0inherit";
const options = computed(() => [
  { value: INHERIT, label: props.inheritLabel ?? "未配置" },
  { value: "", label: "不脱敏" },
  ...scripts.value.map((script) => ({ value: script.name, label: scriptLabel(script) })),
  ...(props.modelValue && !scripts.value.some((s) => s.name === props.modelValue)
    ? [{ value: props.modelValue, label: `${props.modelValue}（脚本缺失）` }]
    : []),
]);
async function load(): Promise<void> {
  try {
    scripts.value = (await backend.listRedactionScripts?.()) ?? [];
  } catch (error) {
    reportError(error, "加载脱敏脚本失败");
  }
}
onMounted(() => {
  void load();
  window.addEventListener("redaction-scripts-changed", load);
});
onBeforeUnmount(() => window.removeEventListener("redaction-scripts-changed", load));
</script>
<template>
  <label class="redaction-select"
    ><span>脱敏脚本</span
    ><CustomSelect
      :model-value="modelValue === null ? INHERIT : modelValue"
      :display-value="modelValue === null ? inheritedValueLabel : undefined"
      :options="options"
      :disabled="disabled"
      @update:model-value="emit('update:modelValue', $event === INHERIT ? null : $event)"
  /></label>
</template>
<style scoped>
.redaction-select {
  display: flex;
  align-items: center;
  gap: 12px;
  font-size: 12px;
  margin: 10px 0;
}
.redaction-select > span {
  flex-shrink: 0;
}
</style>
