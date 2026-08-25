import { createApp } from "vue";
import ShareLanding from "./landing/ShareLanding.vue";
import "./styles/base.css";

const app = createApp(ShareLanding);

// 屏蔽浏览器原生右键菜单；应用内上下文菜单由页面自行实现。
document.addEventListener("contextmenu", (event) => {
  event.preventDefault();
});

app.mount("#app");
