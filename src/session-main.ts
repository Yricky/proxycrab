import { createApp } from "vue";
import ShareLanding from "./landing/ShareLanding.vue";
import { i18n } from "./i18n";
import "./styles/base.css";

const app = createApp(ShareLanding);
app.use(i18n);

// 屏蔽浏览器原生右键菜单；应用内上下文菜单由页面自行实现。
document.addEventListener("contextmenu", (event) => {
  event.preventDefault();
});

app.mount("#app");
