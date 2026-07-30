import { reactive } from "vue";
import { createTauriBackend } from "../api/tauri-backend";
import type {
  InterceptorKind,
  SessionInterceptorItem,
  SessionInterceptorsPayload,
} from "../api/types";
import { reportError } from "./app";

const backend = createTauriBackend();

function clone(items: SessionInterceptorItem[]): SessionInterceptorItem[] {
  return items.map((item) => ({ ...item }));
}

export const interceptorsStore = reactive({
  sessionId: null as number | null,
  request: [] as SessionInterceptorItem[],
  response: [] as SessionInterceptorItem[],
  loading: false,
  saving: false,
  loadVersion: 0,

  apply(payload: SessionInterceptorsPayload): void {
    this.sessionId = payload.session_id;
    this.request = clone(payload.request);
    this.response = clone(payload.response);
  },

  async refresh(sessionId: number | null): Promise<void> {
    const version = ++this.loadVersion;
    if (sessionId === null) {
      this.sessionId = null;
      this.request = [];
      this.response = [];
      this.loading = false;
      return;
    }
    this.loading = true;
    try {
      const payload = await backend.getSessionInterceptors(sessionId);
      if (version === this.loadVersion) this.apply(payload);
    } catch (error) {
      if (version === this.loadVersion) reportError(error, "加载会话拦截器失败");
    } finally {
      if (version === this.loadVersion) this.loading = false;
    }
  },

  async replace(
    sessionId: number,
    request: SessionInterceptorItem[],
    response: SessionInterceptorItem[],
  ): Promise<boolean> {
    if (this.saving) return false;
    const previousRequest = clone(this.request);
    const previousResponse = clone(this.response);
    this.request = clone(request);
    this.response = clone(response);
    this.saving = true;
    try {
      const payload = await backend.replaceSessionInterceptors(sessionId, {
        request: request.map(({ name, enabled }) => ({ name, enabled })),
        response: response.map(({ name, enabled }) => ({ name, enabled })),
      });
      if (this.sessionId === null || this.sessionId === sessionId) this.apply(payload);
      return true;
    } catch (error) {
      if (this.sessionId === sessionId) {
        this.request = previousRequest;
        this.response = previousResponse;
      }
      reportError(error, "保存会话拦截器失败");
      return false;
    } finally {
      this.saving = false;
    }
  },

  async toggle(kind: InterceptorKind, index: number): Promise<boolean> {
    if (this.sessionId === null) return false;
    const request = clone(this.request);
    const response = clone(this.response);
    const target = kind === "request" ? request : response;
    const item = target[index];
    if (!item?.valid) return false;
    item.enabled = !item.enabled;
    return this.replace(this.sessionId, request, response);
  },

  async move(kind: InterceptorKind, from: number, to: number): Promise<boolean> {
    if (this.sessionId === null || from === to) return false;
    const request = clone(this.request);
    const response = clone(this.response);
    const target = kind === "request" ? request : response;
    if (from < 0 || from >= target.length || to < 0 || to >= target.length) return false;
    const [item] = target.splice(from, 1);
    target.splice(to, 0, item);
    return this.replace(this.sessionId, request, response);
  },

  async insert(kind: InterceptorKind, index: number, name: string): Promise<boolean> {
    if (this.sessionId === null) return false;
    const request = clone(this.request);
    const response = clone(this.response);
    const target = kind === "request" ? request : response;
    if (target.length >= 12 || target.some((item) => item.name === name)) return false;
    target.splice(Math.max(0, Math.min(index, target.length)), 0, {
      name,
      enabled: true,
      valid: true,
    });
    return this.replace(this.sessionId, request, response);
  },

  async remove(kind: InterceptorKind, index: number): Promise<boolean> {
    if (this.sessionId === null) return false;
    const request = clone(this.request);
    const response = clone(this.response);
    const target = kind === "request" ? request : response;
    if (index < 0 || index >= target.length) return false;
    target.splice(index, 1);
    return this.replace(this.sessionId, request, response);
  },
});
