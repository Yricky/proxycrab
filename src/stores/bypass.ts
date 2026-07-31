import { reactive } from "vue";
import { createTauriBackend } from "../api/tauri-backend";
import type { BypassEntry } from "../api/types";
import { reportError } from "./app";

const backend = createTauriBackend();
const PAGE_SIZE = 200;

export const bypassStore = reactive({
  rows: [] as BypassEntry[],
  selectedIds: new Set<number>(),
  hasMore: false,
  loading: false,

  async refresh(): Promise<void> {
    this.loading = true;
    try {
      const page = await backend.getBypassEntries({ limit: PAGE_SIZE });
      this.rows = page.rows;
      this.hasMore = page.has_more;
      this.selectedIds = new Set(
        [...this.selectedIds].filter((id) => this.rows.some((row) => row.id === id)),
      );
    } catch (error) {
      reportError(error, "获取透明转发记录失败");
    } finally {
      this.loading = false;
    }
  },

  async refreshNewest(): Promise<void> {
    if (this.loading) return;
    this.loading = true;
    try {
      const targetCount = Math.max(PAGE_SIZE, this.rows.length);
      const rows: BypassEntry[] = [];
      let beforeId: number | undefined;
      let hasMore = true;
      while (rows.length < targetCount && hasMore) {
        const page = await backend.getBypassEntries({
          limit: Math.min(PAGE_SIZE, targetCount - rows.length),
          ...(beforeId === undefined ? {} : { before_id: beforeId }),
        });
        rows.push(...page.rows);
        hasMore = page.has_more;
        beforeId = page.rows[page.rows.length - 1]?.id;
        if (page.rows.length === 0) break;
      }
      this.rows = rows;
      this.hasMore = hasMore;
      this.selectedIds = new Set(
        [...this.selectedIds].filter((id) => this.rows.some((row) => row.id === id)),
      );
    } catch (error) {
      reportError(error, "刷新透明转发记录失败");
    } finally {
      this.loading = false;
    }
  },

  async loadMore(): Promise<void> {
    if (this.loading || !this.hasMore || this.rows.length === 0) return;
    this.loading = true;
    try {
      const page = await backend.getBypassEntries({
        limit: PAGE_SIZE,
        before_id: this.rows[this.rows.length - 1].id,
      });
      this.rows.push(...page.rows);
      this.hasMore = page.has_more;
    } catch (error) {
      reportError(error, "加载更多透明转发记录失败");
    } finally {
      this.loading = false;
    }
  },

  toggle(id: number): void {
    const next = new Set(this.selectedIds);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    this.selectedIds = next;
  },

  async removeSelected(): Promise<boolean> {
    const ids = [...this.selectedIds];
    if (ids.length === 0) return false;
    try {
      await backend.deleteBypassEntries(ids);
      this.selectedIds = new Set();
      await this.refresh();
      return true;
    } catch (error) {
      reportError(error, "删除透明转发记录失败");
      return false;
    }
  },

  async removeOne(id: number): Promise<boolean> {
    try {
      await backend.deleteBypassEntry(id);
      const selectedIds = new Set(this.selectedIds);
      selectedIds.delete(id);
      this.selectedIds = selectedIds;
      await this.refresh();
      return true;
    } catch (error) {
      reportError(error, "删除透明转发记录失败");
      return false;
    }
  },

  async clearTerminal(): Promise<number> {
    try {
      const result = await backend.clearBypassEntries();
      this.selectedIds = new Set();
      await this.refresh();
      return result.deleted;
    } catch (error) {
      reportError(error, "清空透明转发记录失败");
      return 0;
    }
  },
});
