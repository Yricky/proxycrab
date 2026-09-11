# Request Replay Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a request-replay feature: an editable replay window (method/URL/headers/body + session picker + execute) that injects a request into the shared MITM network chain under a chosen session, recorded with source `ProxyCrabRequest`, plus a `POST /api/replay` management API.

**Architecture:** Refactor the proxy pipeline into two stages — attribution (bind traffic to a session + its interceptors) and the shared MITM network chain — then let replay construct the attribution context directly and inject into the shared chain. Replay requires the proxy to be running (reuses the current connection generation's task group / activity tracking). The replay window only sends; the resulting capture lives in the target session like any proxied request.

**Tech Stack:** Rust (axum, hyper, tokio, thiserror), Vue 3 + TypeScript (vite, vitest), Tauri commands, Node skill scripts.

**Key spec points (confirmed with user):**
- Window: floating window, random-UID id, multi-open. Entries: log-detail (prefill final values), interceptor-snapshot (prefill snapshot values), toolbar 小工具 → 发送请求 (blank).
- Method: dropdown GET/POST/PUT/DELETE/PATCH/HEAD/OPTIONS + one free-input custom item.
- URL: single editable input containing the full query, keeping the existing segmented highlight coloring; no separate query table.
- Headers: table editing — edit existing rows, middle empty rows auto-removed, exactly one trailing empty row always present, non-empty rows drag-sortable.
- Body: three mutually exclusive modes — `text` (Monaco, charset=utf8 only) / `body_ref` (session_id+log_id+side → stored blob body of that capture side) / `asset` (VSCode-like tree picker over workspace assets). No `body` field = no body.
- Warnings: Content-Length mismatch → fix-to-actual button; non-empty body missing Content-Length → add button; non-empty body missing Content-Type → guess-and-fill button (JSON → `application/json`, printable text → `text/plain`, fallback `application/octet-stream`).
- Sessions dropdown: unarchived only, defaults to active session.
- Execution: loading + re-entry guard, repeatable; success shows clickable log_id; failure shows error. Breakpoints etc. need no special handling.
- API: `POST /api/replay?session=<id>`, body `{ method, url, headers: [[name,value],...], body?: {type:"text",text,charset?} | {type:"body_ref",session_id,log_id,side} | {type:"asset",asset_id} }`, returns `{ log_id }` as soon as the capture is created (does not wait for response). New permission action `POST /api/replay` default **Allow**. New `GET /api/assets` list endpoint (tree built client-side from metadata; content fetch reuses existing `GET /api/assets/{id}?format=raw`).
- Replay traffic: full Lua interceptor pipeline applies; no SSL-decryption involvement (direct outbound TLS, unaffected by ssl-pinning rules); outbound behavior identical to proxied traffic. Requires proxy running.
- Sync `skills/proxycrab/` docs/scripts/evals and project docs. Web (CLI-served) UI must have feature parity.

---

### Task 1: mitm 管线签名重构（TrafficSource + 泛型 body + capture id 回报）

**Files:**
- Modify: `crates/proxy-crab-mitm/src/proxy.rs`
- Modify: `crates/proxy-crab-mitm/src/proxy/mitm.rs`
- Modify: `crates/proxy-crab-mitm/src/runtime.rs`

引入 `TrafficSource`，把会话链路的 `source: SocketAddr` 改为 `TrafficSource`（落库 label 为客户端地址或 `ProxyCrabRequest`），把 `handle_session_http_request` 的入参从 `Request<Incoming>` 改为 `Request<ProxyBody>`，并加一个可选 oneshot 用于在 capture 建立后立即回报 log_id。路由层（`resolve_route` / Lua routing script）保持使用 `SocketAddr` 不变。

- [ ] **Step 1: 在 `proxy.rs` 中添加 `TrafficSource`（放在 `RouteDecision` 之前）**

```rust
#[derive(Debug, Clone, Copy)]
pub(crate) enum TrafficSource {
    Client(SocketAddr),
    Replay,
}

impl TrafficSource {
    pub(crate) fn label(self) -> String {
        match self {
            Self::Client(address) => address.to_string(),
            Self::Replay => "ProxyCrabRequest".to_string(),
        }
    }
}
```

- [ ] **Step 2: `begin_connect_capture` 改用 `TrafficSource`**

签名改为 `source: TrafficSource`，函数体 `store.begin(&source.label(), request, "connect")`。

- [ ] **Step 3: `handle_connect` 中构造 `TrafficSource::Client` 并传递**

`resolve_route` 仍用 `source`（SocketAddr，bypass 分支也不变）。路由命中 Session 后：

```rust
    let traffic = TrafficSource::Client(source);
    let capture = match begin_connect_capture(&tracker, &pin, traffic, &request_data) { ... };
    // upgrade 任务内：
    mitm::process_connect(TokioIo::new(upgraded), authority, traffic, pin, capture, cancellation, tracker).await;
```

- [ ] **Step 4: `handle_http_request` 的 Session 分支转换 body 类型并传 `TrafficSource`**

```rust
        RouteDecision::Session(session) => {
            let request = request.map(|body| {
                body.map_err(|error| Box::new(error) as BoxError).boxed_unsync()
            });
            mitm::handle_session_http_request(
                request,
                TrafficSource::Client(source),
                runtime,
                session.id,
                cancellation,
                tracker,
                None,
            )
            .await
        }
```

- [ ] **Step 5: `is_upgrade_request` 泛型化为 `fn is_upgrade_request<B>(request: &Request<B>) -> bool`**（函数体不变）。

- [ ] **Step 6: `ProxyController` 暴露 `replay_generation`；`ConnectionGeneration`/`ConnectionRegistry` 提升为 `pub(crate)`**

```rust
    /// 重放复用当前连接代的任务组/取消令牌/活动跟踪；代理未运行返回 None。
    pub(crate) fn replay_generation(&self) -> Option<ConnectionGeneration> {
        if !matches!(
            &*self.status.read().expect("proxy status lock poisoned"),
            ProxyStatus::Running { .. }
        ) {
            return None;
        }
        let registry = self.connections.lock().expect("proxy connections lock poisoned");
        registry.as_ref().map(ConnectionRegistry::current)
    }
```

- [ ] **Step 7: `runtime.rs` 暴露 controller 访问器**

```rust
    pub(crate) fn proxy_controller(&self) -> &ProxyController {
        &self.proxy
    }
```

- [ ] **Step 8: `mitm.rs` — `process_connect` / `serve_mitm_tls` 签名改为 `source: TrafficSource`，服务闭包内转换 body**

`serve_mitm_tls` 的 service 闭包：

```rust
    let service = service_fn(move |mut request| {
        inject_https_authority(&mut request, &authority);
        let local_ca = is_ca_download(&request_data(&request));
        let runtime = runtime.clone();
        let cancellation = service_cancellation.clone();
        let tracker = tracker.clone();
        async move {
            if local_ca {
                return Ok::<_, Infallible>(certificate_response(&runtime));
            }
            let request = request.map(|body| {
                body.map_err(|error| Box::new(error) as BoxError).boxed_unsync()
            });
            Ok::<_, Infallible>(
                handle_session_http_request(
                    request, source, runtime, session_id, cancellation, tracker, None,
                )
                .await,
            )
        }
    });
```

- [ ] **Step 9: `handle_session_http_request` 新签名 + capture id 回报**

```rust
pub(crate) async fn handle_session_http_request(
    mut request: Request<ProxyBody>,
    source: TrafficSource,
    runtime: Arc<ProxyCrab>,
    session_id: u64,
    cancellation: CancellationToken,
    tracker: TaskGroup,
    capture_id_tx: Option<tokio::sync::oneshot::Sender<u64>>,
) -> Response<ProxyBody> {
```

函数体内改动：
1. `store.begin(&source.to_string(), ...)` → `store.begin(&source.label(), &request_data, "request")`。
2. `begin_capture` 成功之后立即 `if let Some(tx) = capture_id_tx { let _ = tx.send(capture_id); }`。
3. `DeferredBody::new(incoming, ...)` 不变（本就泛型；`incoming` 现在是 `ProxyBody`）。
（`pub(super)` 放宽为 `pub(crate)` 以便 replay 子模块调用。）

- [ ] **Step 10: 全量回归**

Run: `cargo test -p proxy-crab-mitm`
Expected: 全部通过（纯重构，无行为变化）

- [ ] **Step 11: Commit**

```bash
git add crates/proxy-crab-mitm/src/proxy.rs crates/proxy-crab-mitm/src/proxy/mitm.rs crates/proxy-crab-mitm/src/runtime.rs
git commit -m "refactor(mitm): split traffic attribution from the shared MITM chain"
```

---

### Task 2: Replay 模块（`proxy/replay.rs`）

**Files:**
- Create: `crates/proxy-crab-mitm/src/proxy/replay.rs`
- Modify: `crates/proxy-crab-mitm/src/proxy.rs`（`pub mod replay;`）
- Modify: `crates/proxy-crab-mitm/src/lib.rs`（re-export）

- [ ] **Step 1: 新建 `crates/proxy-crab-mitm/src/proxy/replay.rs`**

```rust
use std::sync::Arc;

use anyhow::Context;
use bytes::Bytes;
use http_body_util::BodyExt;
use hyper::{Request, Version, header::HeaderName, header::HeaderValue};
use thiserror::Error;

use super::{TrafficSource, body::boxed_full, mitm};
use crate::{
    ProxyCrab,
    storage::{BodySide, BodySourceData},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayBody {
    Text { text: String },
    BodyRef { session_id: u64, log_id: u64, side: BodySide },
    Asset { asset_id: String },
}

#[derive(Debug, Clone)]
pub struct ReplayRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<ReplayBody>,
}

#[derive(Debug, Error)]
pub enum ReplayError {
    #[error("session not found or archived")]
    SessionNotFound,
    #[error("proxy is not running")]
    ProxyNotRunning,
    #[error("referenced capture body not found")]
    BodyNotFound,
    #[error("referenced asset not found")]
    AssetNotFound,
    #[error("invalid replay request: {0}")]
    Invalid(String),
    #[error("replay failed: {0}")]
    Internal(#[from] anyhow::Error),
}

impl ProxyCrab {
    /// 在指定 Session 中重放一条请求：复用 MITM 会话链路（拦截器、捕获、出站），
    /// capture 建立后立即返回 log_id，不等待响应完成。要求代理运行中。
    pub async fn replay(
        self: &Arc<Self>,
        session_id: u64,
        request: ReplayRequest,
    ) -> std::result::Result<u64, ReplayError> {
        if !self.sessions().iter().any(|session| session.id == session_id) {
            return Err(ReplayError::SessionNotFound);
        }
        if request.method.trim().is_empty() {
            return Err(ReplayError::Invalid("method must not be empty".into()));
        }
        if !(request.url.starts_with("http://") || request.url.starts_with("https://")) {
            return Err(ReplayError::Invalid(
                "url must be an absolute http(s) URL".into(),
            ));
        }
        let body = match &request.body {
            None => Bytes::new(),
            Some(ReplayBody::Text { text }) => Bytes::copy_from_slice(text.as_bytes()),
            Some(ReplayBody::Asset { asset_id }) => {
                let asset = self.asset(asset_id)?.ok_or(ReplayError::AssetNotFound)?;
                Bytes::from(tokio::fs::read(asset.path()).await?)
            }
            Some(ReplayBody::BodyRef { session_id: source_session, log_id, side }) => {
                let source = self
                    .capture_body_source(*source_session, *log_id, *side)?
                    .ok_or(ReplayError::BodyNotFound)?;
                match source.data {
                    BodySourceData::Bytes(bytes) => Bytes::from(bytes),
                    BodySourceData::File(path) => Bytes::from(tokio::fs::read(&path).await?),
                }
            }
        };
        let generation = self
            .proxy_controller()
            .replay_generation()
            .ok_or(ReplayError::ProxyNotRunning)?;
        let mut builder = Request::builder()
            .method(request.method.as_str())
            .uri(request.url.as_str())
            .version(Version::HTTP_11);
        {
            let headers = builder
                .headers_mut()
                .ok_or_else(|| ReplayError::Invalid("request builder rejected inputs".into()))?;
            for (name, value) in &request.headers {
                let name = HeaderName::from_bytes(name.as_bytes())
                    .map_err(|_| ReplayError::Invalid(format!("invalid header name: {name}")))?;
                let value = HeaderValue::from_str(value).map_err(|_| {
                    ReplayError::Invalid(format!("invalid value for header {name}"))
                })?;
                headers.append(name, value);
            }
        }
        let http_request = builder
            .body(boxed_full(body))
            .map_err(|error| ReplayError::Invalid(error.to_string()))?;
        let (capture_tx, capture_rx) = tokio::sync::oneshot::channel();
        let runtime = self.clone();
        let cancellation = generation.cancellation.clone();
        let tasks = generation.tasks.clone();
        generation.tasks.spawn(async move {
            let response = mitm::handle_session_http_request(
                http_request,
                TrafficSource::Replay,
                runtime,
                session_id,
                cancellation,
                tasks,
                Some(capture_tx),
            )
            .await;
            // 完整消费响应 body 以驱动响应捕获完成；重放调用方不需要响应内容。
            let _ = response.into_body().collect().await;
        });
        capture_rx
            .await
            .context("replay capture did not start")
            .map_err(ReplayError::Internal)
    }
}
```

已知限制（本期接受）：body 全量读入内存；重放固定 HTTP/1.1 语义。

- [ ] **Step 2: 挂载模块**

`proxy.rs` 中 `pub(crate) mod body;` 附近添加 `pub mod replay;`；`lib.rs` 添加 `pub use proxy::replay::{ReplayBody, ReplayError, ReplayRequest};`。

- [ ] **Step 3: 编译**

Run: `cargo check -p proxy-crab-mitm`
Expected: 通过

- [ ] **Step 4: Commit**

```bash
git add crates/proxy-crab-mitm/src/proxy/replay.rs crates/proxy-crab-mitm/src/proxy.rs crates/proxy-crab-mitm/src/lib.rs
git commit -m "feat(mitm): add session-scoped request replay entry point"
```

---

### Task 3: Replay 集成测试

**Files:**
- Modify: `crates/proxy-crab-mitm/tests/proxy_integration.rs`

- [ ] **Step 1: 写测试（追加到文件末尾），import 增加 `use proxy_crab_mitm::{ReplayBody, ReplayError, ReplayRequest};`**

```rust
// ---------- replay ----------

async fn wait_capture_success(runtime: &ProxyCrab, session_id: u64, id: u64) {
    timeout(Duration::from_secs(5), async {
        loop {
            if let Some(detail) = runtime.capture(session_id, id).unwrap() {
                if detail.summary.outcome == CaptureOutcome::Success {
                    return;
                }
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("replay capture did not complete in time");
}

#[tokio::test]
async fn replay_records_capture_with_proxycrab_source() {
    let (_app_data, runtime, _proxy_port) = runtime().await;
    let session = runtime.create_session(None, None).unwrap();
    let (upstream_port, _upstream) = fixed_http_upstream().await;

    let log_id = runtime
        .replay(
            session.id,
            ReplayRequest {
                method: "POST".into(),
                url: format!("http://127.0.0.1:{upstream_port}/replay-target?a=1"),
                headers: vec![
                    ("content-type".into(), "text/plain".into()),
                    ("x-replay".into(), "yes".into()),
                ],
                body: Some(ReplayBody::Text { text: "hello replay".into() }),
            },
        )
        .await
        .unwrap();
    wait_capture_success(&runtime, session.id, log_id).await;

    let detail = runtime.capture(session.id, log_id).unwrap().unwrap();
    assert_eq!(detail.summary.source, "ProxyCrabRequest");
    assert_eq!(detail.summary.request.method, "POST");
    assert_eq!(
        detail.summary.request.uri,
        format!("http://127.0.0.1:{upstream_port}/replay-target?a=1")
    );
    assert_eq!(
        detail.summary.request.headers.get("x-replay").unwrap(),
        &vec!["yes".to_string()]
    );
    assert_eq!(detail.summary.response.unwrap().status, 200);
    runtime.stop_proxy().await.unwrap();
}

#[tokio::test]
async fn replay_passes_through_session_interceptors() {
    let (_app_data, runtime, _proxy_port) = runtime().await;
    let session = runtime.create_session(None, None).unwrap();
    let (upstream_port, _upstream) = fixed_http_upstream().await;
    runtime
        .create_script(
            ScriptKind::RequestInterceptor,
            Script {
                name: "mark-replay".into(),
                content: "req.headers:set('x-intercepted', '1')".into(),
            },
        )
        .unwrap();
    runtime
        .replace_session_interceptors(
            session.id,
            SessionInterceptors {
                request: vec![SessionInterceptor { name: "mark-replay".into(), enabled: true }],
                response: Vec::new(),
            },
        )
        .unwrap();

    let log_id = runtime
        .replay(
            session.id,
            ReplayRequest {
                method: "GET".into(),
                url: format!("http://127.0.0.1:{upstream_port}/intercepted"),
                headers: Vec::new(),
                body: None,
            },
        )
        .await
        .unwrap();
    wait_capture_success(&runtime, session.id, log_id).await;

    let detail = runtime.capture(session.id, log_id).unwrap().unwrap();
    assert_eq!(
        detail.summary.request.headers.get("x-intercepted").unwrap(),
        &vec!["1".to_string()]
    );
    assert_eq!(detail.request_interceptors.len(), 1);
    assert_eq!(detail.request_interceptors[0].name, "mark-replay");
    runtime.stop_proxy().await.unwrap();
}

#[tokio::test]
async fn replay_body_ref_reuses_stored_capture_body() {
    let (_app_data, runtime, _proxy_port) = runtime().await;
    let session = runtime.create_session(None, None).unwrap();
    let (upstream_port, _upstream) = fixed_http_upstream().await;
    let first_id = runtime
        .replay(
            session.id,
            ReplayRequest {
                method: "POST".into(),
                url: format!("http://127.0.0.1:{upstream_port}/first"),
                headers: vec![("content-type".into(), "application/octet-stream".into())],
                body: Some(ReplayBody::Text { text: "stored-body".into() }),
            },
        )
        .await
        .unwrap();
    wait_capture_success(&runtime, session.id, first_id).await;

    let second_id = runtime
        .replay(
            session.id,
            ReplayRequest {
                method: "POST".into(),
                url: format!("http://127.0.0.1:{upstream_port}/second"),
                headers: vec![("content-type".into(), "application/octet-stream".into())],
                body: Some(ReplayBody::BodyRef {
                    session_id: session.id,
                    log_id: first_id,
                    side: BodySide::Request,
                }),
            },
        )
        .await
        .unwrap();
    wait_capture_success(&runtime, session.id, second_id).await;

    let source = runtime
        .capture_body_source(session.id, second_id, BodySide::Request)
        .unwrap()
        .unwrap();
    let bytes = match source.data {
        BodySourceData::Bytes(bytes) => bytes,
        BodySourceData::File(path) => std::fs::read(path).unwrap(),
    };
    assert_eq!(bytes, b"stored-body");
    runtime.stop_proxy().await.unwrap();
}

#[tokio::test]
async fn replay_requires_running_proxy_and_live_session() {
    let (_app_data, runtime, _proxy_port) = runtime().await;
    let session = runtime.create_session(None, None).unwrap();
    let archived = runtime.create_session(None, None).unwrap();
    runtime.archive_session(archived.id).await.unwrap();
    runtime.stop_proxy().await.unwrap();

    let request = ReplayRequest {
        method: "GET".into(),
        url: "http://127.0.0.1:1/never".into(),
        headers: Vec::new(),
        body: None,
    };
    assert!(matches!(
        runtime.replay(session.id, request.clone()).await,
        Err(ReplayError::ProxyNotRunning)
    ));
    assert!(matches!(
        runtime.replay(archived.id, request).await,
        Err(ReplayError::SessionNotFound)
    ));
}
```

（`SessionNotFound` 断言依赖 Task 2 中 session 校验先于 proxy 校验的顺序；`ScriptKind::RequestInterceptor` 的确切枚举名以 model.rs 现状为准。）

- [ ] **Step 2: 跑新测试**

Run: `cargo test -p proxy-crab-mitm --test proxy_integration replay`
Expected: 4 个新测试全部通过

- [ ] **Step 3: 全量回归 + Commit**

Run: `cargo test -p proxy-crab-mitm`
Expected: 全绿

```bash
git add crates/proxy-crab-mitm/tests/proxy_integration.rs
git commit -m "test(mitm): cover replay capture source, interceptors, body_ref, and guards"
```

---

### Task 4: Asset 列表能力（`AssetStore::list` + runtime 暴露）

**Files:**
- Modify: `crates/proxy-crab-mitm/src/asset.rs`
- Modify: `crates/proxy-crab-mitm/src/runtime.rs`

- [ ] **Step 1: `asset.rs` 增加 `list`（以 .metadata 目录为准递归遍历）**

```rust
    /// 列出全部 asset 元数据（以 .metadata 目录为准），按 id 排序。
    pub fn list(&self) -> Result<Vec<AssetMetadata>, AssetError> {
        fn walk(dir: &Path, out: &mut Vec<AssetMetadata>) -> Result<(), AssetError> {
            let entries = match std::fs::read_dir(dir) {
                Ok(entries) => entries,
                Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
                Err(error) => return Err(error.into()),
            };
            for entry in entries {
                let path = entry?.path();
                if path.is_dir() {
                    walk(&path, out)?;
                } else {
                    let bytes = std::fs::read(&path)?;
                    out.push(serde_json::from_slice(&bytes)?);
                }
            }
            Ok(())
        }
        let mut result = Vec::new();
        walk(&self.metadata, &mut result)?;
        result.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(result)
    }
```

- [ ] **Step 2: `runtime.rs` 暴露 `pub fn assets(&self) -> Result<Vec<AssetMetadata>, AssetError>`**（在 `asset()` 方法旁，委托 asset store 的 `list()`；store 字段名以 runtime.rs 现状为准）。

- [ ] **Step 3: 单元测试（`asset.rs` 测试模块）**：上传 `a/b.txt`、`c.json` 两个 asset 后 `list()` 返回两条且按 id 排序、字段与上传 metadata 一致；空工作区返回空 vec。

Run: `cargo test -p proxy-crab-mitm asset`
Expected: 通过

- [ ] **Step 4: Commit**

```bash
git add crates/proxy-crab-mitm/src/asset.rs crates/proxy-crab-mitm/src/runtime.rs
git commit -m "feat(mitm): list workspace asset metadata"
```

---

### Task 5: mgr 层 DTO + Manager::replay/assets

**Files:**
- Modify: `crates/proxy-crab-mgr/src/dto.rs`
- Modify: `crates/proxy-crab-mgr/src/manager.rs`

- [ ] **Step 1: `dto.rs` 增加类型**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayRequestPayload {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub body: Option<ReplayBodyPayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReplayBodyPayload {
    Text {
        text: String,
        #[serde(default)]
        charset: Option<String>,
    },
    BodyRef {
        session_id: u64,
        log_id: u64,
        side: String,
    },
    Asset {
        asset_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayResult {
    pub log_id: u64,
}
```

- [ ] **Step 2: trait 增加方法**

```rust
    async fn replay(
        &self,
        session_id: u64,
        request: ReplayRequestPayload,
    ) -> ManagerResult<ReplayResult>;
    async fn assets(&self) -> ManagerResult<Vec<proxy_crab_mitm::asset::AssetMetadata>>;
```

- [ ] **Step 3: 实现（`MitmManager`）**

```rust
    async fn replay(
        &self,
        session_id: u64,
        request: ReplayRequestPayload,
    ) -> ManagerResult<ReplayResult> {
        let body = match request.body {
            None => None,
            Some(ReplayBodyPayload::Text { text, charset }) => match charset.as_deref() {
                None | Some("utf8") => Some(ReplayBody::Text { text }),
                Some(other) => {
                    return Err(ManagerError::bad_request(format!(
                        "unsupported charset: {other}"
                    )));
                }
            },
            Some(ReplayBodyPayload::BodyRef { session_id, log_id, side }) => {
                let side = match side.as_str() {
                    "request" => BodySide::Request,
                    "response" => BodySide::Response,
                    other => {
                        return Err(ManagerError::bad_request(format!(
                            "invalid body side: {other}"
                        )));
                    }
                };
                Some(ReplayBody::BodyRef { session_id, log_id, side })
            }
            Some(ReplayBodyPayload::Asset { asset_id }) => Some(ReplayBody::Asset { asset_id }),
        };
        let log_id = self
            .runtime
            .replay(session_id, ReplayRequest {
                method: request.method,
                url: request.url,
                headers: request.headers,
                body,
            })
            .await
            .map_err(|error| match error {
                ReplayError::SessionNotFound => {
                    ManagerError::not_found(format!("session {session_id} not found or archived"))
                }
                ReplayError::ProxyNotRunning => {
                    ManagerError::new("proxy_not_running", "proxy is not running")
                }
                ReplayError::BodyNotFound => {
                    ManagerError::not_found("referenced capture body not found")
                }
                ReplayError::AssetNotFound => ManagerError::not_found("referenced asset not found"),
                ReplayError::Invalid(message) => ManagerError::bad_request(message),
                ReplayError::Internal(error) => {
                    ManagerError::new("replay_failed", error.to_string())
                }
            })?;
        Ok(ReplayResult { log_id })
    }

    async fn assets(&self) -> ManagerResult<Vec<proxy_crab_mitm::asset::AssetMetadata>> {
        self.runtime.assets().map_err(map_error)
    }
```

import 增加 `proxy_crab_mitm::{ReplayBody, ReplayError, ReplayRequest}` 与 dto 新类型。

- [ ] **Step 4: 编译 + Commit**

Run: `cargo check -p proxy-crab-mgr`
Expected: 通过

```bash
git add crates/proxy-crab-mgr/src/dto.rs crates/proxy-crab-mgr/src/manager.rs
git commit -m "feat(mgr): replay and asset-list manager methods"
```

---

### Task 6: HTTP 路由 + 权限注册

**Files:**
- Modify: `crates/proxy-crab-mgr/src/http.rs`
- Modify: `crates/proxy-crab-mgr/src/permission.rs`
- Modify: `crates/proxy-crab-mgr/tests/permission_contract.rs`

- [ ] **Step 1: 注册路由（`router_with_changes_and_extra` 中 `.route("/api/assets/{*asset_id}", ...)` 附近）**

```rust
        .route("/api/assets", get(assets))
        .route("/api/replay", post(replay))
```

- [ ] **Step 2: handler**

```rust
#[derive(Debug, serde::Deserialize)]
struct ReplayQuery {
    session: u64,
}

async fn assets(State(manager): State<ManagerState>) -> ApiResult {
    success(manager.assets().await?)
}

async fn replay(
    State(manager): State<ManagerState>,
    ApiQuery(query): ApiQuery<ReplayQuery>,
    Json(request): Json<ReplayRequestPayload>,
) -> ApiResult {
    success(manager.replay(query.session, request).await?)
}
```

- [ ] **Step 3: 权限目录 + session 归属识别**

`permission.rs` 的 `API_ACTIONS` 添加：

```rust
    action!("GET", "/api/assets", Allow),
    action!("POST", "/api/replay", Allow),
```

`http.rs` 的 `session_id_from_query` 同时识别 `session`：

```rust
        .find_map(|(key, value)| {
            (key == "session_id" || key == "session")
                .then(|| value.parse().ok())
                .flatten()
        })
```

- [ ] **Step 4: 更新权限契约测试**：`assert_eq!(actions.len(), 69)` → `71`；allow 矩阵按字母序加入 `"GET /api/assets"` 与 `"POST /api/replay"`。

Run: `cargo test -p proxy-crab-mgr --test permission_contract`
Expected: 通过

- [ ] **Step 5: http.rs 路由测试（复用 `MitmManager` / `allow_all` / `oneshot` 模式）**

```rust
    #[tokio::test]
    async fn replay_route_validates_input_and_reports_guards() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let session = runtime.create_session(None, None).unwrap();
        let app = router(MitmManager::new(runtime), allow_all());

        // 缺少 ?session= → 4xx
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/replay")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"method":"GET","url":"http://127.0.0.1:1/x"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(response.status().is_client_error());

        // 代理未运行 → proxy_not_running
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/api/replay?session={}", session.id))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"method":"GET","url":"http://127.0.0.1:1/x"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["error"]["code"], "proxy_not_running");

        // 不支持的 charset → bad_request
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/api/replay?session={}", session.id))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"method":"GET","url":"http://127.0.0.1:1/x","body":{"type":"text","text":"a","charset":"gbk"}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["error"]["code"], "bad_request");
    }

    #[tokio::test]
    async fn assets_route_lists_metadata() {
        let app_data = tempdir().unwrap();
        let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
        let mut upload = runtime
            .begin_asset_upload("fixtures/a.json".into(), "application/json".into())
            .await
            .unwrap();
        upload.write(b"{}").await.unwrap();
        upload.finish().await.unwrap();
        let app = router(MitmManager::new(runtime), allow_all());

        let response = app
            .oneshot(Request::builder().uri("/api/assets").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["data"][0]["id"], "fixtures/a.json");
        assert_eq!(body["data"][0]["content_type"], "application/json");
    }
```

（`AssetUpload::write/finish` 调用形式以现状签名为准微调；若 `finish` 消费 self 则去掉 `mut`。）

Run: `cargo test -p proxy-crab-mgr`
Expected: 全绿；若有路由枚举类测试涉及路由清单，按新路由同步更新

- [ ] **Step 6: Commit**

```bash
git add crates/proxy-crab-mgr/src/http.rs crates/proxy-crab-mgr/src/permission.rs crates/proxy-crab-mgr/tests/permission_contract.rs
git commit -m "feat(mgr): expose POST /api/replay and GET /api/assets with permissions"
```

---

### Task 7: Tauri 命令

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: 新增命令（仿照 `get_log`）**

```rust
#[tauri::command]
async fn replay(
    state: State<'_, BackendState>,
    session_id: u64,
    request: ReplayRequestPayload,
) -> Result<ReplayResult, ManagerError> {
    state.manager().replay(session_id, request).await
}

#[tauri::command]
async fn list_assets(
    state: State<'_, BackendState>,
) -> Result<Vec<proxy_crab_mgr::asset::AssetMetadata>, ManagerError> {
    state.manager().assets().await
}
```

import 增加 dto 的 `ReplayRequestPayload, ReplayResult`；若 asset 类型路径不可达，在 mgr `lib.rs` re-export（`pub use proxy_crab_mitm::asset;`）。

- [ ] **Step 2: 注册到 invoke_handler**（`get_interceptor_snapshot,` 附近添加 `replay,` 与 `list_assets,`）。

- [ ] **Step 3: 编译 + Commit**

Run: `cargo check`（workspace 根，覆盖 src-tauri crate）
Expected: 通过

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(tauri): replay and list_assets commands"
```

---

### Task 8: 前端 API 层

**Files:**
- Modify: `src/api/types.ts`
- Modify: `src/api/backend.ts`
- Modify: `src/api/http-backend.ts`
- Modify: `src/api/tauri-backend.ts`

（`share-backend.ts` 基于 Proxy 的 `unsupported()` 兜底，新接口自动表现为不支持，无需修改。）

- [ ] **Step 1: `types.ts` 增加**

```ts
export type ReplayBodyPayload =
  | { type: "text"; text: string; charset?: "utf8" }
  | {
      type: "body_ref";
      session_id: number;
      log_id: number;
      side: "request" | "response";
    }
  | { type: "asset"; asset_id: string };

export interface ReplayRequestPayload {
  method: string;
  url: string;
  headers: Array<[string, string]>;
  body?: ReplayBodyPayload;
}

export interface ReplayResult {
  log_id: number;
}

export interface AssetMetadata {
  id: string;
  size: number;
  content_type: string;
  sha256: string;
  created_at: number;
}
```

- [ ] **Step 2: `backend.ts` 接口增加（logs 区附近）并 import 新类型**

```ts
  replay(sessionId: number, request: ReplayRequestPayload): Promise<ReplayResult>;
  listAssets(): Promise<AssetMetadata[]>;
```

- [ ] **Step 3: `http-backend.ts` 实现（复用 `json()` helper）**

```ts
    replay: (sessionId: number, request: ReplayRequestPayload) =>
      call<ReplayResult>(`/api/replay?session=${sessionId}`, json("POST", request)),
    listAssets: () => call<AssetMetadata[]>("/api/assets"),
```

- [ ] **Step 4: `tauri-backend.ts` 实现**

```ts
    replay: (sessionId: number, request: ReplayRequestPayload) =>
      call<ReplayResult>("replay", { sessionId, request }),
    listAssets: () => call<AssetMetadata[]>("list_assets"),
```

- [ ] **Step 5: 类型检查 + Commit**

Run: `pnpm exec vue-tsc --noEmit`
Expected: 通过（若 Backend 其它实现处报错，按同样模式补齐）

```bash
git add src/api/types.ts src/api/backend.ts src/api/http-backend.ts src/api/tauri-backend.ts
git commit -m "feat(ui): replay and asset-list backend bindings"
```

---
### Task 9: 前端纯逻辑 `src/utils/replay.ts` + 单测

**Files:**
- Create: `src/utils/replay.ts`
- Create: `src/utils/replay.test.mts`

- [ ] **Step 1: 写实现**

```ts
// 重放草稿与预填、headers 表格规范化、header 警告的纯逻辑。

import type {
  BodySourceType,
  InterceptorSnapshotPayload,
  LogDetail,
} from "../api/types";

export type ReplayBodySpec =
  | { type: "text"; text: string; charset: "utf8" }
  | {
      type: "body_ref";
      session_id: number;
      log_id: number;
      side: "request" | "response";
    }
  | { type: "asset"; asset_id: string };

export interface ReplayDraft {
  method: string;
  url: string;
  headers: Array<[string, string]>;
  body: ReplayBodySpec | null;
}

export const REPLAY_METHODS = [
  "GET",
  "POST",
  "PUT",
  "DELETE",
  "PATCH",
  "HEAD",
  "OPTIONS",
] as const;

/** 日志详情页预填：最终值；body 非空时用 body_ref 指向该日志的请求 body。 */
export function prefillFromLog(detail: LogDetail): ReplayDraft {
  return {
    method: detail.request.method,
    url: detail.request.uri,
    headers: detail.request.headers.map((h) => [h.name, h.value]),
    body:
      detail.request.body.type === "empty"
        ? null
        : {
            type: "body_ref",
            session_id: detail.session_id,
            log_id: detail.id,
            side: "request",
          },
  };
}

/** 快照页预填：仅请求侧快照可用；string→text，asset→asset，original→body_ref。 */
export function prefillFromSnapshot(
  sessionId: number,
  logId: number,
  snapshot: InterceptorSnapshotPayload,
): ReplayDraft | null {
  const request = snapshot.request;
  if (!request) return null;
  return {
    method: request.method,
    url: request.uri,
    headers: Object.entries(request.headers).flatMap(([name, values]) =>
      values.map((value) => [name, value] as [string, string]),
    ),
    body: bodySpecFromSource(sessionId, logId, "request", request.body),
  };
}

function bodySpecFromSource(
  sessionId: number,
  logId: number,
  side: "request" | "response",
  source: BodySourceType,
): ReplayBodySpec | null {
  switch (source.type) {
    case "original":
      return { type: "body_ref", session_id: sessionId, log_id: logId, side };
    case "string":
      return { type: "text", text: source.content, charset: "utf8" };
    case "asset":
      return { type: "asset", asset_id: source.asset_id };
  }
}

export interface KvRow {
  name: string;
  value: string;
}

/** 规范化：删除中间全空行，末尾保留恰好一个全空行。 */
export function normalizeKvRows(rows: KvRow[]): KvRow[] {
  const cleaned = rows.filter((row) => row.name !== "" || row.value !== "");
  cleaned.push({ name: "", value: "" });
  return cleaned;
}

/** 提交用：去掉全空行，转二元组。 */
export function kvRowsToTuples(rows: KvRow[]): Array<[string, string]> {
  return rows
    .filter((row) => row.name !== "" || row.value !== "")
    .map((row) => [row.name, row.value]);
}

export function headerValue(
  headers: Array<[string, string]>,
  name: string,
): string | null {
  const found = headers.find(
    ([key]) => key.toLowerCase() === name.toLowerCase(),
  );
  return found ? found[1] : null;
}

/** body 字节数已知时校验 Content-Length。 */
export function contentLengthIssue(
  headers: Array<[string, string]>,
  bodySize: number | null,
): "missing" | "mismatch" | null {
  if (bodySize === null || bodySize === 0) return null;
  const value = headerValue(headers, "content-length");
  if (value === null) return "missing";
  return Number(value) === bodySize ? null : "mismatch";
}

/** 按文本内容猜测 Content-Type。 */
export function guessContentType(text: string): string {
  const trimmed = text.trim();
  if (trimmed === "") return "application/octet-stream";
  try {
    JSON.parse(trimmed);
    return "application/json";
  } catch {
    return "text/plain";
  }
}
```

- [ ] **Step 2: 写单测 `src/utils/replay.test.mts`**

```ts
import { describe, expect, it } from "vitest";
import {
  contentLengthIssue,
  guessContentType,
  kvRowsToTuples,
  normalizeKvRows,
  prefillFromLog,
  prefillFromSnapshot,
} from "./replay";

describe("normalizeKvRows", () => {
  it("removes middle empty rows and keeps exactly one trailing empty row", () => {
    expect(
      normalizeKvRows([
        { name: "a", value: "1" },
        { name: "", value: "" },
        { name: "b", value: "2" },
        { name: "", value: "" },
      ]),
    ).toEqual([
      { name: "a", value: "1" },
      { name: "b", value: "2" },
      { name: "", value: "" },
    ]);
  });
});

describe("kvRowsToTuples", () => {
  it("drops empty rows", () => {
    expect(
      kvRowsToTuples([
        { name: "a", value: "1" },
        { name: "", value: "" },
      ]),
    ).toEqual([["a", "1"]]);
  });
});

describe("prefillFromLog", () => {
  const detail = {
    id: 7,
    session_id: 3,
    request: {
      method: "POST",
      uri: "http://x.test/a?b=1",
      headers: [{ name: "content-type", value: "text/plain" }],
      body: { type: "text", content: "hi", size: 2, path: null },
    },
  } as never;
  it("uses final values and body_ref for non-empty bodies", () => {
    expect(prefillFromLog(detail)).toEqual({
      method: "POST",
      url: "http://x.test/a?b=1",
      headers: [["content-type", "text/plain"]],
      body: { type: "body_ref", session_id: 3, log_id: 7, side: "request" },
    });
  });
});

describe("prefillFromSnapshot", () => {
  it("returns null without a request snapshot", () => {
    expect(
      prefillFromSnapshot(1, 2, { request: null, response: null } as never),
    ).toBeNull();
  });
  it("maps string bodies to text mode", () => {
    const draft = prefillFromSnapshot(1, 2, {
      request: {
        method: "PUT",
        uri: "http://x.test/p",
        version: "HTTP/1.1",
        headers: { a: ["1", "2"] },
        body: { type: "string", content: "v" },
      },
      response: null,
    } as never);
    expect(draft).toEqual({
      method: "PUT",
      url: "http://x.test/p",
      headers: [
        ["a", "1"],
        ["a", "2"],
      ],
      body: { type: "text", text: "v", charset: "utf8" },
    });
  });
});

describe("contentLengthIssue", () => {
  it("detects missing and mismatch, case-insensitively", () => {
    expect(contentLengthIssue([], 5)).toBe("missing");
    expect(contentLengthIssue([["Content-Length", "4"]], 5)).toBe("mismatch");
    expect(contentLengthIssue([["content-length", "5"]], 5)).toBeNull();
    expect(contentLengthIssue([], 0)).toBeNull();
    expect(contentLengthIssue([], null)).toBeNull();
  });
});

describe("guessContentType", () => {
  it("guesses json / text / octet-stream", () => {
    expect(guessContentType('{"a":1}')).toBe("application/json");
    expect(guessContentType("hello")).toBe("text/plain");
    expect(guessContentType("   ")).toBe("application/octet-stream");
  });
});
```

- [ ] **Step 3: 跑单测 + Commit**

Run: `pnpm test:unit`
Expected: 全部通过

```bash
git add src/utils/replay.ts src/utils/replay.test.mts
git commit -m "feat(ui): replay draft/prefill and header-warning logic"
```

---

### Task 10: `EditableKvTable.vue` 组件

**Files:**
- Create: `src/components/EditableKvTable.vue`

- [ ] **Step 1: 实现（`v-model` 为不含末尾空行的 `KvRow[]`；内部展示态恒含一个末尾空行）**

```vue
<script setup lang="ts">
import { ref, watch } from "vue";
import { Io5ReorderTwo } from "vue-icons-plus/io5";
import { normalizeKvRows, type KvRow } from "../utils/replay";

const props = defineProps<{
  nameLabel?: string;
  valueLabel?: string;
}>();

const rows = defineModel<KvRow[]>({ required: true });

const nonEmpty = (list: KvRow[]) =>
  list.filter((row) => row.name !== "" || row.value !== "");

const display = ref<KvRow[]>(normalizeKvRows(rows.value));
watch(rows, (value) => {
  if (JSON.stringify(nonEmpty(value)) !== JSON.stringify(nonEmpty(display.value))) {
    display.value = normalizeKvRows(value);
  }
});

function commit(): void {
  display.value = normalizeKvRows(display.value);
  rows.value = nonEmpty(display.value);
}

const dragIndex = ref<number | null>(null);

function draggable(index: number): boolean {
  const row = display.value[index];
  return row.name !== "" || row.value !== "";
}

function onDrop(index: number): void {
  const from = dragIndex.value;
  dragIndex.value = null;
  if (from === null || from === index) return;
  const next = [...display.value];
  const [moved] = next.splice(from, 1);
  next.splice(index, 0, moved);
  display.value = next;
  commit();
}
</script>

<template>
  <table class="kv-edit-table">
    <thead>
      <tr>
        <th class="kv-drag-col" />
        <th>{{ props.nameLabel ?? "Name" }}</th>
        <th>{{ props.valueLabel ?? "Value" }}</th>
      </tr>
    </thead>
    <tbody>
      <tr
        v-for="(row, index) in display"
        :key="index"
        :draggable="draggable(index)"
        :class="{ dragging: dragIndex === index }"
        @dragstart="dragIndex = index"
        @dragend="dragIndex = null"
        @dragover.prevent
        @drop.prevent="onDrop(index)"
      >
        <td class="kv-drag-col">
          <Io5ReorderTwo v-if="draggable(index)" :size="14" class="text-faint" />
        </td>
        <td>
          <input
            v-model="row.name"
            class="input kv-cell mono"
            placeholder="header"
            spellcheck="false"
            @input="commit"
          />
        </td>
        <td>
          <input
            v-model="row.value"
            class="input kv-cell mono"
            placeholder="value"
            spellcheck="false"
            @input="commit"
          />
        </td>
      </tr>
    </tbody>
  </table>
</template>

<style scoped>
.kv-edit-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}
.kv-edit-table th {
  text-align: left;
  font-weight: 500;
  color: var(--text-secondary);
  padding: 2px 6px;
}
.kv-drag-col {
  width: 20px;
  text-align: center;
}
.kv-edit-table td {
  padding: 1px 2px;
}
.kv-cell {
  width: 100%;
  border-color: transparent;
  background: transparent;
}
tr[draggable="true"] {
  cursor: grab;
}
tr.dragging {
  opacity: 0.5;
}
</style>
```

（CSS 变量名以 `src/styles` 现有主题变量为准微调。）

- [ ] **Step 2: 类型检查 + Commit**

Run: `pnpm exec vue-tsc --noEmit`
Expected: 通过

```bash
git add src/components/EditableKvTable.vue
git commit -m "feat(ui): editable key-value table with drag sorting"
```

---

### Task 11: `UrlInput.vue` 组件（分段高亮输入）

**Files:**
- Create: `src/components/UrlInput.vue`

- [ ] **Step 1: 实现（透明输入框叠在着色高亮层上，复用 `parseUrlSegments`）**

```vue
<script setup lang="ts">
import { computed } from "vue";
import { parseUrlSegments, type UrlSegment } from "../utils/url-segments";

const model = defineModel<string>({ required: true });

const segments = computed<UrlSegment[]>(
  () => parseUrlSegments(model.value) ?? [{ text: model.value, cls: "" }],
);
</script>

<template>
  <div class="url-input">
    <div class="url-input-highlight mono" aria-hidden="true">
      <span v-for="(seg, i) in segments" :key="i" :class="seg.cls">{{
        seg.text
      }}</span>
    </div>
    <input
      v-model="model"
      class="url-input-field mono"
      spellcheck="false"
      placeholder="https://example.com/path?query=1"
    />
  </div>
</template>

<style scoped>
.url-input {
  position: relative;
  flex: 1;
  min-width: 0;
}
.url-input-highlight,
.url-input-field {
  font-size: 12px;
  line-height: 22px;
  padding: 0 8px;
  white-space: pre;
}
.url-input-highlight {
  position: absolute;
  inset: 0;
  overflow: hidden;
  pointer-events: none;
  border: 1px solid transparent;
}
.url-input-field {
  width: 100%;
  color: transparent;
  caret-color: var(--text);
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: 4px;
  outline: none;
}
.url-input-field:focus {
  border-color: var(--accent);
}
.url-input-field::placeholder {
  color: var(--text-faint);
}
</style>
```

注意：高亮层与输入框的字体/字号/行高/内边距必须完全一致才能对齐；`.mono` 类若带 letter-spacing 需两层一致（两层都不加 `mono`，直接在本组件设定同一 monospace 字体栈）。

- [ ] **Step 2: 类型检查 + Commit**

Run: `pnpm exec vue-tsc --noEmit`
Expected: 通过

```bash
git add src/components/UrlInput.vue
git commit -m "feat(ui): segmented-highlight URL input"
```

---

### Task 12: `AssetTreePicker.vue` 组件

**Files:**
- Create: `src/components/AssetTreePicker.vue`

- [ ] **Step 1: 实现（按 asset id 的 `/` 分段建树；扁平化渲染带 depth 的可见节点；文件夹可折叠，文件可选）**

```vue
<script setup lang="ts">
import { computed, ref } from "vue";
import {
  Io5ChevronDown,
  Io5ChevronForward,
  Io5DocumentOutline,
  Io5FolderOutline,
} from "vue-icons-plus/io5";
import type { AssetMetadata } from "../api/types";

const props = defineProps<{ assets: AssetMetadata[] }>();
const selected = defineModel<string | null>({ required: true });

interface AssetNode {
  name: string;
  path: string;
  depth: number;
  children: AssetNode[];
  asset: AssetMetadata | null;
}

function buildTree(assets: AssetMetadata[]): AssetNode[] {
  const root: AssetNode = {
    name: "",
    path: "",
    depth: -1,
    children: [],
    asset: null,
  };
  for (const asset of assets) {
    const parts = asset.id.split("/");
    let node = root;
    parts.forEach((part, index) => {
      const path = parts.slice(0, index + 1).join("/");
      let child = node.children.find((item) => item.name === part);
      if (!child) {
        child = { name: part, path, depth: index, children: [], asset: null };
        node.children.push(child);
      }
      if (index === parts.length - 1) child.asset = asset;
      node = child;
    });
  }
  const sort = (nodes: AssetNode[]) => {
    nodes.sort((a, b) =>
      a.asset === null && b.asset !== null
        ? -1
        : a.asset !== null && b.asset === null
          ? 1
          : a.name.localeCompare(b.name),
    );
    nodes.forEach((node) => sort(node.children));
  };
  sort(root.children);
  return root.children;
}

const tree = computed(() => buildTree(props.assets));
const collapsed = ref(new Set<string>());

const visibleNodes = computed(() => {
  const result: AssetNode[] = [];
  const walk = (nodes: AssetNode[]) => {
    for (const node of nodes) {
      result.push(node);
      if (node.asset === null && !collapsed.value.has(node.path)) walk(node.children);
    }
  };
  walk(tree.value);
  return result;
});

function toggle(path: string): void {
  const next = new Set(collapsed.value);
  if (next.has(path)) next.delete(path);
  else next.add(path);
  collapsed.value = next;
}

function formatSize(size: number): string {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${(size / 1024 / 1024).toFixed(1)} MB`;
}
</script>

<template>
  <div class="asset-tree">
    <div v-if="visibleNodes.length === 0" class="empty-hint">工作区暂无资源</div>
    <button
      v-for="node in visibleNodes"
      :key="node.path"
      class="asset-row mono"
      :class="{ active: node.asset !== null && selected === node.asset.id }"
      :style="{ paddingLeft: `${8 + node.depth * 14}px` }"
      :title="node.asset ? `${node.asset.content_type} · ${formatSize(node.asset.size)}` : node.path"
      @click="node.asset === null ? toggle(node.path) : (selected = node.asset.id)"
    >
      <template v-if="node.asset === null">
        <Io5ChevronDown v-if="!collapsed.has(node.path)" :size="12" />
        <Io5ChevronForward v-else :size="12" />
        <Io5FolderOutline :size="13" />
      </template>
      <Io5DocumentOutline v-else :size="13" />
      <span class="asset-name">{{ node.name }}</span>
      <span v-if="node.asset" class="asset-size text-faint">{{
        formatSize(node.asset.size)
      }}</span>
    </button>
  </div>
</template>

<style scoped>
.asset-tree {
  overflow: auto;
  font-size: 12px;
}
.asset-row {
  display: flex;
  align-items: center;
  gap: 4px;
  width: 100%;
  height: 22px;
  border: none;
  background: transparent;
  color: var(--text);
  cursor: pointer;
  text-align: left;
}
.asset-row:hover {
  background: var(--bg-hover);
}
.asset-row.active {
  background: var(--accent-bg);
}
.asset-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.asset-size {
  margin-left: auto;
  padding-right: 6px;
}
.empty-hint {
  color: var(--text-faint);
  padding: 8px;
}
</style>
```

（CSS 变量名以现有主题为准微调。）

- [ ] **Step 2: 类型检查 + Commit**

Run: `pnpm exec vue-tsc --noEmit`
Expected: 通过

```bash
git add src/components/AssetTreePicker.vue
git commit -m "feat(ui): workspace asset tree picker"
```

---

### Task 13: `ReplayWindow.vue` 主窗口 + launcher

**Files:**
- Create: `src/windows/ReplayWindow.vue`
- Modify: `src/windows/launcher.ts`

- [ ] **Step 1: launcher 增加 `openReplay`（随机 UID，允许多开）**

```ts
export function openReplay(prefill?: ReplayDraft): void {
  windowsStore.open(`replay-${crypto.randomUUID()}`, {
    title: "发送请求",
    component: ReplayWindow,
    props: { prefill },
    width: 920,
    height: 640,
  });
}
```

（import `ReplayWindow` 与 `type ReplayDraft` from `../utils/replay`。）

- [ ] **Step 2: 实现 `ReplayWindow.vue` 的 script**

```vue
<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { Io5OpenOutline, Io5Play, Io5Warning } from "vue-icons-plus/io5";
import { useBackend } from "../api";
import type { AssetMetadata, SessionMetadata } from "../api/types";
import CustomSelect from "../components/CustomSelect.vue";
import MonacoEditor from "../components/MonacoEditor.vue";
import EditableKvTable from "../components/EditableKvTable.vue";
import UrlInput from "../components/UrlInput.vue";
import AssetTreePicker from "../components/AssetTreePicker.vue";
import {
  REPLAY_METHODS,
  contentLengthIssue,
  guessContentType,
  headerValue,
  kvRowsToTuples,
  type KvRow,
  type ReplayBodySpec,
  type ReplayDraft,
} from "../utils/replay";
import { openLogDetail } from "./launcher";

const props = defineProps<{ prefill?: ReplayDraft }>();
const backend = useBackend();

// ---------- 表单状态 ----------
const CUSTOM_METHOD = "__custom";
const methodChoice = ref<string>("GET");
const customMethod = ref("");
const url = ref("");
const rows = ref<KvRow[]>([]);
type BodyMode = "none" | "text" | "body_ref" | "asset";
const bodyMode = ref<BodyMode>("none");
const bodyText = ref("");
const bodyRefSession = ref<number | null>(null);
const bodyRefLog = ref<number | null>(null);
const bodyRefSide = ref<"request" | "response">("request");
const assetId = ref<string | null>(null);

if (props.prefill) {
  const p = props.prefill;
  if ((REPLAY_METHODS as readonly string[]).includes(p.method)) {
    methodChoice.value = p.method;
  } else {
    methodChoice.value = CUSTOM_METHOD;
    customMethod.value = p.method;
  }
  url.value = p.url;
  rows.value = p.headers.map(([name, value]) => ({ name, value }));
  if (p.body?.type === "text") {
    bodyMode.value = "text";
    bodyText.value = p.body.text;
  } else if (p.body?.type === "body_ref") {
    bodyMode.value = "body_ref";
    bodyRefSession.value = p.body.session_id;
    bodyRefLog.value = p.body.log_id;
    bodyRefSide.value = p.body.side;
  } else if (p.body?.type === "asset") {
    bodyMode.value = "asset";
    assetId.value = p.body.asset_id;
  }
}

const methodOptions = [
  ...REPLAY_METHODS.map((value) => ({ value, label: value })),
  { value: CUSTOM_METHOD, label: "自定义…" },
];
const method = computed(() =>
  methodChoice.value === CUSTOM_METHOD
    ? customMethod.value.trim().toUpperCase()
    : methodChoice.value,
);

// ---------- 会话选择 ----------
const sessions = ref<SessionMetadata[]>([]);
const sessionId = ref<string | null>(null);
onMounted(async () => {
  const [list, active] = await Promise.all([
    backend.listSessions(),
    backend.getActiveSession(),
  ]);
  sessions.value = list;
  sessionId.value =
    active.session_id !== null &&
    list.some((session) => session.id === active.session_id)
      ? String(active.session_id)
      : list[0]
        ? String(list[0].id)
        : null;
});
const sessionOptions = computed(() =>
  sessions.value.map((session) => ({
    value: String(session.id),
    label: `#${session.id} ${session.name ?? "未命名"}`,
  })),
);

// ---------- 资源 ----------
const assets = ref<AssetMetadata[]>([]);
let assetsLoaded = false;
async function ensureAssets(): Promise<void> {
  if (assetsLoaded) return;
  assetsLoaded = true;
  assets.value = await backend.listAssets();
}
function selectBodyMode(mode: BodyMode): void {
  bodyMode.value = mode;
  if (mode === "asset") void ensureAssets();
}
if (bodyMode.value === "asset") void ensureAssets();

// ---------- 警告 ----------
const tuples = computed(() => kvRowsToTuples(rows.value));
const knownBodySize = computed<number | null>(() => {
  if (bodyMode.value === "text")
    return new TextEncoder().encode(bodyText.value).length;
  if (bodyMode.value === "asset" && assetId.value) {
    const meta = assets.value.find((item) => item.id === assetId.value);
    return meta ? meta.size : null;
  }
  return null; // body_ref / none：长度未知或无 body
});
const hasBody = computed(
  () =>
    (bodyMode.value === "text" && bodyText.value.length > 0) ||
    (bodyMode.value === "asset" && assetId.value !== null) ||
    (bodyMode.value === "body_ref" &&
      bodyRefSession.value !== null &&
      bodyRefLog.value !== null),
);
const lengthIssue = computed(() =>
  contentLengthIssue(tuples.value, knownBodySize.value),
);
const missingContentType = computed(
  () => hasBody.value && headerValue(tuples.value, "content-type") === null,
);

function upsertHeader(name: string, value: string): void {
  const committed = rows.value.filter(
    (row) => row.name !== "" || row.value !== "",
  );
  const found = committed.find(
    (row) => row.name.toLowerCase() === name.toLowerCase(),
  );
  if (found) found.value = value;
  else committed.push({ name, value });
  rows.value = committed;
}
function fixContentLength(): void {
  if (knownBodySize.value !== null)
    upsertHeader("content-length", String(knownBodySize.value));
}
function fixContentType(): void {
  if (bodyMode.value === "text") {
    upsertHeader("content-type", guessContentType(bodyText.value));
  } else if (bodyMode.value === "asset" && assetId.value) {
    const meta = assets.value.find((item) => item.id === assetId.value);
    upsertHeader(
      "content-type",
      meta?.content_type || "application/octet-stream",
    );
  } else {
    upsertHeader("content-type", "application/octet-stream");
  }
}

// ---------- 执行 ----------
const executing = ref(false);
const resultLogId = ref<number | null>(null);
const executeError = ref<string | null>(null);

function buildBody(): ReplayBodySpec | undefined {
  if (bodyMode.value === "text" && bodyText.value.length > 0)
    return { type: "text", text: bodyText.value, charset: "utf8" };
  if (
    bodyMode.value === "body_ref" &&
    bodyRefSession.value !== null &&
    bodyRefLog.value !== null
  )
    return {
      type: "body_ref",
      session_id: bodyRefSession.value,
      log_id: bodyRefLog.value,
      side: bodyRefSide.value,
    };
  if (bodyMode.value === "asset" && assetId.value)
    return { type: "asset", asset_id: assetId.value };
  return undefined;
}

const canExecute = computed(
  () =>
    !executing.value &&
    sessionId.value !== null &&
    method.value !== "" &&
    /^https?:\/\//.test(url.value),
);

async function execute(): Promise<void> {
  if (!canExecute.value || sessionId.value === null) return;
  executing.value = true;
  resultLogId.value = null;
  executeError.value = null;
  try {
    const targetSession = Number(sessionId.value);
    const result = await backend.replay(targetSession, {
      method: method.value,
      url: url.value,
      headers: tuples.value,
      body: buildBody(),
    });
    resultLogId.value = result.log_id;
  } catch (error) {
    executeError.value = error instanceof Error ? error.message : String(error);
  } finally {
    executing.value = false;
  }
}

function openResult(): void {
  if (resultLogId.value !== null && sessionId.value !== null) {
    openLogDetail(Number(sessionId.value), resultLogId.value);
  }
}
</script>
```

- [ ] **Step 3: 模板与样式**

```vue
<template>
  <div class="replay-root">
    <div class="replay-bar">
      <CustomSelect
        :model-value="methodChoice"
        :options="methodOptions"
        aria-label="请求方法"
        @update:model-value="methodChoice = $event"
      />
      <input
        v-if="methodChoice === CUSTOM_METHOD"
        v-model="customMethod"
        class="input mono custom-method"
        placeholder="METHOD"
        spellcheck="false"
      />
      <UrlInput v-model="url" />
      <CustomSelect
        :model-value="sessionId"
        :options="sessionOptions"
        placeholder="选择会话"
        aria-label="目标会话"
        @update:model-value="sessionId = $event"
      />
      <button class="btn primary" :disabled="!canExecute" @click="execute">
        <Io5Play :size="13" /> {{ executing ? "发送中…" : "发送" }}
      </button>
    </div>

    <div v-if="lengthIssue || missingContentType" class="replay-warnings">
      <span v-if="lengthIssue" class="warning-item">
        <Io5Warning :size="13" class="text-warning" />
        {{
          lengthIssue === "missing"
            ? "缺少 Content-Length"
            : "Content-Length 与实际 body 大小不符"
        }}
        <button class="btn" @click="fixContentLength">
          {{ lengthIssue === "missing" ? "自动补上" : `修正为 ${knownBodySize}` }}
        </button>
      </span>
      <span v-if="missingContentType" class="warning-item">
        <Io5Warning :size="13" class="text-warning" />
        缺少 Content-Type
        <button class="btn" @click="fixContentType">按内容补齐</button>
      </span>
    </div>

    <div class="replay-section">
      <div class="section-title text-secondary">Headers</div>
      <EditableKvTable v-model="rows" name-label="Header" value-label="Value" />
    </div>

    <div class="replay-section replay-body-section">
      <div class="section-title text-secondary">
        Body
        <span class="body-mode-tabs">
          <button
            v-for="mode in (['none', 'text', 'body_ref', 'asset'] as const)"
            :key="mode"
            class="btn body-mode-tab"
            :class="{ primary: bodyMode === mode }"
            @click="selectBodyMode(mode)"
          >
            {{ { none: "无", text: "文本", body_ref: "引用日志", asset: "资源" }[mode] }}
          </button>
        </span>
      </div>
      <MonacoEditor
        v-if="bodyMode === 'text'"
        v-model="bodyText"
        language="plaintext"
      />
      <div v-else-if="bodyMode === 'body_ref'" class="body-ref-form">
        <input
          v-model.number="bodyRefSession"
          class="input mono body-ref-input"
          type="number"
          min="1"
          placeholder="Session ID"
        />
        <input
          v-model.number="bodyRefLog"
          class="input mono body-ref-input"
          type="number"
          min="1"
          placeholder="Log ID"
        />
        <CustomSelect
          :model-value="bodyRefSide"
          :options="[
            { value: 'request', label: '请求 body' },
            { value: 'response', label: '响应 body' },
          ]"
          aria-label="Body 方向"
          @update:model-value="bodyRefSide = $event as 'request' | 'response'"
        />
      </div>
      <AssetTreePicker
        v-else-if="bodyMode === 'asset'"
        v-model:selected="assetId"
        :assets="assets"
      />
      <div v-else class="empty-hint text-faint">无 Body</div>
    </div>

    <div v-if="resultLogId !== null || executeError" class="replay-result">
      <span v-if="resultLogId !== null" class="result-ok">
        已发送，日志 #{{ resultLogId }}
        <button class="btn" @click="openResult">
          <Io5OpenOutline :size="13" /> 查看详情
        </button>
      </span>
      <span v-else class="result-error text-error">{{ executeError }}</span>
    </div>
  </div>
</template>

<style scoped>
.replay-root {
  display: flex;
  flex-direction: column;
  gap: 8px;
  height: 100%;
  padding: 10px;
  overflow: auto;
}
.replay-bar {
  display: flex;
  align-items: center;
  gap: 6px;
}
.custom-method {
  width: 90px;
}
.replay-warnings {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  font-size: 12px;
}
.warning-item {
  display: inline-flex;
  align-items: center;
  gap: 4px;
}
.replay-section {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-height: 0;
}
.replay-body-section {
  flex: 1;
}
.section-title {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 12px;
}
.body-mode-tabs {
  display: inline-flex;
  gap: 4px;
}
.body-ref-form {
  display: flex;
  align-items: center;
  gap: 6px;
}
.body-ref-input {
  width: 110px;
}
.replay-result {
  font-size: 12px;
}
.result-ok {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
</style>
```

（`text-warning`/`text-error`/`--bg-hover`/`--accent-bg` 等类与变量以 `src/styles` 现状为准微调；CustomSelect 的 modelValue 为 `string | null`。）

- [ ] **Step 4: 类型检查 + Commit**

Run: `pnpm exec vue-tsc --noEmit`
Expected: 通过

```bash
git add src/windows/ReplayWindow.vue src/windows/launcher.ts
git commit -m "feat(ui): replay composer window"
```

---

### Task 14: 入口接线（小工具 / 日志详情 / 快照页）

**Files:**
- Modify: `src/components/AppToolbar.vue`
- Modify: `src/windows/LogDetailWindow.vue`
- Modify: `src/windows/InterceptorSnapshotWindow.vue`

- [ ] **Step 1: AppToolbar 小工具菜单新增"发送请求"**

import 区 `openBase64, openJwt, ...` 处加 `openReplay`；小工具菜单（`JWT 解码 / 验签` 项之后）加：

```vue
          <button class="tb-menu-item" @click="runMenuAction(() => openReplay())">
            <Io5SendOutline :size="14" />
            <span>发送请求</span>
          </button>
```

（图标 `Io5SendOutline` import 自 `vue-icons-plus/io5`，与现有图标导入合并。）

- [ ] **Step 2: LogDetailWindow 复制按钮旁新增重放入口**

script：`import { openReplay } from "./launcher";` 与 `import { prefillFromLog } from "../utils/replay";`，加：

```ts
function replayCurrent(): void {
  if (detail.value) openReplay(prefillFromLog(detail.value));
}
```

模板复制按钮（`copy-menu-button`）之前加：

```vue
          <button class="btn icon" title="重放此请求" @click="replayCurrent">
            <Io5SendOutline :size="14" />
          </button>
```

- [ ] **Step 3: InterceptorSnapshotWindow 头部新增重放入口（仅请求侧快照）**

script：`import { openReplay } from "./launcher";`、`import { prefillFromSnapshot } from "../utils/replay";`、`import { Io5SendOutline } from "vue-icons-plus/io5";`，加：

```ts
function replaySnapshot(): void {
  if (!snapshot.value) return;
  const draft = prefillFromSnapshot(props.sessionId, props.logId, snapshot.value);
  if (draft) openReplay(draft);
}
```

模板 `summary-line` 内末尾（`asset-meta` 之后）加：

```vue
          <span class="summary-spacer" />
          <button
            v-if="request"
            class="btn icon"
            title="重放此快照请求"
            @click="replaySnapshot"
          >
            <Io5SendOutline :size="14" />
          </button>
```

（若 snapshot 窗口无 `summary-spacer` 样式则直接在 url 后加按钮，保持现有布局。）

- [ ] **Step 4: 类型检查 + 构建 + Commit**

Run: `pnpm build`
Expected: `vue-tsc --noEmit && vite build` 通过

```bash
git add src/components/AppToolbar.vue src/windows/LogDetailWindow.vue src/windows/InterceptorSnapshotWindow.vue
git commit -m "feat(ui): replay entries in toolbar, log detail, and snapshot windows"
```

---

### Task 15: Skill 同步（`skills/proxycrab/`）

**Files:**
- Modify: `skills/proxycrab/references/http-api.md`
- Modify: `skills/proxycrab/SKILL.md`
- Create: `skills/proxycrab/scripts/replay-send.mjs`
- Modify: `skills/proxycrab/evals/evals.json`

- [ ] **Step 1: `references/http-api.md` 新增 "Request replay" 章节并更新 Contents**

在 Capture logs 章节之后插入新章节（Contents 同步加条目）：

```markdown
## Request replay

`POST /api/replay?session=<id>` sends a fully specified request through the target Session's
interceptor pipeline and outbound chain, recording it as a normal capture whose source is
`ProxyCrabRequest`. The proxy must be running. The response returns as soon as the capture is
created — it does not wait for the upstream exchange to finish.

```json
{
  "method": "POST",
  "url": "https://example.com/api?x=1",
  "headers": [["content-type", "application/json"]],
  "body": { "type": "text", "text": "{\"a\":1}", "charset": "utf8" }
}
```

- `headers` is an ordered array of `[name, value]` pairs; duplicates are preserved and sent as-is
  (including `content-length` — the sender is responsible for consistency).
- `body` is optional (omit for no body) and is one of:
  - `{ "type": "text", "text": "...", "charset": "utf8" }` — only `utf8` is supported.
  - `{ "type": "body_ref", "session_id": 1, "log_id": 2, "side": "request" | "response" }` —
    reuse the stored blob body of an existing capture.
  - `{ "type": "asset", "asset_id": "dir/file.bin" }` — reuse a workspace asset.
- Success: `{ "ok": true, "data": { "log_id": 12 } }`. Errors: `not_found` (session/body/asset),
  `proxy_not_running`, `bad_request` (validation), `replay_failed`.

`GET /api/assets` lists every workspace asset's metadata (`id`, `size`, `content_type`,
`sha256`, `created_at`) sorted by id; build trees from the `/`-separated ids. Fetch content with
the existing `GET /api/assets/{id}?format=raw`.
```

- [ ] **Step 2: `scripts/replay-send.mjs`**

```js
#!/usr/bin/env node
import {
  apiRequest,
  assertAllowedArgs,
  optionalInteger,
  optionalString,
  parseArgs,
  printHelp,
  printJson,
  requiredInteger,
  requiredString,
  run,
} from "./lib/common.mjs";

run(async () => {
  const args = parseArgs();
  assertAllowedArgs(args, [
    "session-id",
    "method",
    "url",
    "headers-json",
    "body-text",
    "body-asset-id",
    "body-ref",
  ]);
  if (args.help) {
    printHelp(`
Usage: node replay-send.mjs --session-id ID --method M --url U [options] [--base-url URL]

Send a request through a Session's pipeline (source: ProxyCrabRequest). The proxy must be running.

Options:
  --headers-json JSON   Ordered header pairs, e.g. '[["content-type","text/plain"]]'
  --body-text TEXT      UTF-8 text body
  --body-asset-id ID    Workspace asset body
  --body-ref S:L:SIDE   Reuse capture body, e.g. '3:12:request' or '3:12:response'
`);
    return;
  }
  const sessionId = requiredInteger(args, "session-id", { min: 1 });
  const method = requiredString(args, "method");
  const url = requiredString(args, "url");
  const headers = args["headers-json"] ? JSON.parse(args["headers-json"]) : [];
  const bodyModes = [args["body-text"], args["body-asset-id"], args["body-ref"]].filter(
    (value) => value !== undefined,
  );
  if (bodyModes.length > 1) throw new Error("use only one body option");
  let body;
  if (args["body-text"] !== undefined) {
    body = { type: "text", text: args["body-text"], charset: "utf8" };
  } else if (args["body-asset-id"] !== undefined) {
    body = { type: "asset", asset_id: args["body-asset-id"] };
  } else if (args["body-ref"] !== undefined) {
    const match = /^(\d+):(\d+):(request|response)$/.exec(args["body-ref"]);
    if (!match) throw new Error("--body-ref must be '<session>:<log>:request|response'");
    body = {
      type: "body_ref",
      session_id: Number(match[1]),
      log_id: Number(match[2]),
      side: match[3],
    };
  }
  printJson(
    await apiRequest(args, `/api/replay?session=${sessionId}`, {
      method: "POST",
      body: JSON.stringify({ method, url, headers, ...(body ? { body } : {}) }),
    }),
  );
});
```

（`apiRequest` 的 POST 调用形式以 `lib/common.mjs` 现状签名为准对齐，参考 `session-create.mjs`。）

- [ ] **Step 3: `SKILL.md`**：在能力/脚本清单中补充 `replay-send.mjs`（重放请求到指定 Session，source 为 ProxyCrabRequest，要求代理运行中），并在 HTTP API 概述中提及 `POST /api/replay` 与 `GET /api/assets`。

- [ ] **Step 4: `evals/evals.json` 新增用例**

```json
    {
      "id": 8,
      "prompt": "Use ProxyCrab to replay the request from log <N> into the active Session with an added x-debug: 1 header, then confirm the new capture exists with source ProxyCrabRequest and report its log id.",
      "expected_output": "The agent reads the source capture, replays it via the replay endpoint/script with body_ref (or text for textual bodies), verifies the new capture's source and interceptor history, and cites the new log id without fabricating response evidence.",
      "files": []
    }
```

（id 顺延现有最大 id。）

- [ ] **Step 5: Commit**

```bash
git add skills/proxycrab/references/http-api.md skills/proxycrab/SKILL.md skills/proxycrab/scripts/replay-send.mjs skills/proxycrab/evals/evals.json
git commit -m "docs(skill): document replay API and add replay-send script"
```

---

### Task 16: 项目文档同步 + 全量验证

**Files:**
- Modify: `docs/backend-api.md`（若该文件维护管理 API 清单）

- [ ] **Step 1: `docs/backend-api.md` 增加 `POST /api/replay` 与 `GET /api/assets` 说明**（风格对齐现有条目，内容同 Task 15 Step 1 的精简版）。

- [ ] **Step 2: 全量验证**

Run: `cargo test`（workspace 全量）、`pnpm test:unit`、`pnpm build`
Expected: 全部通过

- [ ] **Step 3: Commit**

```bash
git add docs/backend-api.md
git commit -m "docs: replay and asset-list management API"
```

---

## 自查记录

**Spec coverage：**
- 管线重构（归因/链路解耦、保持断连逻辑）→ Task 1、2（`handle_connect` 的 bypass/断连逻辑未触碰；活动会话切换逻辑未触碰）
- source=ProxyCrabRequest 落库 → Task 1 Step 1/9 + Task 3 测试断言
- 完整拦截器管线 → 复用 `handle_session_http_request`（Task 2）+ Task 3 拦截器测试
- 无 SSL 解密参与 → 重放直接构造 origin 请求进出站链，不经 CONNECT/MITM（Task 2）
- `POST /api/replay?session=` + `{log_id}` 立即返回 + 权限 Allow → Task 5、6
- `GET /api/assets` 列表 → Task 4、6；内容获取复用现有 `GET /api/assets/{id}?format=raw`（无需新接口）
- 重放窗口（多开、三入口、method 下拉+自定义、URL 高亮、headers 表格、三模式 body、警告、会话选择、执行反馈）→ Task 9–14
- body 三种类型 + charset=utf8 → Task 5（校验）+ Task 9（预填映射）
- Tauri/Web 对齐 → Task 7（Tauri 命令）+ Task 8（http/tauri 双 backend；share 只读自动 unsupported）
- Skill/文档/评测同步 → Task 15、16

**已知遗留（本期不做，实现时如遇阻塞回报用户）：**
- body 全量入内存；重放仅 HTTP/1.1 语义
- body_ref 模式下前端无法预知 body 大小，Content-Length 警告不触发（后端不强制）
