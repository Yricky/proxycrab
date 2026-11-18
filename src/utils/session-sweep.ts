import type { CreateSessionRequest } from "../api/generated/CreateSessionRequest";
import type { SessionInterceptorsPayload } from "../api/generated/SessionInterceptorsPayload";
import type { SessionMetadata } from "../api/generated/SessionMetadata";
import type { SessionViewPayload } from "../api/generated/SessionViewPayload";

/**
 * 清扫会话：以原会话的身份信息与已保存配置，构造一个配置一致的新会话请求。
 * 这里只负责创建：原会话活跃时随后切换活跃身份，否则保持新会话非活跃；
 * 归档原会话时由后端断开其连接并等待相关任务结束。
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
