<script setup lang="ts">
import { useBackend } from "../api";
import ScriptLibraryManager, {
  type ScriptLibraryConfig,
} from "../components/ScriptLibraryManager.vue";

const backend = useBackend();

const config: ScriptLibraryConfig = {
  windowId: "column-manager",
  sidebarTitle: "列脚本",
  createPlaceholder: "新列脚本名",
  emptyListHint: "暂无列脚本",
  editorEmptyHint: "新建一个列脚本后即可开始编辑",
  statusbar: "Lua 5.4 沙箱 · 10 万指令上限 · 16 MiB 内存上限",
  defaultSource: "",
  resources: ["column_scripts"],
  changeEvent: "column-scripts-changed",
  refreshLogsView: true,
  clearDebugOnSessionChange: true,
  deleteMessage: (script, dirty) =>
    dirty
      ? `列脚本「${script.name}」包含未保存的更改。确定删除吗？该操作不可撤销。`
      : `确定删除列脚本「${script.name}」吗？该操作不可撤销。`,
  debug: {
    mode: "log-id",
    emptyLabel: "（空字符串）",
    run: async ({ name, sessionId, logId }) => {
      const payload = await backend.getLogViews({
        session_id: sessionId,
        logs: [{ id: logId }],
        view: {
          columns: [{ kind: "script", script_name: name, width: 160 }],
        },
      });
      const exception = payload.exceptions.find((item) => item.id === logId);
      if (exception) return { result: exception.message, failed: true };
      const row = payload.rows.find((item) => item.id === logId);
      if (!row) return { result: `Log ${logId} 不存在`, failed: true };
      return { result: row.cells[0] ?? "", failed: false };
    },
  },
  api: {
    list: () => backend.listColumnScripts(),
    get: (name) => backend.getColumnScript(name),
    create: (name, content) => backend.createColumnScript({ name, content }),
    update: (name, content) => backend.updateColumnScript(name, { content }),
    remove: (name) => backend.deleteColumnScript(name),
  },
};
</script>

<template>
  <ScriptLibraryManager :config="config" />
</template>
