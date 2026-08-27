import { reactive } from "vue";
import { runtimeBackend as backend } from "../api/runtime-backend";
import type { SkillInstallStatus } from "../api/types";

/**
 * 默认位置（~/.agents/skills/proxycrab）的 skill 安装状态。
 * 启动时检查一次；安装成功后由安装窗口触发 refresh 重新检查。
 * 初始为 installed，避免检查完成前工具栏闪现警示色。
 */
export const skillStore = reactive({
  status: "installed" as SkillInstallStatus,

  async refresh(): Promise<void> {
    const installer = backend.host?.skillInstaller;
    if (!installer) return;
    try {
      this.status = await installer.checkStatus();
    } catch {
      this.status = "not_installed";
    }
  },
});
