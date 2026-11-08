import { expect, test } from "vitest";
import { createSSRApp } from "vue";
import { renderToString } from "vue/server-renderer";
import type { ApiActionView } from "../api/generated/ApiActionView";
import { ACTION_META, type PermissionGroupDimension } from "../utils/permission-meta";
import PermissionEditor from "./PermissionEditor.vue";

const redactedActions = [
  "POST /api/logs/ids",
  "POST /api/logs/views",
  "GET /api/logs/{id}",
  "GET /api/logs/{id}/body",
  "GET /api/logs/{id}/websocket/messages/{message_id}/body",
  "GET /api/logs/{id}/interceptors/{execution_id}/snapshot",
  "GET /api/breakpoints",
  "GET /api/breakpoints/{id}",
  "GET /api/breakpoints/{id}/body",
  "POST /api/breakpoints/{id}/extend",
  "POST /api/breakpoints/{id}/execute",
  "GET /api/session-view",
  "POST /api/filter-scripts/{name}/debug",
  "GET /api/bypass",
];
const actionIds = [
  ...new Set([
    ...Object.keys(ACTION_META),
    ...redactedActions,
    "GET /api/logs/{id}/interceptors/{execution_id}/content",
    "GET /api/unknown",
  ]),
];
const catalog: ApiActionView[] = actionIds.map((id) => {
  const [method, route_template] = id.split(" ");
  return { id, method, route_template, default_mode: "allow" };
});

test.each<PermissionGroupDimension>(["function", "impact"])(
  "%s grouping marks only redaction-supported actions to the left of permission controls without a binding",
  async (dimension) => {
    const html = await renderToString(
      createSSRApp(PermissionEditor, {
        catalog,
        modelValue: {},
        dimension,
      }),
    );
    const rows = html.split('<div class="permission-row"').slice(1);
    expect(rows).toHaveLength(catalog.length);
    for (const action of catalog) {
      const row = rows.find(
        (row) =>
          row.includes(`>${action.route_template}</code>`) &&
          row.includes(`>${action.method}</span>`),
      );
      expect(row, action.id).toBeDefined();
      if (redactedActions.includes(action.id)) {
        expect(row, action.id).toContain('class="redaction-indicator"');
        expect(row, action.id).toContain('aria-label="支持脱敏：绑定脱敏脚本后生效"');
        expect(row, action.id).not.toContain('title="支持脱敏');
        expect(row!.indexOf('class="redaction-indicator"')).toBeLessThan(
          row!.indexOf('class="mode-group"'),
        );
      } else {
        expect(row, action.id).not.toContain('class="redaction-indicator"');
      }
    }
  },
);

test("impact grouping ignores method/default mode and keeps permissions unchanged", async () => {
  const html = await renderToString(
    createSSRApp(PermissionEditor, {
      catalog: [
        {
          id: "GET /api/sessions",
          method: "GET",
          route_template: "/api/sessions",
          default_mode: "deny",
        },
        {
          id: "POST /api/logs/ids",
          method: "GET",
          route_template: "/api/logs/ids",
          default_mode: "deny",
        },
        {
          id: "GET /api/redaction-scripts/{name}",
          method: "POST",
          route_template: "/api/redaction-scripts/{name}",
          default_mode: "allow",
        },
        {
          id: "GET /api/logs/new",
          method: "GET",
          route_template: "/api/logs/new",
          default_mode: "deny",
        },
      ],
      modelValue: {},
      dimension: "impact",
    }),
  );
  const sections = html.split('<section class="permission-group"').slice(1);
  const read = sections.find((section) => section.includes(">仅读取</strong>"));
  expect(read).toContain(">/api/sessions</code>");
  expect(read).toMatch(/<button[^>]*class="[^"]*\bm-deny\b[^"]*"[^>]*aria-pressed="true"/);
  expect(sections.find((section) => section.includes(">有写操作</strong>"))).toContain(
    ">/api/logs/ids</code>",
  );
  expect(sections.find((section) => section.includes(">危险操作</strong>"))).toContain(
    ">/api/redaction-scripts/{name}</code>",
  );
  expect(sections.find((section) => section.includes(">其他</strong>"))).toContain(
    ">/api/logs/new</code>",
  );
});

test("unconfigured actions stay in other even with a known function prefix", async () => {
  const html = await renderToString(
    createSSRApp(PermissionEditor, {
      catalog: [
        {
          id: "GET /api/logs/new",
          method: "GET",
          route_template: "/api/logs/new",
          default_mode: "allow",
        },
      ],
      modelValue: {},
      dimension: "function",
    }),
  );
  expect(html).toContain(">其他</strong>");
  expect(html).not.toContain(">抓包日志</strong>");
  expect(html).toContain(">/api/logs/new</code>");
});
