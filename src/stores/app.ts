import { reactive, watch } from "vue";

export type ThemeMode = "light" | "dark" | "system";

export interface ToastItem {
  id: number;
  kind: "info" | "success" | "error";
  message: string;
}

const STORAGE_KEY = "proxycrab.theme";

function initialTheme(): ThemeMode {
  const saved = localStorage.getItem(STORAGE_KEY);
  return saved === "light" || saved === "dark" || saved === "system" ? saved : "system";
}

const systemDark = window.matchMedia("(prefers-color-scheme: dark)");

function applyTheme(mode: ThemeMode): void {
  const dark = mode === "dark" || (mode === "system" && systemDark.matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
}

export const appStore = reactive({
  theme: initialTheme() as ThemeMode,
  toasts: [] as ToastItem[],

  setTheme(mode: ThemeMode) {
    this.theme = mode;
    localStorage.setItem(STORAGE_KEY, mode);
  },

  toast(message: string, kind: ToastItem["kind"] = "info", duration = 3000) {
    const id = ++toastSeq;
    this.toasts.push({ id, kind, message });
    window.setTimeout(() => this.dismissToast(id), duration);
  },

  dismissToast(id: number) {
    const index = this.toasts.findIndex((t) => t.id === id);
    if (index >= 0) this.toasts.splice(index, 1);
  },
});

let toastSeq = 0;

watch(
  () => appStore.theme,
  (mode) => applyTheme(mode),
  { immediate: true },
);
systemDark.addEventListener("change", () => applyTheme(appStore.theme));

/** Report an error to the user; returns the display message. */
export function reportError(error: unknown, prefix = ""): string {
  const message =
    error instanceof Error ? error.message : typeof error === "string" ? error : String(error);
  appStore.toast(prefix ? `${prefix}: ${message}` : message, "error", 5000);
  return message;
}
