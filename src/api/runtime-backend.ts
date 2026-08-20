import type { Backend } from "./backend";

declare global {
  interface Window {
    proxyCrabBackend?: Backend;
  }
}

export function installBackend(backend: Backend): void {
  if (window.proxyCrabBackend) {
    throw new Error("ProxyCrab Backend already initialized");
  }
  Object.defineProperty(window, "proxyCrabBackend", {
    value: backend,
    writable: false,
    configurable: false,
  });
}

export function getBackend(): Backend {
  const backend = window.proxyCrabBackend;
  if (!backend) throw new Error("ProxyCrab Backend is not initialized");
  return backend;
}

/** Lazy proxy for module-level stores that are imported before the Landing installs a Backend. */
export const runtimeBackend = new Proxy({} as Backend, {
  get(_target, property) {
    const backend = getBackend();
    const value = Reflect.get(backend, property, backend) as unknown;
    return typeof value === "function" ? value.bind(backend) : value;
  },
});
