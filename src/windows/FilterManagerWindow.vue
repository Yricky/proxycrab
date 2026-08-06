<script setup lang="ts">
import { useBackend } from "../api";
import ScriptLibraryManager, {
  type ScriptLibraryConfig,
} from "../components/ScriptLibraryManager.vue";

const DEFAULT_SOURCE = "local input = ...\nreturn false\n";
const backend = useBackend();

const config: ScriptLibraryConfig = {
  windowId: "filter-manager",
  sidebarTitle: "过滤脚本",
  createPlaceholder: "新过滤脚本名",
  emptyListHint: "暂无过滤脚本",
  editorEmptyHint: "新建一个过滤脚本后即可开始编辑",
  statusbar: "entry 为当前记录 · 字符串参数通过 ... 传入 · 必须返回布尔值",
  defaultSource: DEFAULT_SOURCE,
  resources: ["filter_scripts"],
  changeEvent: "filter-scripts-changed",
  deleteMessage: (script) =>
    `确定删除过滤脚本「${script.name}」吗？引用它的会话会清除过滤。该操作不可撤销。`,
  debug: {
    mode: "log-id-input",
    run: async ({ name, sessionId, logId, input }) => {
      const result = await backend.debugFilterScript(name, {
        session_id: sessionId,
        log_id: logId,
        input,
      });
      return { result: String(result), failed: false };
    },
  },
  api: {
    list: () => backend.listFilterScripts(),
    get: (name) => backend.getFilterScript(name),
    create: (name, content) => backend.createFilterScript({ name, content }),
    update: (name, content) => backend.updateFilterScript(name, { content }),
    remove: (name) => backend.deleteFilterScript(name),
  },
};
</script>

<template>
  <ScriptLibraryManager :config="config" />
</template>
