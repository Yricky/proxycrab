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
    Text {
        text: String,
    },
    BodyRef {
        session_id: u64,
        log_id: u64,
        side: BodySide,
    },
    Asset {
        asset_id: String,
    },
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
        if !self
            .sessions()
            .iter()
            .any(|session| session.id == session_id)
        {
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
                let asset = self
                    .asset(asset_id)
                    .map_err(|error| ReplayError::Internal(error.into()))?
                    .ok_or(ReplayError::AssetNotFound)?;
                Bytes::from(
                    tokio::fs::read(asset.path())
                        .await
                        .map_err(|error| ReplayError::Internal(error.into()))?,
                )
            }
            Some(ReplayBody::BodyRef {
                session_id: source_session,
                log_id,
                side,
            }) => {
                let source = self
                    .capture_body_source(*source_session, *log_id, *side)?
                    .ok_or(ReplayError::BodyNotFound)?;
                match source.data {
                    BodySourceData::Bytes(bytes) => Bytes::from(bytes),
                    BodySourceData::File(path) => Bytes::from(
                        tokio::fs::read(&path)
                            .await
                            .map_err(|error| ReplayError::Internal(error.into()))?,
                    ),
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
