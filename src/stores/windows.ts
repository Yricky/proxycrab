import { markRaw, reactive } from "vue";
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
  maximized: boolean;
  /** Geometry to restore when un-maximizing. */
  restore?: { x: number; y: number; width: number; height: number };
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
      component: markRaw(options.component),
      props: options.props ?? {},
      x: 360 + base,
      y: 80 + base,
      width: options.width ?? 720,
      height: options.height ?? 480,
      z: ++zCounter,
      maximized: false,
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

  toggleMaximize(id: string): void {
    const win = this.windows.find((w) => w.id === id);
    if (!win) return;
    if (!win.maximized) {
      win.restore = { x: win.x, y: win.y, width: win.width, height: win.height };
      win.maximized = true;
      win.x = 0;
      win.y = 0;
      win.width = window.innerWidth;
      win.height = window.innerHeight;
    } else {
      const restored = win.restore ?? { x: 120, y: 80, width: win.width, height: win.height };
      win.maximized = false;
      win.restore = undefined;
      win.x = restored.x;
      win.y = restored.y;
      win.width = restored.width;
      win.height = restored.height;
    }
    win.z = ++zCounter;
  },
});
