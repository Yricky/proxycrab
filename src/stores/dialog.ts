import { reactive } from "vue";

export interface ConfirmOptions {
  title: string;
  message: string;
  confirmText?: string;
  danger?: boolean;
}

export interface MenuItem {
  label: string;
  icon?: unknown;
  danger?: boolean;
  disabled?: boolean;
  dividerBefore?: boolean;
  action: () => void;
}

interface ConfirmState extends ConfirmOptions {
  visible: boolean;
  resolve: ((ok: boolean) => void) | null;
}

export const confirmState = reactive<ConfirmState>({
  visible: false,
  title: "",
  message: "",
  confirmText: "确定",
  danger: false,
  resolve: null,
});

export function confirmDialog(options: ConfirmOptions): Promise<boolean> {
  return new Promise((resolve) => {
    confirmState.visible = true;
    confirmState.title = options.title;
    confirmState.message = options.message;
    confirmState.confirmText = options.confirmText ?? "确定";
    confirmState.danger = options.danger ?? false;
    confirmState.resolve = resolve;
  });
}

export function settleConfirm(ok: boolean): void {
  confirmState.visible = false;
  confirmState.resolve?.(ok);
  confirmState.resolve = null;
}

interface ContextMenuState {
  visible: boolean;
  x: number;
  y: number;
  items: MenuItem[];
}

export const contextMenuState = reactive<ContextMenuState>({
  visible: false,
  x: 0,
  y: 0,
  items: [],
});

function showContextMenu(x: number, y: number, items: MenuItem[]): void {
  const width = 180;
  const height = Math.min(items.length * 32 + 12, window.innerHeight - 16);
  contextMenuState.x = Math.max(8, Math.min(x, window.innerWidth - width - 8));
  contextMenuState.y = Math.max(8, Math.min(y, window.innerHeight - height - 8));
  contextMenuState.items = items;
  contextMenuState.visible = true;
}

export function openContextMenu(event: MouseEvent, items: MenuItem[]): void {
  event.preventDefault();
  event.stopPropagation();
  showContextMenu(event.clientX, event.clientY, items);
}

export function openDropdownMenu(anchor: HTMLElement, items: MenuItem[]): void {
  const rect = anchor.getBoundingClientRect();
  showContextMenu(rect.left, rect.bottom + 4, items);
}

export function closeContextMenu(): void {
  contextMenuState.visible = false;
}
