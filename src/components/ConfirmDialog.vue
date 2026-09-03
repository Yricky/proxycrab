<script setup lang="ts">
import { confirmState, settleConfirm } from "../stores/dialog";
</script>

<template>
  <Teleport to="body">
    <div v-if="confirmState.visible" class="confirm-mask" @click.self="settleConfirm('cancel')">
      <div class="confirm-box">
        <div class="confirm-title">{{ confirmState.title }}</div>
        <div class="confirm-message">{{ confirmState.message }}</div>
        <div class="confirm-actions">
          <button class="btn" @click="settleConfirm('cancel')">取消</button>
          <button
            v-if="confirmState.secondaryText"
            class="btn"
            @click="settleConfirm('secondary')"
          >
            {{ confirmState.secondaryText }}
          </button>
          <button
            class="btn"
            :class="confirmState.danger ? 'danger-solid' : 'primary'"
            @click="settleConfirm('confirm')"
          >
            {{ confirmState.confirmText }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.confirm-mask {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.35);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 100001;
}
.confirm-box {
  width: 360px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-window);
  padding: 16px;
}
.confirm-title {
  font-size: 14px;
  font-weight: 600;
  margin-bottom: 8px;
}
.confirm-message {
  color: var(--text-secondary);
  font-size: 12px;
  margin-bottom: 16px;
  word-break: break-all;
  user-select: text;
}
.confirm-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
.danger-solid {
  background: var(--danger);
  border-color: var(--danger);
  color: #fff;
}
.danger-solid:hover:not(:disabled) {
  background: var(--danger-hover);
  border-color: var(--danger-hover);
}
</style>
