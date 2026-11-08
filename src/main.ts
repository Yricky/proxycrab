import { isTauri } from "@tauri-apps/api/core";
import { createApp } from "vue";
import { i18n } from "./i18n";
import "./styles/base.css";

// 屏蔽 webview 原生右键菜单；应用内上下文菜单由页面自行实现。
document.addEventListener("contextmenu", (event) => {
  event.preventDefault();
});

async function mount(): Promise<void> {
  const Landing = isTauri()
    ? (await import("./landing/TauriLanding.vue")).default
    : (await import("./landing/CliLanding.vue")).default;
  createApp(Landing).use(i18n).mount("#app");
}

void mount();
