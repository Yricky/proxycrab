/**
 * 权限目录展示元数据（前端侧）。文案、影响分组和脱敏能力显式配置，
 * 不依赖后端默认权限或 HTTP 方法；未配置的接口归入「其他」。
 */
export type ImpactGroupKey = "read" | "write" | "danger";

interface PermissionActionMeta {
  label: string;
  impact: ImpactGroupKey;
  /** 是否支持身份绑定的脱敏策略；省略表示不支持。 */
  redaction?: boolean;
}

/** 动作 id（`METHOD /route/template`）→ 展示元数据。 */
export const ACTION_META: Readonly<Record<string, PermissionActionMeta>> = {
  "GET /api/redaction-scripts": { label: "列出脱敏脚本", impact: "read" },
  "POST /api/redaction-scripts": { label: "创建脱敏脚本", impact: "write" },
  "GET /api/redaction-scripts/{name}": { label: "读取脱敏脚本", impact: "danger" },
  "PUT /api/redaction-scripts/{name}": { label: "更新脱敏脚本", impact: "write" },
  "DELETE /api/redaction-scripts/{name}": { label: "删除脱敏脚本", impact: "write" },
  "PUT /api/session-shares/{id}/redaction": { label: "绑定分享脱敏脚本", impact: "write" },
  "PUT /api/session-har-shares/{id}/redaction": { label: "绑定 HAR 脱敏脚本", impact: "write" },
  "GET /api/agents.md": { label: "读取 Agent 指令", impact: "read" },
  "GET /api/assets/{*asset_id}": { label: "读取资源", impact: "read" },
  "POST /api/assets/{*asset_id}": { label: "上传资源", impact: "write" },
  "GET /api/config": { label: "读取全局配置", impact: "read" },
  "GET /api/proxy/status": { label: "读取代理状态", impact: "read" },
  "POST /api/proxy/start": { label: "启动代理", impact: "write" },
  "POST /api/proxy/stop": { label: "停止代理", impact: "write" },
  "GET /api/sessions": { label: "列出 Session 及活动状态", impact: "read" },
  "POST /api/sessions": { label: "创建 Session", impact: "write" },
  "GET /api/archived-sessions": { label: "列出归档 Session", impact: "read" },
  "POST /api/sessions/{id}/archive": { label: "归档 Session", impact: "danger" },
  "POST /api/archived-sessions/{id}/restore": { label: "恢复归档 Session", impact: "write" },
  "DELETE /api/archived-sessions/{id}": { label: "永久删除归档 Session", impact: "danger" },
  "PUT /api/active-session": { label: "切换活动 Session", impact: "write" },
  "PUT /api/sessions/{id}": { label: "更新 Session", impact: "write" },
  "PUT /api/sessions/{id}/filter": { label: "保存 Session 过滤条件", impact: "write" },
  "POST /api/session-shares": { label: "启用 Session 分享", impact: "write" },
  "GET /api/session-shares/{id}": { label: "查看 Session 分享状态", impact: "read" },
  "DELETE /api/session-shares/{id}": { label: "禁用 Session 分享", impact: "write" },
  "POST /api/session-har-shares": { label: "启用 HAR 分享", impact: "write" },
  "GET /api/session-har-shares/{id}": { label: "查看 HAR 分享状态", impact: "read" },
  "DELETE /api/session-har-shares/{id}": { label: "禁用 HAR 分享", impact: "write" },
  "POST /api/logs/ids": { label: "查询日志 ID", impact: "write", redaction: true },
  "POST /api/logs/views": { label: "批量渲染日志", impact: "write", redaction: true },
  "GET /api/logs/{id}/body": { label: "读取日志正文", impact: "read", redaction: true },
  "GET /api/logs/{id}": { label: "读取日志详情", impact: "read", redaction: true },
  "GET /api/session-view": { label: "读取 Session 视图", impact: "read", redaction: true },
  "PUT /api/session-view": { label: "替换 Session 视图", impact: "write" },
  "GET /api/session-interceptors": { label: "读取拦截器链", impact: "read" },
  "PUT /api/session-interceptors": { label: "替换拦截器链", impact: "write" },
  "GET /api/column-scripts": { label: "列出自定义列脚本", impact: "read" },
  "POST /api/column-scripts": { label: "创建自定义列脚本", impact: "write" },
  "GET /api/column-scripts/{name}": { label: "读取自定义列脚本", impact: "read" },
  "PUT /api/column-scripts/{name}": { label: "更新自定义列脚本", impact: "write" },
  "DELETE /api/column-scripts/{name}": { label: "删除自定义列脚本", impact: "danger" },
  "GET /api/filter-scripts": { label: "列出过滤脚本", impact: "read" },
  "POST /api/filter-scripts": { label: "创建过滤脚本", impact: "write" },
  "POST /api/filter-scripts/{name}/debug": {
    label: "调试过滤脚本",
    impact: "write",
    redaction: true,
  },
  "GET /api/filter-scripts/{name}": { label: "读取过滤脚本", impact: "read" },
  "PUT /api/filter-scripts/{name}": { label: "更新过滤脚本", impact: "write" },
  "DELETE /api/filter-scripts/{name}": { label: "删除过滤脚本", impact: "danger" },
  "GET /api/routing-scripts": { label: "列出分流脚本", impact: "read" },
  "POST /api/routing-scripts": { label: "创建分流脚本", impact: "write" },
  "GET /api/routing-scripts/{name}": { label: "读取分流脚本", impact: "read" },
  "PUT /api/routing-scripts/{name}": { label: "更新分流脚本", impact: "write" },
  "DELETE /api/routing-scripts/{name}": { label: "删除分流脚本", impact: "danger" },
  "GET /api/routing-script-selection": { label: "读取活动分流脚本", impact: "read" },
  "PUT /api/routing-script-selection": { label: "选择活动分流脚本", impact: "write" },
  "GET /api/interceptors": { label: "列出拦截器", impact: "read" },
  "POST /api/interceptors": { label: "创建拦截器", impact: "write" },
  "GET /api/interceptors/{kind}/{name}": { label: "读取拦截器", impact: "read" },
  "PUT /api/interceptors/{kind}/{name}": { label: "更新拦截器", impact: "write" },
  "DELETE /api/interceptors/{kind}/{name}": { label: "删除拦截器", impact: "danger" },
  "GET /api/breakpoints": { label: "列出活动断点", impact: "read", redaction: true },
  "GET /api/breakpoints/{id}/body": { label: "读取断点正文", impact: "read", redaction: true },
  "GET /api/breakpoints/{id}": { label: "读取断点详情", impact: "read", redaction: true },
  "POST /api/breakpoints/{id}/extend": { label: "延长断点等待", impact: "write", redaction: true },
  "POST /api/breakpoints/{id}/release": { label: "释放断点", impact: "write" },
  "POST /api/breakpoints/{id}/execute": { label: "执行临时脚本", impact: "write", redaction: true },
  "GET /api/bypass": { label: "读取 Bypass 流量", impact: "read", redaction: true },
  "DELETE /api/bypass": { label: "清空 Bypass 流量", impact: "danger" },
  "POST /api/bypass/delete": { label: "批量删除 Bypass 流量", impact: "danger" },
  "DELETE /api/bypass/{id}": { label: "删除 Bypass 流量", impact: "danger" },
  "GET /api/ca": { label: "读取 CA 证书", impact: "read" },
  "GET /api/clash-subscription": { label: "读取 Clash 订阅状态", impact: "read" },
  "PUT /api/clash-subscription": { label: "开启或更新 Clash 订阅", impact: "write" },
  "DELETE /api/clash-subscription": { label: "关闭 Clash 订阅", impact: "write" },
  "GET /api/system-logs": { label: "读取系统日志", impact: "read" },
  "DELETE /api/system-logs": { label: "清空系统日志", impact: "danger" },
  "GET /api/assets": { label: "列出资源", impact: "read" },
  "POST /api/replay": { label: "重放请求", impact: "write" },
  "GET /api/logs/{id}/interceptors/{execution_id}/content": {
    label: "读取拦截器执行脚本",
    impact: "read",
  },
  "GET /api/logs/{id}/interceptors/{execution_id}/snapshot": {
    label: "读取拦截器快照",
    impact: "read",
    redaction: true,
  },
  "GET /api/clash-config": { label: "读取 Clash 配置", impact: "read" },
  "PUT /api/clash-config": { label: "更新 Clash 配置", impact: "write" },
  "DELETE /api/clash-config": { label: "重置 Clash 配置", impact: "write" },
};

/** 「功能」维度分组定义（顺序即展示顺序；prefixes 按 route_template 前缀匹配）。 */
export interface FunctionGroupDef {
  key: string;
  label: string;
  prefixes: string[];
}

export const FUNCTION_GROUPS: FunctionGroupDef[] = [
  { key: "general", label: "基础配置", prefixes: ["/api/agents.md", "/api/config", "/api/assets"] },
  { key: "proxy", label: "代理服务", prefixes: ["/api/proxy/", "/api/clash-subscription"] },
  {
    key: "sessions",
    label: "Session",
    prefixes: [
      "/api/sessions",
      "/api/session-shares",
      "/api/session-har-shares",
      "/api/archived-sessions",
      "/api/active-session",
    ],
  },
  { key: "logs", label: "抓包日志", prefixes: ["/api/logs/"] },
  {
    key: "views",
    label: "Session 视图",
    prefixes: ["/api/session-view", "/api/session-interceptors"],
  },
  {
    key: "scripts",
    label: "脚本与拦截器",
    prefixes: [
      "/api/redaction-scripts",
      "/api/column-scripts",
      "/api/filter-scripts",
      "/api/routing-scripts",
      "/api/routing-script-selection",
      "/api/interceptors",
    ],
  },
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

/** 「影响」维度分组（顺序即展示顺序）。 */
export const IMPACT_GROUPS: ReadonlyArray<{ key: ImpactGroupKey; label: string }> = [
  { key: "read", label: "仅读取" },
  { key: "write", label: "有写操作" },
  { key: "danger", label: "危险操作" },
];

export function impactGroupOf(actionId: string): ImpactGroupKey | "other" {
  return ACTION_META[actionId]?.impact ?? OTHER_GROUP.key;
}

/** 展示分组维度。 */
export type PermissionGroupDimension = "function" | "impact";

export const DIMENSION_OPTIONS: ReadonlyArray<{ value: PermissionGroupDimension; label: string }> =
  [
    { value: "function", label: "功能" },
    { value: "impact", label: "影响" },
  ];
