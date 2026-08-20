import { reactive } from "vue";
import { runtimeBackend as backend } from "../api/runtime-backend";
import type { Script } from "../api/types";
import { reportError } from "./app";

export const routingStore = reactive({
  scripts: [] as Script[],
  selectedName: null as string | null,
  loading: false,

  async refresh(): Promise<void> {
    this.loading = true;
    try {
      const [scripts, selection] = await Promise.all([
        backend.listRoutingScripts(),
        backend.getRoutingSelection(),
      ]);
      this.scripts = scripts;
      this.selectedName = selection.name;
    } catch (error) {
      reportError(error, "获取分流规则失败");
    } finally {
      this.loading = false;
    }
  },

  async select(name: string | null): Promise<boolean> {
    try {
      const selection = await backend.replaceRoutingSelection({ name });
      this.selectedName = selection.name;
      return true;
    } catch (error) {
      reportError(error, "切换分流规则失败");
      return false;
    }
  },
});
