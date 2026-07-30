<script setup lang="ts">
import { Io5Close } from "vue-icons-plus/io5";
import { windowsStore, type WindowState } from "../stores/windows";

const props = defineProps<{ win: WindowState; minWidth?: number; minHeight?: number }>();

const MIN_W = props.minWidth ?? 280;
const MIN_H = props.minHeight ?? 180;

type ResizeDir = "n" | "s" | "e" | "w" | "ne" | "nw" | "se" | "sw";

function startDrag(event: PointerEvent): void {
  if (event.button !== 0) return;
  const startX = event.clientX;
  const startY = event.clientY;
  const origX = props.win.x;
  const origY = props.win.y;
  const onMove = (e: PointerEvent) => {
    const maxX = window.innerWidth - 80;
    const maxY = window.innerHeight - 40;
    props.win.x = Math.min(Math.max(origX + e.clientX - startX, -props.win.width + 120), maxX);
    props.win.y = Math.min(Math.max(origY + e.clientY - startY, 0), maxY);
  };
  const onUp = () => {
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUp);
  };
  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", onUp);
  windowsStore.focus(props.win.id);
  event.preventDefault();
}

function startResize(dir: ResizeDir, event: PointerEvent): void {
  if (event.button !== 0) return;
  const startX = event.clientX;
  const startY = event.clientY;
  const { x: ox, y: oy, width: ow, height: oh } = props.win;
  const onMove = (e: PointerEvent) => {
    const dx = e.clientX - startX;
    const dy = e.clientY - startY;
    if (dir.includes("e")) props.win.width = Math.max(ow + dx, MIN_W);
    if (dir.includes("s")) props.win.height = Math.max(oh + dy, MIN_H);
    if (dir.includes("w")) {
      const newW = Math.max(ow - dx, MIN_W);
      props.win.x = ox + (ow - newW);
      props.win.width = newW;
    }
    if (dir.includes("n")) {
      const newH = Math.max(oh - dy, MIN_H);
      props.win.y = oy + (oh - newH);
      props.win.height = newH;
    }
  };
  const onUp = () => {
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUp);
  };
  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", onUp);
  windowsStore.focus(props.win.id);
  event.preventDefault();
  event.stopPropagation();
}

const dirs: ResizeDir[] = ["n", "s", "e", "w", "ne", "nw", "se", "sw"];
</script>

<template>
  <div
    class="floating-window"
    :style="{
      left: win.x + 'px',
      top: win.y + 'px',
      width: win.width + 'px',
      height: win.height + 'px',
      zIndex: win.z,
    }"
    @pointerdown="windowsStore.focus(win.id)"
  >
    <div class="fw-titlebar" @pointerdown="startDrag">
      <span class="fw-title">{{ win.title }}</span>
      <button class="btn icon fw-close" title="关闭" @click="windowsStore.close(win.id)">
        <Io5Close :size="16" />
      </button>
    </div>
    <div class="fw-body">
      <component :is="win.component" v-bind="win.props" />
    </div>
    <div
      v-for="dir in dirs"
      :key="dir"
      class="fw-resize"
      :class="'fw-resize-' + dir"
      @pointerdown="startResize(dir, $event)"
    />
  </div>
</template>

<style scoped>
.floating-window {
  position: fixed;
  display: flex;
  flex-direction: column;
  background: var(--bg-elevated);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-window);
  overflow: hidden;
}

.fw-titlebar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 34px;
  padding: 0 6px 0 12px;
  background: var(--bg-panel);
  border-bottom: 1px solid var(--border);
  cursor: move;
  flex: none;
}

.fw-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.fw-close {
  flex: none;
}

.fw-body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.fw-resize {
  position: absolute;
  z-index: 10;
}
.fw-resize-n {
  top: -3px;
  left: 8px;
  right: 8px;
  height: 6px;
  cursor: ns-resize;
}
.fw-resize-s {
  bottom: -3px;
  left: 8px;
  right: 8px;
  height: 6px;
  cursor: ns-resize;
}
.fw-resize-e {
  right: -3px;
  top: 8px;
  bottom: 8px;
  width: 6px;
  cursor: ew-resize;
}
.fw-resize-w {
  left: -3px;
  top: 8px;
  bottom: 8px;
  width: 6px;
  cursor: ew-resize;
}
.fw-resize-ne {
  top: -4px;
  right: -4px;
  width: 10px;
  height: 10px;
  cursor: nesw-resize;
}
.fw-resize-nw {
  top: -4px;
  left: -4px;
  width: 10px;
  height: 10px;
  cursor: nwse-resize;
}
.fw-resize-se {
  bottom: -4px;
  right: -4px;
  width: 10px;
  height: 10px;
  cursor: nwse-resize;
}
.fw-resize-sw {
  bottom: -4px;
  left: -4px;
  width: 10px;
  height: 10px;
  cursor: nesw-resize;
}
</style>
