<script setup lang="ts">
import { onMounted, ref } from "vue";
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
    <div class="text-faint file-create-directory">{{ directory || "根目录" }}/</div>
    <input
      ref="input"
      v-model="name"
      class="input"
      aria-label="新文件名或相对路径"
      :disabled="busy"
      placeholder="文件名或子目录/文件名"
    />
    <div v-if="error" class="text-danger">{{ error }}</div>
    <div class="file-create-actions">
      <button type="submit" class="btn compact primary" :disabled="busy">
        {{ busy ? "处理中…" : confirmText || "创建" }}
      </button>
      <button type="button" class="btn compact" :disabled="busy" @click="emit('cancel')">
        取消
      </button>
    </div>
  </form>
</template>
<style scoped>
.file-create {
  padding: 6px 8px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 12px;
}
.file-create-directory {
  overflow-wrap: anywhere;
}
.file-create-actions {
  display: flex;
  gap: 6px;
}
</style>
