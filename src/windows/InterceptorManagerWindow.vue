<script setup lang="ts">
import { computed } from "vue";
import { useBackend } from "../api";
import type { InterceptorKind } from "../api/types";
import ScriptLibraryManager, {
  type ScriptLibraryConfig,
} from "../components/ScriptLibraryManager.vue";

const props = defineProps<{ kind: InterceptorKind; initialName?: string }>();

const backend = useBackend();

const kindLabel = computed(() => (props.kind === "request" ? "请求拦截器" : "响应拦截器"));

const config: ScriptLibraryConfig = {
  windowId: `interceptor-manager-${props.kind}`,
  sidebarTitle: kindLabel.value,
  createPlaceholder: `新${kindLabel.value}名`,
  emptyListHint: `暂无${kindLabel.value}`,
  editorEmptyHint: `新建一个${kindLabel.value}后即可开始编辑`,
  statusbar: "Lua 5.4 沙箱 · 10 万指令上限 · 16 MiB 内存上限",
  defaultSource: "",
  resources: ["interceptors"],
  changeEvent: "interceptors-changed",
  focusScope: props.kind,
  validateName: (name) => {
    if (!name) return "名称不能为空";
    if (name.includes("/") || name.includes("\\")) return "名称不能包含路径分隔符（/ 或 \\）";
    return null;
  },
  deleteMessage: (entry, dirty) =>
    `确定删除${kindLabel.value}「${entry.name}」吗？${
      dirty ? "它包含未保存的更改。" : ""
    }${
      entry.usage_count
        ? `它正在被 ${entry.usage_count} 个会话使用，删除后会从这些会话的链路中移除。`
        : "当前没有会话引用它。"
    }该操作不可撤销。`,
  api: {
    list: () => backend.listInterceptors(props.kind).then((result) => result.items),
    get: (name) => backend.getInterceptor(props.kind, name),
    create: (name, content) => backend.createInterceptor({ kind: props.kind, name, content }),
    update: (name, content) => backend.updateInterceptor(props.kind, name, { content }),
    remove: (name) => backend.deleteInterceptor(props.kind, name),
  },
};
</script>

<template>
  <ScriptLibraryManager :config="config" :initial-name="initialName" />
</template>
