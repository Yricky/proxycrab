import "./styles/base.css";
import { createApp, h } from "vue";
import AboutWindow from "./windows/AboutWindow.vue";

document.documentElement.dataset.theme = "dark";

createApp({
  render: () =>
    h("div", { class: "preview-window" }, [
      h("div", { class: "preview-titlebar" }, [h("span", "关于 ProxyCrab")]),
      h("div", { class: "preview-body" }, [h(AboutWindow)]),
    ]),
}).mount("#app");
