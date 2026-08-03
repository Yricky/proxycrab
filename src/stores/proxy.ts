import { reactive } from "vue";
import { createTauriBackend } from "../api/tauri-backend";
import type { AppConfig, ProxyStatus } from "../api/types";
import { reportError } from "./app";

// Stores are module singletons; they use their own backend handle so that
// polling logic can live outside component setup. Components still go
// through useBackend() for user-triggered actions.
const backend = createTauriBackend();

let pollTimer: number | undefined;
let portPersistTimer: number | undefined;

/** Delay before a valid port edit is written back to AppConfig. */
const PORT_PERSIST_DEBOUNCE_MS = 500;

function cancelPortPersist(): void {
  if (portPersistTimer !== undefined) {
    window.clearTimeout(portPersistTimer);
    portPersistTimer = undefined;
  }
}

export const proxyStore = reactive({
  status: { status: "stopped" } as ProxyStatus,
  busy: false,
  /** Machine IPv4 addresses (including loopback), for the display-only picker. */
  localIps: [] as string[],
  /** Address shown in the toolbar's IP segment. Purely informational — the
   *  proxy binds to the configured `proxy_host`. While running it mirrors the
   *  actual listening address; while stopped it stays at its last value
   *  (default 0.0.0.0). Not persisted. */
  displayIp: "0.0.0.0",
  /** Port shown in the toolbar (editable while stopped). */
  portText: "",
  /** Last port value known to be stored in AppConfig. */
  persistedPort: null as number | null,

  get running(): boolean {
    return this.status.status === "running";
  },

  get portNumber(): number | null {
    const text = this.portText.trim();
    if (!/^\d{1,5}$/.test(text)) return null;
    const port = Number(text);
    if (!Number.isInteger(port) || port < 1 || port > 65535) return null;
    return port;
  },

  get portValid(): boolean {
    return this.portNumber !== null;
  },

  /** 0.0.0.0 + loopback + every local IPv4, deduplicated, in stable order. */
  get ipOptions(): string[] {
    const seen = new Set<string>();
    const options: string[] = [];
    for (const ip of ["0.0.0.0", "127.0.0.1", ...this.localIps]) {
      if (!seen.has(ip)) {
        seen.add(ip);
        options.push(ip);
      }
    }
    return options;
  },

  /** Whether the start/stop button is actionable right now. */
  get canToggle(): boolean {
    if (this.busy) return false;
    if (this.status.status === "starting" || this.status.status === "stopping") {
      return false;
    }
    if (this.running) return true;
    return this.portValid;
  },

  async refresh(): Promise<void> {
    try {
      const status = await backend.getProxyStatus();
      this.status = status;
      if (status.status === "running") {
        // Show the real listening address while running; editing is disabled.
        this.displayIp = status.host;
        this.portText = String(status.port);
      }
    } catch (error) {
      reportError(error, "获取代理状态失败");
    }
    // Keep the (stopped) toolbar port in sync with AppConfig, unless the user
    // is mid-edit (portText already diverged from the persisted value).
    const syncedPort = String(this.persistedPort);
    const unmodified =
      this.persistedPort === null || this.portText === syncedPort;
    if (this.status.status !== "running" && unmodified) {
      try {
        const config = await backend.getConfig();
        if (config.proxy_port !== this.persistedPort) {
          this.persistedPort = config.proxy_port;
          this.portText = String(config.proxy_port);
        }
      } catch {
        // Non-fatal: the toolbar keeps its current value.
      }
    }
  },

  /** Seed toolbar state from AppConfig and load the local IP list. */
  async init(): Promise<void> {
    try {
      const config = await backend.getConfig();
      this.persistedPort = config.proxy_port;
      if (this.status.status !== "running") {
        this.portText = String(config.proxy_port);
      }
    } catch (error) {
      reportError(error, "获取代理配置失败");
    }
    await this.refreshLocalIps();
  },

  async refreshLocalIps(): Promise<void> {
    try {
      this.localIps = await backend.listLocalIps();
    } catch (error) {
      reportError(error, "获取本机 IP 列表失败");
    }
  },

  /** Update the toolbar port; persist to AppConfig once the edit settles. */
  setPortText(text: string): void {
    this.portText = text;
    cancelPortPersist();
    if (this.running) return;
    const port = this.portNumber;
    if (port === null) return;
    portPersistTimer = window.setTimeout(() => {
      void this.persistPort(port);
    }, PORT_PERSIST_DEBOUNCE_MS);
  },

  /** Write `port` into AppConfig unless it is already stored. Returns success. */
  async persistPort(port: number): Promise<boolean> {
    if (port === this.persistedPort) return true;
    try {
      const latest = await backend.getConfig();
      const next: AppConfig = { ...latest, proxy_port: port };
      await backend.replaceConfig(next);
      this.persistedPort = port;
      return true;
    } catch (error) {
      reportError(error, "保存代理端口失败");
      return false;
    }
  },

  async toggle(): Promise<void> {
    if (!this.canToggle) return;
    this.busy = true;
    try {
      if (this.running) {
        this.status = await backend.stopProxy();
      } else {
        const port = this.portNumber;
        if (port === null) return;
        // Write the current port through so start binds exactly what is shown.
        if (!(await this.persistPort(port))) return;
        this.status = await backend.startProxy();
      }
    } catch (error) {
      reportError(error, this.running ? "停止代理失败" : "启动代理失败");
      await this.refresh();
    } finally {
      this.busy = false;
      cancelPortPersist();
    }
  },

  startPolling(intervalMs = 2000): void {
    this.stopPolling();
    void this.init();
    pollTimer = window.setInterval(() => void this.refresh(), intervalMs);
  },

  stopPolling(): void {
    if (pollTimer !== undefined) {
      window.clearInterval(pollTimer);
      pollTimer = undefined;
    }
    cancelPortPersist();
  },
});
