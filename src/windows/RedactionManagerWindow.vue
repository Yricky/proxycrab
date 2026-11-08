<script setup lang="ts">
import { useBackend } from "../api";
import { t } from "../i18n";
import ScriptLibraryManager, {
  type ScriptLibraryConfig,
} from "../components/ScriptLibraryManager.vue";
const backend = useBackend();
const config: ScriptLibraryConfig = {
  windowId: "redaction-manager",
  sidebarTitle: t("脱敏脚本"),
  createPlaceholder: "新脱敏脚本名",
  emptyListHint: t("暂无脱敏脚本"),
  editorEmptyHint: t("新建脚本后，在访问身份或分享设置中绑定"),
  statusbar: t("context：unloaded / loaded / redacted · 读取原值，仅 loaded 部分的修改生效"),
  defaultSource: `-- #display_name: 隐藏凭据
if context.req.header == "loaded" and req.headers then
  req.headers:remove("authorization")
  req.headers:remove("cookie")
end
if resp and context.resp.header == "loaded" then
  resp.headers:remove("set-cookie")
end
`,
  resources: ["all"],
  changeEvent: "redaction-scripts-changed",
  deleteMessage: (script) =>
    t("删除脱敏脚本「{name}」？已有绑定会保留，相关读取将被拒绝，直到重新绑定或恢复脚本。", {
      name: script.name,
    }),
  api: {
    list: () => backend.listRedactionScripts!(),
    get: (name) => backend.getRedactionScript!(name),
    create: (name, content) => backend.createRedactionScript!({ name, content }),
    update: (name, content) => backend.updateRedactionScript!(name, { content }),
    remove: (name) => backend.deleteRedactionScript!(name),
  },
};
</script>
<template><ScriptLibraryManager :config="config" /></template>
