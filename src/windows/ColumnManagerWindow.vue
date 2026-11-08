<script setup lang="ts">
import { useBackend } from "../api";
import { scriptTitle } from "../utils/script-label";
import { t } from "../i18n";
import ScriptLibraryManager, {
  type ScriptLibraryConfig,
} from "../components/ScriptLibraryManager.vue";

const props = defineProps<{ initialName?: string; sessionId?: number | null }>();
const backend = useBackend();

const config: ScriptLibraryConfig = {
  windowId: "column-manager",
  sidebarTitle: t("列脚本"),
  createPlaceholder: "新列脚本名",
  emptyListHint: t("暂无列脚本"),
  editorEmptyHint: t("新建一个列脚本后即可开始编辑"),
  statusbar: t("Lua 5.4 沙箱 · 10 万指令上限 · 16 MiB 内存上限"),
  defaultSource: "",
  resources: ["column_scripts"],
  changeEvent: "column-scripts-changed",
  focusScope: "column",
  deleteMessage: (script, dirty) =>
    dirty
      ? t("列脚本「{name}」包含未保存的更改。确定删除吗？该操作不可撤销。", {
          name: scriptTitle(script),
        })
      : t("确定删除列脚本「{name}」吗？该操作不可撤销。", { name: scriptTitle(script) }),
  debug: {
    mode: "log-id",
    emptyLabel: t("（空字符串）"),
    run: async ({ name, logId }) => {
      const payload = await backend.getLogViews({
        session_id: props.sessionId ?? null,
        logs: [{ id: logId }],
        view: {
          columns: [{ kind: "script", script_name: name, width: 160 }],
        },
      });
      const exception = payload.exceptions.find((item) => item.id === logId);
      if (exception) return { result: exception.message, failed: true };
      const row = payload.rows.find((item) => item.id === logId);
      if (!row) return { result: t("Log {id} 不存在", { id: logId }), failed: true };
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
  <ScriptLibraryManager
    :config="config"
    :initial-name="props.initialName"
    :session-id="props.sessionId ?? null"
  />
</template>
