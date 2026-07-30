<script setup lang="ts">
import { closeContextMenu, contextMenuState } from "../stores/dialog";

function run(item: (typeof contextMenuState.items)[number]): void {
  if (item.disabled) return;
  closeContextMenu();
  item.action();
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="contextMenuState.visible"
      class="ctx-mask"
      @click="closeContextMenu"
      @contextmenu.prevent="closeContextMenu"
    >
      <div
        class="ctx-menu"
        :style="{ left: contextMenuState.x + 'px', top: contextMenuState.y + 'px' }"
      >
        <template
          v-for="(item, i) in contextMenuState.items"
          :key="i"
        >
          <div v-if="item.dividerBefore" class="ctx-divider" />
          <button
            class="ctx-item"
            :class="{ danger: item.danger }"
            :disabled="item.disabled"
            @click="run(item)"
          >
            <component :is="item.icon" v-if="item.icon" :size="14" class="ctx-icon" />
            <span>{{ item.label }}</span>
          </button>
        </template>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.ctx-mask {
  position: fixed;
  inset: 0;
  z-index: 100002;
}
.ctx-menu {
  position: fixed;
  min-width: 160px;
  padding: 5px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-popup);
  display: flex;
  flex-direction: column;
  max-height: calc(100vh - 16px);
  overflow-y: auto;
}
.ctx-divider {
  height: 1px;
  margin: 5px 4px;
  background: var(--border);
}
.ctx-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text);
  font: inherit;
  font-size: 12px;
  text-align: left;
  cursor: pointer;
}
.ctx-item:hover:not(:disabled) {
  background: var(--bg-hover);
}
.ctx-item.danger {
  color: var(--danger);
}
.ctx-item:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}
.ctx-icon {
  flex: none;
}
</style>
