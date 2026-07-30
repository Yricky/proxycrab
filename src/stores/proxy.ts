import { reactive } from "vue";
import { createTauriBackend } from "../api/tauri-backend";
import type { ProxyStatus } from "../api/types";
import { reportError } from "./app";

// Stores are module singletons; they use their own backend handle so that
// polling logic can live outside component setup. Components still go
// through useBackend() for user-triggered actions.
const backend = createTauriBackend();

let pollTimer: number | undefined;

export const proxyStore = reactive({
  status: { status: "stopped" } as ProxyStatus,
  busy: false,

  get running(): boolean {
    return this.status.status === "running";
  },

  get label(): string {
    switch (this.status.status) {
      case "running":
        return `${this.status.host}:${this.status.port}`;
      case "starting":
        return "启动中…";
      case "stopping":
        return "停止中…";
      case "failed":
        return "启动失败";
      default:
        return "未运行";
    }
  },

  async refresh(): Promise<void> {
    try {
      this.status = await backend.getProxyStatus();
    } catch (error) {
      reportError(error, "获取代理状态失败");
    }
  },

  async toggle(): Promise<void> {
    if (this.busy) return;
    this.busy = true;
    try {
      this.status = this.running
        ? await backend.stopProxy()
        : await backend.startProxy();
    } catch (error) {
      reportError(error, this.running ? "停止代理失败" : "启动代理失败");
      await this.refresh();
    } finally {
      this.busy = false;
    }
  },

  startPolling(intervalMs = 2000): void {
    this.stopPolling();
    void this.refresh();
    pollTimer = window.setInterval(() => void this.refresh(), intervalMs);
  },

  stopPolling(): void {
    if (pollTimer !== undefined) {
      window.clearInterval(pollTimer);
      pollTimer = undefined;
    }
  },
});
