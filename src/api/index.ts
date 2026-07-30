import type { App, InjectionKey } from "vue";
import { inject } from "vue";
import type { Backend } from "./backend";
import { createTauriBackend } from "./tauri-backend";

const backendKey: InjectionKey<Backend> = Symbol("proxycrab-backend");

/** Install a Backend implementation app-wide. Defaults to the Tauri one. */
export function provideBackend(app: App, backend: Backend = createTauriBackend()): void {
  app.provide(backendKey, backend);
}

/** Access the current Backend implementation from any component. */
export function useBackend(): Backend {
  const backend = inject(backendKey);
  if (!backend) {
    throw new Error("Backend not provided; call provideBackend(app) in main.ts");
  }
  return backend;
}
