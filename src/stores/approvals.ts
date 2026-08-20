import { reactive } from "vue";
import type { Unsubscribe } from "../api/backend";
import { runtimeBackend as backend } from "../api/runtime-backend";
import type { PendingApproval, ResolveApprovalRequest } from "../api/types";
import { reportError } from "./app";

let unlisten: Unsubscribe | undefined;
let refreshTimer: number | undefined;

export const approvalsStore = reactive({
  items: [] as PendingApproval[],
  count: 0,
  loading: false,
  resolving: new Set<number>(),

  async refresh(): Promise<void> {
    this.loading = true;
    try {
      this.items = await backend.listHttpApprovals();
      this.count = this.items.length;
    } catch (error) {
      reportError(error, "获取接口审批列表失败");
    } finally {
      this.loading = false;
    }
  },

  scheduleRefresh(): void {
    if (refreshTimer !== undefined) return;
    refreshTimer = window.setTimeout(() => {
      refreshTimer = undefined;
      void this.refresh();
    }, 40);
  },

  async start(): Promise<void> {
    if (unlisten) return;
    try {
      unlisten = await backend.subscribeApprovalChanges((count) => {
        this.count = count;
        this.scheduleRefresh();
      });
      await this.refresh();
    } catch (error) {
      reportError(error, "启动接口审批同步失败");
    }
  },

  stop(): void {
    unlisten?.();
    unlisten = undefined;
    if (refreshTimer !== undefined) {
      window.clearTimeout(refreshTimer);
      refreshTimer = undefined;
    }
    this.items = [];
    this.count = 0;
    this.resolving.clear();
  },

  async resolve(id: number, request: ResolveApprovalRequest): Promise<void> {
    this.resolving.add(id);
    try {
      await backend.resolveHttpApproval(id, request);
      await this.refresh();
    } catch (error) {
      await this.refresh();
      reportError(error, "处理接口审批失败");
    } finally {
      this.resolving.delete(id);
    }
  },
});
