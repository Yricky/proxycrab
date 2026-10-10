import type {
  CreateSessionRequest,
  SessionInterceptorsPayload,
  SessionMetadata,
  SessionViewPayload,
} from "../api/types";

/**
 * 清扫会话：以原会话的身份信息与已保存配置，构造一个配置一致的新会话请求。
 * 这里只负责创建：活跃身份随后由 PUT /api/active-session 接管，只有那一步会断开原会话
 * 已建立的连接与在途请求，从而让随后的归档不被 pin 拒绝。
 */
export function sweepCreateRequest(
  session: SessionMetadata,
  view: SessionViewPayload,
  chains: SessionInterceptorsPayload,
): CreateSessionRequest {
  const chain = (items: SessionInterceptorsPayload["request"]) =>
    items.map(({ name, enabled }) => ({ name, enabled }));
  return {
    name: session.name,
    description: session.description,
    view: { columns: view.columns, filter: view.filter },
    interceptors: {
      request: chain(chains.request),
      response: chain(chains.response),
    },
  };
}
