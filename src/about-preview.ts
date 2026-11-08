import "./styles/base.css";
import { createApp, h } from "vue";
import AboutWindow from "./windows/AboutWindow.vue";
import { t } from "./i18n";
import { i18n } from "./i18n";

document.documentElement.dataset.theme = "dark";

createApp({
  render: () =>
    h("div", { class: "preview-window" }, [
      h("div", { class: "preview-titlebar" }, [h("span", t("关于 ProxyCrab"))]),
      h("div", { class: "preview-body" }, [h(AboutWindow)]),
    ]),
})
  .use(i18n)
  .mount("#app");
