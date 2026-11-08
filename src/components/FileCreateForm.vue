<script setup lang="ts">
import { onMounted, ref } from "vue";
import { Io5Checkmark, Io5Close } from "vue-icons-plus/io5";
import { t } from "../i18n";
defineProps<{ directory: string; busy?: boolean; error?: string; confirmText?: string }>();
const name = defineModel<string>({ required: true });
const emit = defineEmits<{ submit: []; cancel: [] }>();
const input = ref<HTMLInputElement | null>(null);

onMounted(() => {
  input.value?.focus();
  input.value?.select();
});
</script>

<template>
  <form
    class="file-create"
    @submit.prevent="emit('submit')"
    @keydown.esc.stop="!busy && emit('cancel')"
  >
    <span v-if="directory" class="file-create-directory mono" :title="directory"
      >{{ directory }}/</span
    >
    <input
      ref="input"
      v-model="name"
      class="input file-create-input"
      :aria-label="t('新文件名或相对路径')"
      :aria-invalid="!!error"
      :disabled="busy"
      :class="{ invalid: !!error }"
      :title="error || (directory ? t('在 {dir} 中新建', { dir: directory }) : t('在根目录中新建'))"
      :placeholder="t('文件名或子目录/文件名')"
    />
    <button
      type="submit"
      class="btn icon file-create-action"
      :disabled="busy"
      :title="busy ? t('处理中…') : confirmText || t('创建')"
      :aria-label="confirmText || t('创建')"
    >
      <Io5Checkmark :size="14" />
    </button>
    <button
      type="button"
      class="btn icon file-create-action"
      :disabled="busy"
      :title="t('取消')"
      :aria-label="t('取消')"
      @click="emit('cancel')"
    >
      <Io5Close :size="14" />
    </button>
  </form>
</template>

<style scoped>
.file-create {
  flex: 1;
  min-width: 0;
  height: 26px;
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
}
.file-create-directory {
  flex: none;
  max-width: 40%;
  overflow: hidden;
  color: var(--text-faint);
  text-overflow: ellipsis;
  white-space: nowrap;
}
.file-create-input {
  flex: 1;
  min-width: 0;
  height: 20px;
  padding: 0 6px;
  font-size: 12px;
}
.file-create-input.invalid {
  border-color: var(--danger);
}
.file-create-action {
  flex: none;
  height: 20px;
  padding: 0 2px;
}
</style>
