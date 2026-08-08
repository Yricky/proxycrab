import type { ApiActionView } from "../api/types";

/**
 * 权限目录展示元数据（前端侧）。
 *
 * 后端 ApiActionView 只下发 id/method/route_template/default_mode，
 * 文案与分组由前端自行推导，避免展示概念与传输模型耦合。
 */

/** 动作 id（`METHOD /route/template`）→ 中文文案。 */
export const ACTION_LABELS: Record<string, string> = {
  "GET /api/agents.md": "读取 Agent 指令",
  "GET /api/workspace": "读取工作区",
  "PUT /api/workspace": "设置下次启动工作区",
  "GET /api/assets/{*asset_id}": "读取资源",
  "POST /api/assets/{*asset_id}": "上传资源",
  "GET /api/config": "读取全局配置",
  "PUT /api/config": "替换全局配置",
  "GET /api/proxy/status": "读取代理状态",
  "POST /api/proxy/start": "启动代理",
  "POST /api/proxy/stop": "停止代理",
  "GET /api/sessions": "列出 Session",
  "POST /api/sessions": "创建 Session",
  "GET /api/archived-sessions": "列出归档 Session",
  "POST /api/sessions/{id}/archive": "归档 Session",
  "POST /api/archived-sessions/{id}/restore": "恢复归档 Session",
  "DELETE /api/archived-sessions/{id}": "永久删除归档 Session",
  "GET /api/active-session": "读取活动 Session",
  "PUT /api/active-session": "切换活动 Session",
  "PUT /api/sessions/{id}": "更新 Session",
  "POST /api/logs/export": "导出抓包日志",
  "POST /api/logs/ids": "查询日志 ID",
  "POST /api/logs/views": "批量渲染日志",
  "GET /api/logs/{id}/body": "读取日志正文",
  "GET /api/logs/{id}": "读取日志详情",
  "GET /api/session-view": "读取 Session 视图",
  "PUT /api/session-view": "替换 Session 视图",
  "GET /api/session-interceptors": "读取拦截器链",
  "PUT /api/session-interceptors": "替换拦截器链",
  "GET /api/column-scripts": "列出自定义列脚本",
  "POST /api/column-scripts": "创建自定义列脚本",
  "GET /api/column-scripts/{name}": "读取自定义列脚本",
  "PUT /api/column-scripts/{name}": "更新自定义列脚本",
  "DELETE /api/column-scripts/{name}": "删除自定义列脚本",
  "GET /api/filter-scripts": "列出过滤脚本",
  "POST /api/filter-scripts": "创建过滤脚本",
  "POST /api/filter-scripts/{name}/debug": "调试过滤脚本",
  "GET /api/filter-scripts/{name}": "读取过滤脚本",
  "PUT /api/filter-scripts/{name}": "更新过滤脚本",
  "DELETE /api/filter-scripts/{name}": "删除过滤脚本",
  "GET /api/routing-scripts": "列出分流脚本",
  "POST /api/routing-scripts": "创建分流脚本",
  "GET /api/routing-scripts/{name}": "读取分流脚本",
  "PUT /api/routing-scripts/{name}": "更新分流脚本",
  "DELETE /api/routing-scripts/{name}": "删除分流脚本",
  "GET /api/routing-script-selection": "读取活动分流脚本",
  "PUT /api/routing-script-selection": "选择活动分流脚本",
  "GET /api/interceptors": "列出拦截器",
  "POST /api/interceptors": "创建拦截器",
  "GET /api/interceptors/{kind}/{name}": "读取拦截器",
  "PUT /api/interceptors/{kind}/{name}": "更新拦截器",
  "DELETE /api/interceptors/{kind}/{name}": "删除拦截器",
  "GET /api/breakpoints": "列出活动断点",
  "GET /api/breakpoints/{id}/body": "读取断点正文",
  "GET /api/breakpoints/{id}": "读取断点详情",
  "POST /api/breakpoints/{id}/extend": "延长断点等待",
  "POST /api/breakpoints/{id}/release": "释放断点",
  "POST /api/breakpoints/{id}/execute": "执行临时脚本",
  "GET /api/bypass": "读取 Bypass 流量",
  "DELETE /api/bypass": "清空 Bypass 流量",
  "POST /api/bypass/delete": "批量删除 Bypass 流量",
  "DELETE /api/bypass/{id}": "删除 Bypass 流量",
  "GET /api/ca": "读取 CA 证书",
  "POST /api/ca": "重新生成 CA 证书",
  "GET /api/system-logs": "读取系统日志",
  "DELETE /api/system-logs": "清空系统日志",
};

/** 「功能」维度分组定义（顺序即展示顺序；prefixes 按 route_template 前缀匹配）。 */
export interface FunctionGroupDef {
  key: string;
  label: string;
  prefixes: string[];
}

export const FUNCTION_GROUPS: FunctionGroupDef[] = [
  { key: "general", label: "基础与工作区", prefixes: ["/api/agents.md", "/api/workspace", "/api/config", "/api/assets"] },
  { key: "proxy", label: "代理服务", prefixes: ["/api/proxy/"] },
  { key: "sessions", label: "Session", prefixes: ["/api/sessions", "/api/archived-sessions", "/api/active-session"] },
  { key: "logs", label: "抓包日志", prefixes: ["/api/logs/"] },
  { key: "views", label: "Session 视图", prefixes: ["/api/session-view", "/api/session-interceptors"] },
  { key: "scripts", label: "脚本与拦截器", prefixes: ["/api/column-scripts", "/api/filter-scripts", "/api/routing-scripts", "/api/routing-script-selection", "/api/interceptors"] },
  { key: "breakpoints", label: "断点", prefixes: ["/api/breakpoints"] },
  { key: "bypass", label: "Bypass 流量", prefixes: ["/api/bypass"] },
  { key: "ca", label: "CA 证书", prefixes: ["/api/ca"] },
  { key: "system_logs", label: "系统日志", prefixes: ["/api/system-logs"] },
];

export const OTHER_GROUP = { key: "other", label: "其他" } as const;

export function functionGroupOf(routeTemplate: string): { key: string; label: string } {
  for (const group of FUNCTION_GROUPS) {
    if (group.prefixes.some((prefix) => routeTemplate.startsWith(prefix))) {
      return { key: group.key, label: group.label };
    }
  }
  return OTHER_GROUP;
}

/** 「影响」维度分组。 */
export type ImpactGroupKey = "read" | "write" | "danger";

export const IMPACT_GROUPS: ReadonlyArray<{ key: ImpactGroupKey; label: string }> = [
  { key: "read", label: "仅读取" },
  { key: "write", label: "有写操作" },
  { key: "danger", label: "危险操作" },
];

/**
 * 影响分类（基于动作默认值静态判定，不随用户配置漂移）：
 * - 默认 Deny 的动作即「危险操作」；
 * - 其余 GET 为「仅读取」；
 * - 其余非 GET（POST/PUT）为「有写操作」。
 */
export function impactGroupOf(action: ApiActionView): ImpactGroupKey {
  if (action.default_mode === "deny") return "danger";
  if (action.method === "GET") return "read";
  return "write";
}

/** 展示分组维度。 */
export type PermissionGroupDimension = "function" | "impact";

export const DIMENSION_OPTIONS: ReadonlyArray<{ value: PermissionGroupDimension; label: string }> = [
  { value: "function", label: "功能" },
  { value: "impact", label: "影响" },
];
