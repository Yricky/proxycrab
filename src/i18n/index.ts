import { createI18n } from "vue-i18n";
import enUS from "./locales/en-US";

const STORAGE_KEY = "proxycrab.locale";

export type AppLocale = "zh-CN" | "en-US";

/**
 * 中文原文即 key：en-US 是唯一人工维护的翻译表；
 * zh-CN 由 en-US 的 key 恒等生成，无需单独维护。
 */
const zhCN: Record<string, string> = Object.fromEntries(Object.keys(enUS).map((key) => [key, key]));

function initialLocale(): AppLocale {
  if (typeof localStorage !== "undefined") {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved === "zh-CN" || saved === "en-US") return saved;
  }
  if (typeof navigator !== "undefined" && !navigator.language.toLowerCase().startsWith("zh")) {
    return "en-US";
  }
  return "zh-CN";
}

export const i18n = createI18n({
  legacy: false,
  locale: initialLocale(),
  fallbackLocale: "zh-CN",
  messages: { "zh-CN": zhCN, "en-US": enUS },
});

/** 模板 / 组件脚本 / stores / utils 统一从本模块导入 t。 */
export const t = i18n.global.t;

// 入口 html 声明的静态 lang（zh-CN）与实际语言同步
if (typeof document !== "undefined") document.documentElement.lang = i18n.global.locale.value;

export function setLocale(locale: AppLocale): void {
  i18n.global.locale.value = locale;
  if (typeof localStorage !== "undefined") localStorage.setItem(STORAGE_KEY, locale);
  if (typeof document !== "undefined") document.documentElement.lang = locale;
}
