<script setup lang="ts">
import { computed, ref } from "vue";
import { Io5CheckmarkCircle, Io5Download, Io5FolderOpenOutline } from "vue-icons-plus/io5";
import { useBackend } from "../api";
import { appStore, reportError } from "../stores/app";
import { skillStore } from "../stores/skill";
import { confirmDialog } from "../stores/dialog";

const backend = useBackend();
const installer = backend.host?.skillInstaller;

const parentPath = ref("~/.agents/skills");
const installing = ref(false);
const installedPath = ref("");

const targetPreview = computed(() => {
  const parent = parentPath.value.trim().replace(/\/+$/, "");
  return parent ? `${parent}/proxycrab` : "—";
});

async function install(): Promise<void> {
  const parent = parentPath.value.trim();
  if (!parent || installing.value) return;
  if (!installer) {
    reportError("当前运行目标不支持安装 Skill");
    return;
  }
  installing.value = true;
  try {
    const info = await installer.getInfo(parent);
    if (info.exists) {
      const confirmed = await confirmDialog({
        title: "覆盖 ProxyCrab Skill",
        message: `目标目录 ${info.target_path} 已存在。继续后将完整替换其中的所有文件，确定覆盖吗？`,
        confirmText: "覆盖安装",
        danger: true,
      });
      if (!confirmed) return;
    }
    const result = await installer.install(parent, info.exists);
    installedPath.value = result.target_path;
    void skillStore.refresh();
    appStore.toast("ProxyCrab Skill 已安装", "success");
  } catch (error) {
    reportError(error, "安装 ProxyCrab Skill 失败");
  } finally {
    installing.value = false;
  }
}
</script>

<template>
  <div class="si-root">
    <div class="si-intro">
      <Io5Download :size="22" class="si-icon" />
      <div>
        <div class="si-heading">安装 Agent Skill</div>
        <div class="si-description text-secondary">
          将应用内置的 ProxyCrab API 文档、Node 工具和调试最佳实践复制到 Agent skill 目录。
          安装过程完全离线。
        </div>
      </div>
    </div>

    <label class="si-field">
      <span class="si-label">安装父目录</span>
      <div class="si-input-row">
        <Io5FolderOpenOutline :size="15" class="text-faint" />
        <input
          v-model="parentPath"
          class="input mono si-input"
          placeholder="~/.agents/skills"
          spellcheck="false"
          @keyup.enter="install"
        />
      </div>
    </label>

    <div class="si-target">
      <span class="text-secondary">最终安装位置</span>
      <span class="mono" :title="targetPreview">{{ targetPreview }}</span>
    </div>

    <div v-if="installedPath" class="si-success">
      <Io5CheckmarkCircle :size="16" />
      <div>
        <div>安装完成</div>
        <div class="mono si-success-path">{{ installedPath }}</div>
      </div>
    </div>

    <div class="si-actions">
      <span class="text-faint">目标已存在时会在确认后完整覆盖。</span>
      <button
        class="btn primary"
        :disabled="!parentPath.trim() || installing"
        @click="install"
      >
        <Io5Download :size="14" />
        {{ installing ? "安装中…" : "安装" }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.si-root {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding: var(--space-4);
}
.si-intro {
  display: flex;
  align-items: flex-start;
  gap: var(--space-3);
}
.si-icon {
  color: var(--accent);
  flex: none;
}
.si-heading {
  font-size: 14px;
  font-weight: 600;
  margin-bottom: var(--space-1);
}
.si-description {
  font-size: 12px;
  line-height: 1.55;
}
.si-field {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}
.si-label {
  color: var(--text-secondary);
  font-size: 12px;
}
.si-input-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}
.si-input {
  flex: 1;
  min-width: 0;
}
.si-target {
  display: grid;
  grid-template-columns: 112px minmax(0, 1fr);
  gap: var(--space-2);
  padding: var(--space-2) 0;
  border-top: 1px solid var(--border);
  border-bottom: 1px solid var(--border);
  font-size: 12px;
}
.si-target .mono {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.si-success {
  display: flex;
  align-items: flex-start;
  gap: var(--space-2);
  color: var(--success);
  font-size: 12px;
}
.si-success-path {
  margin-top: 2px;
  color: var(--text-secondary);
  word-break: break-all;
  user-select: text;
}
.si-actions {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  margin-top: auto;
  font-size: 11px;
}
</style>
