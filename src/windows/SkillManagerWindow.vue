<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import {
  Io5Add,
  Io5AlertCircleOutline,
  Io5FolderOpenOutline,
  Io5Refresh,
  Io5SaveOutline,
  Io5TrashOutline,
} from "vue-icons-plus/io5";
import { appStore, reportError } from "../stores/app";
import { choiceDialog } from "../stores/dialog";
import { skillStore } from "../stores/skill";

const draftPaths = ref<string[]>([]);
const newPath = ref("");

const savedPaths = computed(() =>
  (skillStore.state?.paths ?? []).map((path) => path.parent_path),
);
const dirty = computed(
  () =>
    draftPaths.value.length !== savedPaths.value.length ||
    draftPaths.value.some((path, index) => path !== savedPaths.value[index]),
);
const needsSave = computed(
  () => dirty.value || !skillStore.state?.configured || Boolean(skillStore.state?.config_error),
);
const removedPaths = computed(() =>
  savedPaths.value.filter((path) => !draftPaths.value.includes(path)),
);

function resetDraft(): void {
  draftPaths.value = [...savedPaths.value];
}

function addPath(): void {
  const path = newPath.value.trim();
  if (!path || draftPaths.value.includes(path)) return;
  draftPaths.value.push(path);
  newPath.value = "";
}

function removePath(index: number): void {
  draftPaths.value.splice(index, 1);
}

function pathError(parentPath: string): string | undefined {
  return skillStore.state?.paths.find((path) => path.parent_path === parentPath)?.error;
}

function isUnsynced(parentPath: string): boolean {
  return !savedPaths.value.includes(parentPath);
}

async function save(): Promise<void> {
  if (!needsSave.value || skillStore.loading) return;
  let deleteRemoved = false;
  if (removedPaths.value.length > 0) {
    const choice = await choiceDialog({
      title: "保存 Skill 路径",
      message: `已移除 ${removedPaths.value.length} 个路径。是否同时删除这些路径中的 ProxyCrab Skill？`,
      secondaryText: "仅停止管理",
      confirmText: "停止管理并删除",
      danger: true,
    });
    if (choice === "cancel") return;
    deleteRemoved = choice === "confirm";
  }
  try {
    const result = await skillStore.save(draftPaths.value, deleteRemoved);
    resetDraft();
    if (result.removal_errors.length > 0) {
      const details = result.removal_errors
        .map((error) => `${error.parent_path}: ${error.message}`)
        .join("；");
      appStore.toast(`路径已保存，但部分 Skill 删除失败：${details}`, "error", 6000);
    } else if (result.state.paths.some((path) => path.error)) {
      appStore.toast("路径已保存，但部分 Skill 同步失败", "error", 5000);
    } else {
      appStore.toast("Skill 路径已保存并同步", "success");
    }
  } catch (error) {
    reportError(error, "保存 Skill 路径失败");
  }
}

async function syncAll(): Promise<void> {
  if (needsSave.value || skillStore.loading) return;
  try {
    await skillStore.sync();
    if (skillStore.state?.paths.some((path) => path.error)) {
      appStore.toast("部分 Skill 同步失败", "error", 5000);
    } else {
      appStore.toast("所有 Skill 已同步", "success");
    }
  } catch (error) {
    reportError(error, "同步 Skill 失败");
  }
}

onMounted(async () => {
  try {
    await skillStore.refresh();
    resetDraft();
  } catch (error) {
    reportError(error, "读取 Skill 路径失败");
  }
});
</script>

<template>
  <div class="sm-root">
    <div class="sm-notice">
      <Io5AlertCircleOutline :size="18" />
      <span>
        内置 ProxyCrab Skill 将会安装到下列目录中的
        <span class="mono">proxycrab</span> 子目录。
      </span>
    </div>

    <div v-if="skillStore.state?.config_error" class="sm-config-error">
      路径配置读取失败：{{ skillStore.state.config_error }}。保存后将重建配置文件。
    </div>

    <div class="sm-add-row">
      <Io5FolderOpenOutline :size="15" class="text-faint" />
      <input
        v-model="newPath"
        class="input mono sm-input"
        placeholder="添加父目录，例如 ~/.agents/skills"
        spellcheck="false"
        @keyup.enter="addPath"
      />
      <button class="btn" :disabled="!newPath.trim()" @click="addPath">
        <Io5Add :size="15" />
        添加
      </button>
    </div>

    <div class="sm-list">
      <div v-if="draftPaths.length === 0" class="sm-empty text-faint">暂无受管理路径</div>
      <div v-for="(path, index) in draftPaths" :key="path" class="sm-item">
        <div class="sm-path-wrap">
          <div class="mono sm-path" :title="path">{{ path }}</div>
          <div v-if="pathError(path)" class="sm-status error" :title="pathError(path)">
            同步失败：{{ pathError(path) }}
          </div>
          <div v-else-if="isUnsynced(path)" class="sm-status">未同步</div>
        </div>
        <button class="btn sm-remove danger" title="移除路径" @click="removePath(index)">
          <Io5TrashOutline :size="15" />
        </button>
      </div>
    </div>

    <div class="sm-actions">
      <span v-if="needsSave" class="text-secondary">路径配置尚未保存</span>
      <span v-else class="text-faint">在点击本行的按钮前，应用不会写入任何更改</span>
      <div class="sm-action-buttons">
        <button class="btn" :disabled="needsSave || skillStore.loading" @click="syncAll">
          <Io5Refresh :size="14" />
          {{ skillStore.loading && !dirty ? "同步中…" : "同步全部" }}
        </button>
        <button class="btn primary" :disabled="!needsSave || skillStore.loading" @click="save">
          <Io5SaveOutline :size="14" />
          {{ skillStore.loading ? "保存中…" : "保存" }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.sm-root {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding: var(--space-4);
}
.sm-notice,
.sm-config-error {
  display: flex;
  align-items: flex-start;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border: 1px solid color-mix(in srgb, var(--warning) 35%, var(--border));
  border-radius: var(--radius-md);
  background: color-mix(in srgb, var(--warning) 9%, transparent);
  color: var(--text-secondary);
  font-size: 12px;
  line-height: 1.5;
}
.sm-notice > svg {
  flex: none;
  color: var(--warning);
}
.sm-notice > span {
  min-width: 0;
}
.sm-config-error {
  border-color: color-mix(in srgb, var(--danger) 35%, var(--border));
  color: var(--danger);
}
.sm-add-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}
.sm-input {
  flex: 1;
  min-width: 0;
}
.sm-list {
  flex: 1;
  min-height: 0;
  overflow: auto;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
}
.sm-empty {
  display: grid;
  place-items: center;
  min-height: 100%;
  font-size: 12px;
}
.sm-item {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  min-height: 48px;
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid var(--border);
}
.sm-item:last-child {
  border-bottom: 0;
}
.sm-path-wrap {
  min-width: 0;
  flex: 1;
}
.sm-path {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
}
.sm-status {
  margin-top: 2px;
  color: var(--warning);
  font-size: 11px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.sm-status.error {
  color: var(--danger);
}
.sm-remove {
  flex: none;
  padding: 5px;
}
.sm-actions {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  flex: none;
  font-size: 11px;
}
.sm-action-buttons {
  display: flex;
  gap: var(--space-2);
}
</style>
