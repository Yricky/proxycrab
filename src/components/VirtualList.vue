<script setup lang="ts">
import { computed, ref, type Directive } from "vue";

const props = withDefaults(
  defineProps<{
    items: unknown[];
    itemHeight: number;
    /** Extra rows rendered above/below the viewport. */
    buffer?: number;
  }>(),
  { buffer: 6 },
);

const emit = defineEmits<{ scroll: [event: Event] }>();

/** Local v-resize-observer directive to track viewport size. */
const vResizeObserver: Directive<HTMLElement, (rect: DOMRectReadOnly) => void> = {
  mounted(el, binding) {
    const observer = new ResizeObserver((entries) => {
      const rect = entries[0]?.contentRect;
      if (rect) binding.value(rect);
    });
    observer.observe(el);
    (el as unknown as Record<string, unknown>).__vlResizeObserver = observer;
    binding.value(el.getBoundingClientRect());
  },
  unmounted(el) {
    (
      (el as unknown as Record<string, ResizeObserver | undefined>).__vlResizeObserver as
        | ResizeObserver
        | undefined
    )?.disconnect();
  },
};

const scroller = ref<HTMLElement | null>(null);
const scrollTop = ref(0);
const viewportHeight = ref(0);

function onScroll(event: Event): void {
  const el = event.target as HTMLElement;
  scrollTop.value = el.scrollTop;
  emit("scroll", event);
}

function onViewportResize(rect: DOMRectReadOnly): void {
  viewportHeight.value = rect.height;
}

const startIndex = computed(() =>
  Math.max(0, Math.floor(scrollTop.value / props.itemHeight) - props.buffer),
);
const endIndex = computed(() =>
  Math.min(
    props.items.length,
    Math.ceil((scrollTop.value + viewportHeight.value) / props.itemHeight) + props.buffer,
  ),
);
const visibleItems = computed(() =>
  props.items.slice(startIndex.value, endIndex.value).map((item, i) => ({
    item,
    index: startIndex.value + i,
  })),
);
const totalHeight = computed(() => props.items.length * props.itemHeight);
const offsetY = computed(() => startIndex.value * props.itemHeight);

defineExpose({
  scrollToTop() {
    scroller.value?.scrollTo({ top: 0 });
  },
});
</script>

<template>
  <div
    ref="scroller"
    v-resize-observer="onViewportResize"
    class="virtual-list"
    @scroll.passive="onScroll"
  >
    <div class="vl-spacer" :style="{ height: totalHeight + 'px' }">
      <div class="vl-window" :style="{ transform: `translateY(${offsetY}px)` }">
        <slot
          v-for="entry in visibleItems"
          :key="entry.index"
          :item="entry.item"
          :index="entry.index"
        />
      </div>
    </div>
  </div>
</template>

<style scoped>
.virtual-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  overflow-x: hidden;
  position: relative;
}
.vl-spacer {
  position: relative;
  width: 100%;
}
.vl-window {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
}
</style>
