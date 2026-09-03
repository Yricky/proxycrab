import { reactive } from "vue";
import { runtimeBackend as backend } from "../api/runtime-backend";
import type { SkillManagerState, SkillSaveResult } from "../api/types";

export const skillStore = reactive({
  state: null as SkillManagerState | null,
  loading: false,

  get warning(): boolean {
    const state = this.state;
    return Boolean(
      state &&
        (!state.configured || state.config_error || state.paths.some((path) => path.error)),
    );
  },

  get warningTitle(): string | undefined {
    const state = this.state;
    if (!state) return undefined;
    if (!state.configured) return "Skill 未配置";
    if (state.config_error) return "Skill 路径配置读取失败";
    if (state.paths.some((path) => path.error)) return "Skill 同步失败";
    return undefined;
  },

  async refresh(): Promise<void> {
    const manager = backend.host?.skillManager;
    if (!manager) return;
    try {
      this.state = await manager.getState();
    } catch (error) {
      this.state = {
        configured: true,
        config_error: error instanceof Error ? error.message : String(error),
        paths: [],
      };
    }
  },

  async sync(): Promise<void> {
    const manager = backend.host?.skillManager;
    if (!manager || this.loading) return;
    this.loading = true;
    try {
      this.state = await manager.sync();
    } finally {
      this.loading = false;
    }
  },

  async save(paths: string[], deleteRemoved: boolean): Promise<SkillSaveResult> {
    const manager = backend.host?.skillManager;
    if (!manager) throw new Error("当前运行目标不支持管理 Skill");
    this.loading = true;
    try {
      const result = await manager.savePaths(paths, deleteRemoved);
      this.state = result.state;
      return result;
    } finally {
      this.loading = false;
    }
  },
});
