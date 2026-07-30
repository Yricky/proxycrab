<script setup lang="ts">
import { appStore } from "../stores/app";
import { Io5CheckmarkCircle, Io5CloseCircle, Io5Close, Io5InformationCircle } from "vue-icons-plus/io5";
</script>

<template>
  <div class="toast-container">
    <div v-for="toast in appStore.toasts" :key="toast.id" class="toast" :class="toast.kind">
      <Io5CheckmarkCircle v-if="toast.kind === 'success'" :size="15" class="toast-icon" />
      <Io5CloseCircle v-else-if="toast.kind === 'error'" :size="15" class="toast-icon" />
      <Io5InformationCircle v-else :size="15" class="toast-icon" />
      <span class="toast-message">{{ toast.message }}</span>
      <button class="btn icon" @click="appStore.dismissToast(toast.id)"><Io5Close :size="13" /></button>
    </div>
  </div>
</template>

<style scoped>
.toast-container {
  position: fixed;
  top: 52px;
  left: 50%;
  transform: translateX(-50%);
  display: flex;
  flex-direction: column;
  gap: 8px;
  z-index: 100000;
  pointer-events: none;
}
.toast {
  pointer-events: auto;
  display: flex;
  align-items: center;
  gap: 8px;
  max-width: 520px;
  padding: 8px 12px;
  border-radius: var(--radius-md);
  background: var(--bg-elevated);
  border: 1px solid var(--border-strong);
  box-shadow: var(--shadow-popup);
  font-size: 12px;
}
.toast.success .toast-icon {
  color: var(--success);
}
.toast.error .toast-icon {
  color: var(--danger);
}
.toast.info .toast-icon {
  color: var(--accent);
}
.toast-message {
  user-select: text;
  word-break: break-all;
}
</style>
