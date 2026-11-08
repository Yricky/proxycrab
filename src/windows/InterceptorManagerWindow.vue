<script setup lang="ts">
import { computed } from "vue";
import { useBackend } from "../api";
import { scriptTitle } from "../utils/script-label";
import type { InterceptorKind } from "../api/types";
import { t } from "../i18n";
import ScriptLibraryManager, {
  type ScriptLibraryConfig,
} from "../components/ScriptLibraryManager.vue";

const props = defineProps<{ kind: InterceptorKind; initialName?: string }>();

const backend = useBackend();

const kindLabel = computed(() => (props.kind === "request" ? t("请求拦截器") : t("响应拦截器")));

const config: ScriptLibraryConfig = {
  windowId: `interceptor-manager-${props.kind}`,
  sidebarTitle: kindLabel.value,
  createPlaceholder: t("新{title}名", { title: kindLabel.value }),
  emptyListHint: t("暂无{title}", { title: kindLabel.value }),
  editorEmptyHint: t("新建一个{title}后即可开始编辑", { title: kindLabel.value }),
  statusbar: t("Lua 5.4 沙箱 · 10 万指令上限 · 16 MiB 内存上限"),
  defaultSource: "",
  resources: ["interceptors"],
  changeEvent: "interceptors-changed",
  focusScope: props.kind,
  deleteMessage: (entry, dirty) =>
    t("确定删除{title}「{name}」吗？{dirty}{usage}该操作不可撤销。", {
      title: kindLabel.value,
      name: scriptTitle(entry),
      dirty: dirty ? t("它包含未保存的更改。") : "",
      usage: entry.usage_count
        ? t("它正在被 {n} 个会话使用，删除后会从这些会话的链路中移除。", {
            n: entry.usage_count,
          })
        : t("当前没有会话引用它。"),
    }),
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
