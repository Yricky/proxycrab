import { createSSRApp } from "vue";
import { renderToString } from "vue/server-renderer";
import { expect, test, vi } from "vitest";
vi.mock("../stores/sessions", () => ({
  sessionsStore: {
    shares: {
      3: { session_id: 3, persistent: true },
      8: { session_id: 8, persistent: false },
    },
  },
}));
import SessionListItem from "./SessionListItem.vue";

test.each([
  [3, "已开启分享 · 重启后保留"],
  [8, "已开启分享 · 应用退出后失效"],
  [9, null],
])("Session %s shows its sharing state before its name", async (id, label) => {
  const html = await renderToString(
    createSSRApp(SessionListItem, {
      session: { id, name: "Example", created_at: 0, description: null },
    }),
  );
  if (label) {
    expect(html).toContain(`aria-label="${label}"`);
    expect(html.indexOf('role="img"')).toBeLessThan(html.indexOf('class="session-item-name"'));
  } else {
    expect(html).not.toContain('role="img"');
  }
  expect(html).toContain("Example");
});
