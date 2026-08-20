import { createApp } from "vue";
import Root from "./Root.vue";
import "./styles/base.css";

const app = createApp(Root);

// 屏蔽 webview 原生右键菜单；应用内上下文菜单由页面自行实现。
document.addEventListener("contextmenu", (event) => {
  event.preventDefault();
});

app.mount("#app");
