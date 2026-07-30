import { createApp } from "vue";
import App from "./App.vue";
import { provideBackend } from "./api";
import "./styles/base.css";

const app = createApp(App);
provideBackend(app);

// 屏蔽 webview 原生右键菜单；应用内上下文菜单由页面自行实现。
document.addEventListener("contextmenu", (event) => {
  event.preventDefault();
});

app.mount("#app");
