<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { clamp } from "../utils/log-canvas";

const props = defineProps<{ text: string; x: number; y: number }>();
const emit = defineEmits<{ close: [] }>();
const field = ref<HTMLTextAreaElement | null>(null);
const viewport = ref({ width: window.innerWidth, height: window.innerHeight });
const height = ref(54);
const position = computed(() => ({
  left: `${clamp(props.x, 8, viewport.value.width - Math.min(520, viewport.value.width - 16) - 8)}px`,
  top: `${clamp(props.y, 8, viewport.value.height - height.value - 8)}px`,
  height: `${height.value}px`,
}));

function resize(): void {
  viewport.value = { width: window.innerWidth, height: window.innerHeight };
  height.value = Math.min(
    Math.max(54, field.value?.scrollHeight ?? 54),
    280,
    window.innerHeight - 16,
  );
}

onMounted(() => {
  resize();
  field.value?.focus({ preventScroll: true });
  window.addEventListener("resize", resize);
});
onBeforeUnmount(() => window.removeEventListener("resize", resize));
</script>

<template>
  <Teleport to="body">
    <div
      class="cell-popup-mask"
      @pointerdown.self="emit('close')"
      @contextmenu.self.prevent="emit('close')"
      @keydown.esc.stop.prevent="emit('close')"
    >
      <textarea
        ref="field"
        class="cell-popup input mono"
        :style="position"
        :value="text"
        aria-label="单元格完整内容"
        readonly
        spellcheck="false"
      />
    </div>
  </Teleport>
</template>

<style scoped>
.cell-popup-mask {
  position: fixed;
  inset: 0;
  z-index: 100001;
}
.cell-popup {
  position: fixed;
  width: min(520px, calc(100vw - 16px));
  padding: 8px;
  margin: 0;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-md);
  background: var(--bg-elevated);
  box-shadow: var(--shadow-popup);
  color: var(--text);
  font-size: 12px;
  line-height: 18px;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  overflow-y: auto;
  resize: none;
}
</style>
