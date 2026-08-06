<script setup lang="ts">
import { useBackend } from "../api";
import ScriptLibraryManager, {
  type ScriptLibraryConfig,
} from "../components/ScriptLibraryManager.vue";
import { routingStore } from "../stores/routing";

const DEFAULT_SOURCE = "return true\n";
const backend = useBackend();

const config: ScriptLibraryConfig = {
  windowId: "routing-manager",
  sidebarTitle: "分流规则",
  createPlaceholder: "新规则名称",
  emptyListHint: "暂无分流规则",
  editorEmptyHint: "选择或创建一个分流规则",
  defaultSource: DEFAULT_SOURCE,
  resources: ["routing_scripts", "routing_selection"],
  deleteMessage: (script) => `确定删除「${script.name}」吗？名称创建后不可恢复。`,
  selection: {
    get: () => backend.getRoutingSelection().then((selection) => selection.name),
    set: (name) => backend.replaceRoutingSelection({ name }).then((selection) => selection.name),
    onChanged: () => routingStore.refresh(),
  },
  api: {
    list: () => backend.listRoutingScripts(),
    get: (name) => backend.getRoutingScript(name),
    create: (name, content) => backend.createRoutingScript({ name, content }),
    update: (name, content) => backend.updateRoutingScript(name, { content }),
    remove: (name) => backend.deleteRoutingScript(name),
  },
};
</script>

<template>
  <ScriptLibraryManager :config="config" />
</template>
