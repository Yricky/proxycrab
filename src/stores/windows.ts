import { reactive } from "vue";
import type { Component } from "vue";

export interface WindowState {
  /** Unique id; reopening the same id focuses the existing window. */
  id: string;
  title: string;
  component: Component;
  props: Record<string, unknown>;
  x: number;
  y: number;
  width: number;
  height: number;
  z: number;
}

export interface OpenWindowOptions {
  title: string;
  component: Component;
  props?: Record<string, unknown>;
  width?: number;
  height?: number;
  /** Center offset hint so consecutively opened windows cascade. */
  cascade?: boolean;
}

export type WindowCloseGuard = () => boolean | Promise<boolean>;

let zCounter = 100;
let cascadeOffset = 0;
const closeGuards = new Map<string, WindowCloseGuard>();

export const windowsStore = reactive({
  windows: [] as WindowState[],

  open(id: string, options: OpenWindowOptions): WindowState {
    const existing = this.windows.find((w) => w.id === id);
    if (existing) {
      existing.title = options.title;
      this.focus(id);
      return existing;
    }
    const base = cascadeOffset;
    cascadeOffset = (cascadeOffset + 28) % 140;
    const win: WindowState = {
      id,
      title: options.title,
      component: options.component,
      props: options.props ?? {},
      x: 120 + base,
      y: 80 + base,
      width: options.width ?? 720,
      height: options.height ?? 480,
      z: ++zCounter,
    };
    this.windows.push(win);
    return win;
  },

  async close(id: string): Promise<boolean> {
    const guard = closeGuards.get(id);
    if (guard && !(await guard())) return false;
    const index = this.windows.findIndex((w) => w.id === id);
    if (index >= 0) {
      this.windows.splice(index, 1);
      closeGuards.delete(id);
    }
    return index >= 0;
  },

  registerCloseGuard(id: string, guard: WindowCloseGuard): void {
    closeGuards.set(id, guard);
  },

  unregisterCloseGuard(id: string): void {
    closeGuards.delete(id);
  },

  focus(id: string): void {
    const win = this.windows.find((w) => w.id === id);
    if (win) win.z = ++zCounter;
  },

  updateGeometry(id: string, x: number, y: number, width: number, height: number): void {
    const win = this.windows.find((w) => w.id === id);
    if (win) {
      win.x = x;
      win.y = y;
      win.width = width;
      win.height = height;
    }
  },
});
