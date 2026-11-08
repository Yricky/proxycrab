import { beforeAll, describe, expect, it } from "vitest";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { i18n, setLocale, t } from "./index";
import enUS from "./locales/en-US";

// node 环境注入最小 localStorage 桩（vitest 环境为 node，无 localStorage/navigator）
beforeAll(() => {
  const store = new Map<string, string>();
  Object.defineProperty(globalThis, "localStorage", {
    value: {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
      removeItem: (k: string) => void store.delete(k),
    },
    configurable: true,
  });
});

describe("i18n", () => {
  it("zh-CN 恒等：key 即中文原文", () => {
    setLocale("zh-CN");
    expect(t("确认")).toBe("确认");
  });

  it("en-US 查找与回退", () => {
    setLocale("en-US");
    expect(t("确认")).toBe("Confirm");
    expect(t("不存在的文案")).toBe("不存在的文案"); // 缺失回退中文
  });

  it("插值", () => {
    setLocale("zh-CN");
    expect(t("已清空 {n} 条记录", { n: 3 })).toBe("已清空 3 条记录");
    setLocale("en-US");
    expect(t("已清空 {n} 条记录", { n: 3 })).toContain("3");
  });

  it("复数（en）", () => {
    setLocale("en-US");
    expect(t("{n} 分钟前", 1)).toBe("1 minute ago");
    expect(t("{n} 分钟前", 5)).toBe("5 minutes ago");
  });

  it("持久化到 localStorage", () => {
    setLocale("en-US");
    expect(localStorage.getItem("proxycrab.locale")).toBe("en-US");
  });

  it("i18n 实例 locale 与 setLocale 同步", () => {
    setLocale("zh-CN");
    expect(i18n.global.locale.value).toBe("zh-CN");
  });

  it("# 前缀 key 正常插值（har-export 文案）", () => {
    setLocale("zh-CN");
    expect(t("#{id} 已写入 {method} {status}", { id: 5, method: "GET", status: 200 })).toBe(
      "#5 已写入 GET 200",
    );
  });
});

describe("静态 key 覆盖检查", () => {
  it("所有 t(\"...\") 字面量 key 均存在于 en-US 翻译表", () => {
    const keys = new Set(Object.keys(enUS));
    const used = new Map<string, string>();
    const collect = (dir: string): void => {
      for (const name of readdirSync(dir)) {
        const path = join(dir, name);
        if (statSync(path).isDirectory()) {
          if (path.includes(join("src", "i18n", "locales"))) continue;
          collect(path);
        } else if (/\.(ts|mts|vue)$/.test(name) && !/\.test\.mts$/.test(name)) {
          const content = readFileSync(path, "utf8");
          for (const match of content.matchAll(/\bt\(\s*["']([^"'\n]+)["']/g)) {
            if (!used.has(match[1])) used.set(match[1], path);
          }
        }
      }
    };
    collect(join(import.meta.dirname, ".."));
    const missing = [...used]
      .filter(([key]) => !keys.has(key))
      .map(([key, path]) => `${JSON.stringify(key)} <- ${path}`);
    expect(missing, `\n${missing.join("\n")}\n`).toEqual([]);
  });
});
