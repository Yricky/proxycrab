import { createSSRApp, h } from "vue";
import { renderToString } from "vue/server-renderer";
import { expect, test } from "vitest";
import FileTree from "./FileTree.vue";

test("shared tree renders selection, directory controls and business slots without nesting buttons", async () => {
  const html = await renderToString(
    createSSRApp({
      render: () =>
        h(
          FileTree,
          {
            entries: [{ path: "folder/script", label: "script.lua", data: { size: 42 } }],
            selected: "folder/script",
          },
          {
            suffix: ({ entry }: { entry: { data: { size: number } } }) =>
              h("span", `${entry.data.size} B`),
            actions: () => h("button", "删除"),
          },
        ),
    }),
  );
  expect(html).toContain('aria-expanded="true"');
  expect(html).toContain('aria-current="true"');
  expect(html).toContain("script.lua");
  expect(html).toContain("42 B");
  expect(html).toContain("删除");
  expect(html).not.toMatch(/<button\b[^>]*>(?:(?!<\/button>)[\s\S])*<button\b/);
});

test("shared tree renders the caller's empty state", async () => {
  const html = await renderToString(
    createSSRApp({
      render: () => h(FileTree, { entries: [], emptyHint: "工作区暂无资源" }),
    }),
  );
  expect(html).toContain("工作区暂无资源");
  expect(html).not.toContain("file-tree-row");
});

test("creation capability controls root and directory actions", async () => {
  for (const canCreate of [false, true]) {
    const html = await renderToString(
      createSSRApp({
        render: () =>
          h(FileTree, {
            entries: [{ path: "folder/item", data: null }],
            canCreate,
          }),
      }),
    );
    expect(html.includes('aria-label="在根目录中新增文件"')).toBe(canCreate);
    expect(html.includes('aria-label="在 folder 中新增文件"')).toBe(canCreate);
  }
});
