<script setup lang="ts">
import { onBeforeUnmount, ref } from "vue";

defineProps<{
  title: string;
  detail?: string;
  status?: string;
}>();

const visible = ref(false);
const position = ref({ left: 0, top: 0 });
let timer: number | null = null;

function open(event: PointerEvent): void {
  const target = event.currentTarget as HTMLElement;
  timer = window.setTimeout(() => {
    const rect = target.getBoundingClientRect();
    const width = 240;
    position.value = {
      left: Math.max(8, Math.min(rect.left + rect.width / 2 - width / 2, window.innerWidth - width - 8)),
      top: Math.max(8, rect.top - 8),
    };
    visible.value = true;
  }, 260);
}

function close(): void {
  if (timer !== null) window.clearTimeout(timer);
  timer = null;
  visible.value = false;
}

onBeforeUnmount(close);
</script>

<template>
  <span class="tooltip-anchor" @pointerenter="open" @pointerleave="close" @click="close">
    <slot />
  </span>
  <Teleport to="body">
    <Transition name="tooltip">
      <div
        v-if="visible"
        class="app-tooltip"
        :style="{ left: `${position.left}px`, top: `${position.top}px` }"
      >
        <strong>{{ title }}</strong>
        <span v-if="detail">{{ detail }}</span>
        <span v-if="status" class="app-tooltip-status">{{ status }}</span>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.tooltip-anchor {
  display: inline-flex;
  align-items: center;
}

.app-tooltip {
  position: fixed;
  z-index: 100004;
  width: 240px;
  transform: translateY(-100%);
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 7px 9px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-md);
  background: var(--bg-elevated);
  color: var(--text);
  box-shadow: var(--shadow-popup);
  font-size: 11px;
  line-height: 1.45;
  pointer-events: none;
}
.app-tooltip strong {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: var(--font-mono);
  font-weight: 600;
}
.app-tooltip span {
  color: var(--text-secondary);
}
.app-tooltip-status {
  color: var(--text-faint) !important;
}

.tooltip-enter-active,
.tooltip-leave-active {
  transition:
    opacity 0.12s ease,
    transform 0.12s ease;
}
.tooltip-enter-from,
.tooltip-leave-to {
  opacity: 0;
  transform: translateY(calc(-100% + 3px));
}
</style>
