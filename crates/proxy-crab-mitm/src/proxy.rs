use std::{
    convert::Infallible,
    error::Error as StdError,
    future::Future,
    net::SocketAddr,
    pin::Pin,
    str::FromStr,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    task::{Context as TaskContext, Poll},
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use bytes::Bytes;
use http_body_util::{BodyExt, Full, LengthLimitError, Limited, combinators::UnsyncBoxBody};
use hyper::{
    HeaderMap, Method, Request, Response, StatusCode, Uri, Version,
    body::{Body, Frame, Incoming},
    header::{
        CONNECTION, CONTENT_LENGTH, HOST, HeaderName, HeaderValue, TRANSFER_ENCODING, UPGRADE,
    },
    service::service_fn,
};
use hyper_util::rt::{TokioExecutor, TokioIo};
use rustls::{ClientConfig, RootCertStore, pki_types::ServerName};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, BufStream},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    task::{AbortHandle, JoinHandle},
    time::{Instant, Sleep, timeout},
};
use tokio_rustls::TlsConnector;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::{
    ProxyCrab,
    breakpoint::BreakpointContext,
    lua::{
        BodyReplacement, BreakpointHook, ModificationJournal, ResponseScriptContext,
        SharedInterceptorState, evaluate_routing, execute_request_with_state,
        execute_response_with_state, read_body_replacement,
    },
    model::{
        CaptureError, ErrorStage, HeaderValues, InterceptorExecutionOrigin, InterceptorKind,
        InterceptorRun, ProxyStatus, RequestData, RequestTags, ResponseData, ScriptKind,
        SessionInterceptor, SessionMetadata, script_content_hash,
    },
    runtime::SessionPin,
    storage::{BodySide, CaptureStore},
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(60);
const BODY_READ_TIMEOUT: Duration = Duration::from_secs(60);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CAPTURE_BODY_BYTES: usize = 64 * 1024 * 1024;
const MAX_CLIENT_CONNECTIONS: usize = 256;
const BINARY_HEADER_PREFIX: &str = "\u{e000}proxy-crab-binary:v1:";
const PACE_CHUNKS_PER_SECOND: u64 = 50;
const CRAB_REQ_SPEED_TAG: &str = "_crab_req_speed";
const CRAB_RESP_SPEED_TAG: &str = "_crab_resp_speed";
const CRAB_REQ_TIMEOUT_TAG: &str = "_crab_req_timeout";

type BoxError = Box<dyn StdError + Send + Sync>;
type ProxyBody = UnsyncBoxBody<Bytes, BoxError>;

#[derive(Debug, Clone)]
struct InterceptorSnapshot {
    name: String,
    hash: String,
    content: String,
}

#[derive(Debug, Clone, Default)]
struct SessionInterceptorSnapshot {
    request: Vec<InterceptorSnapshot>,
    response: Vec<InterceptorSnapshot>,
}

pub struct ProxyController {
    status: Arc<RwLock<ProxyStatus>>,
    cancellation: Mutex<Option<CancellationToken>>,
    task: Mutex<Option<JoinHandle<()>>>,
    operation: tokio::sync::Mutex<()>,
    lifecycle_transition: Mutex<()>,
}

impl ProxyController {
    pub fn new() -> Self {
        Self {
            status: Arc::new(RwLock::new(ProxyStatus::Stopped)),
            cancellation: Mutex::new(None),
            task: Mutex::new(None),
            operation: tokio::sync::Mutex::new(()),
            lifecycle_transition: Mutex::new(()),
        }
    }

    pub fn status(&self) -> ProxyStatus {
        self.status
            .read()
            .expect("proxy status lock poisoned")
            .clone()
    }

    pub(crate) fn with_stopped_session_mutation<T>(
        &self,
        action: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let _transition = self
            .lifecycle_transition
            .lock()
            .expect("proxy lifecycle transition lock poisoned");
        if !matches!(
            self.status(),
            ProxyStatus::Stopped | ProxyStatus::Failed { .. }
        ) {
            bail!("sessions cannot be deleted while the proxy is running");
        }
        action()
    }

    pub async fn start(&self, runtime: Arc<ProxyCrab>) -> Result<ProxyStatus> {
        let _operation = self.operation.lock().await;
        {
            let _transition = self
                .lifecycle_transition
                .lock()
                .expect("proxy lifecycle transition lock poisoned");
            if !matches!(
                self.status(),
                ProxyStatus::Stopped | ProxyStatus::Failed { .. }
            ) {
                bail!("proxy is already running or changing state");
            }
            *self.status.write().expect("proxy status lock poisoned") = ProxyStatus::Starting;
        }
        let config = runtime.config();
        let listener = match TcpListener::bind((config.proxy_host.as_str(), config.proxy_port))
            .await
        {
            Ok(listener) => listener,
            Err(error) => {
                let message = error.to_string();
                *self.status.write().expect("proxy status lock poisoned") = ProxyStatus::Failed {
                    message: message.clone(),
                };
                bail!("failed to bind proxy: {message}");
            }
        };
        let cancellation = CancellationToken::new();
        let tracker = TaskGroup::new();
        *self
            .cancellation
            .lock()
            .expect("proxy cancellation lock poisoned") = Some(cancellation.clone());
        let status = self.status.clone();
        let host = config.proxy_host.clone();
        let port = config.proxy_port;
        *status.write().expect("proxy status lock poisoned") = ProxyStatus::Running {
            host: host.clone(),
            port,
        };
        tracing::info!("MITM proxy listening on {host}:{port}");

        let task = tokio::spawn(async move {
            accept_loop(listener, runtime, cancellation, tracker).await;
            *status.write().expect("proxy status lock poisoned") = ProxyStatus::Stopped;
            tracing::info!("MITM proxy stopped");
        });
        *self.task.lock().expect("proxy task lock poisoned") = Some(task);
        Ok(self.status())
    }

    pub async fn stop(&self, runtime: &ProxyCrab) -> Result<ProxyStatus> {
        let _operation = self.operation.lock().await;
        if matches!(self.status(), ProxyStatus::Stopped) {
            return Ok(ProxyStatus::Stopped);
        }
        *self.status.write().expect("proxy status lock poisoned") = ProxyStatus::Stopping;
        if let Some(cancellation) = self
            .cancellation
            .lock()
            .expect("proxy cancellation lock poisoned")
            .take()
        {
            cancellation.cancel();
        }
        let task = self.task.lock().expect("proxy task lock poisoned").take();
        if let Some(mut task) = task
            && timeout(SHUTDOWN_TIMEOUT + Duration::from_secs(1), &mut task)
                .await
                .is_err()
        {
            task.abort();
        }
        runtime.mark_in_progress_as_shutdown();
        let _ = runtime.bypass_store().mark_in_progress_as_shutdown();
        *self.status.write().expect("proxy status lock poisoned") = ProxyStatus::Stopped;
        Ok(ProxyStatus::Stopped)
    }

    pub async fn regenerate_ca(&self, runtime: &ProxyCrab) -> Result<String> {
        let _operation = self.operation.lock().await;
        runtime.ensure_proxy_stopped()?;
        runtime.regenerate_ca_unlocked()
    }
}

#[derive(Clone)]
struct TaskGroup {
    tracker: TaskTracker,
    state: Arc<Mutex<TaskGroupState>>,
}

struct TaskGroupState {
    closing: bool,
    abort_handles: Vec<AbortHandle>,
}

struct ConnectCapture {
    store: CaptureStore,
    id: u64,
}

impl TaskGroup {
    fn new() -> Self {
        Self {
            tracker: TaskTracker::new(),
            state: Arc::new(Mutex::new(TaskGroupState {
                closing: false,
                abort_handles: Vec::new(),
            })),
        }
    }

    fn spawn<F>(&self, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let mut state = self.state.lock().expect("proxy task state lock poisoned");
        if state.closing {
            return;
        }
        state.abort_handles.retain(|handle| !handle.is_finished());
        let handle = self.tracker.spawn(future);
        state.abort_handles.push(handle.abort_handle());
    }

    async fn shutdown(self, timeout_duration: Duration) {
        {
            let mut state = self.state.lock().expect("proxy task state lock poisoned");
            state.closing = true;
            self.tracker.close();
        }
        if timeout(timeout_duration, self.tracker.wait())
            .await
            .is_err()
        {
            tracing::warn!("proxy connection drain timed out; aborting unfinished tasks");
            let handles = {
                let mut state = self.state.lock().expect("proxy task state lock poisoned");
                std::mem::take(&mut state.abort_handles)
            };
            for handle in handles {
                handle.abort();
            }
            if timeout(Duration::from_secs(1), self.tracker.wait())
                .await
                .is_err()
            {
                tracing::error!("aborted proxy tasks did not finish promptly");
            }
        }
    }
}

impl Default for ProxyController {
    fn default() -> Self {
        Self::new()
    }
}

async fn accept_loop(
    listener: TcpListener,
    runtime: Arc<ProxyCrab>,
    cancellation: CancellationToken,
    tracker: TaskGroup,
) {
    let connection_slots = Arc::new(Semaphore::new(MAX_CLIENT_CONNECTIONS));
    loop {
        tokio::select! {
            _ = cancellation.cancelled() => break,
            accepted = listener.accept() => match accepted {
                Ok((stream, address)) => {
                    match connection_slots.clone().try_acquire_owned() {
                        Ok(permit) => {
                            let client_runtime = runtime.clone();
                            let client_cancellation = cancellation.clone();
                            let client_tracker = tracker.clone();
                            tracker.spawn(async move {
                                let _permit = permit;
                                serve_client(
                                    stream,
                                    address,
                                    client_runtime,
                                    client_cancellation,
                                    client_tracker,
                                ).await;
                            });
                        }
                        Err(_) => tracing::warn!(
                            "proxy connection limit reached; rejecting client {address}"
                        ),
                    }
                }
                Err(error) => tracing::warn!("failed to accept proxy connection: {error}"),
            }
        }
    }
    tracker.shutdown(SHUTDOWN_TIMEOUT).await;
}

async fn serve_client(
    stream: TcpStream,
    source: SocketAddr,
    runtime: Arc<ProxyCrab>,
    cancellation: CancellationToken,
    tracker: TaskGroup,
) {
    let service_cancellation = cancellation.clone();
    let service = service_fn(move |request| {
        handle_proxy_request(
            request,
            source,
            runtime.clone(),
            service_cancellation.clone(),
            tracker.clone(),
        )
    });
    let connection = hyper::server::conn::http1::Builder::new()
        .preserve_header_case(true)
        .title_case_headers(true)
        .serve_connection(TokioIo::new(stream), service)
        .with_upgrades();
    tokio::pin!(connection);
    tokio::select! {
        result = &mut connection => {
            if let Err(error) = result {
                tracing::warn!("proxy client connection failed: {error}");
            }
        }
        _ = cancellation.cancelled() => {
            connection.as_mut().graceful_shutdown();
            let _ = timeout(SHUTDOWN_TIMEOUT, &mut connection).await;
        }
    }
}

async fn handle_proxy_request(
    mut request: Request<Incoming>,
    source: SocketAddr,
    runtime: Arc<ProxyCrab>,
    cancellation: CancellationToken,
    tracker: TaskGroup,
) -> Result<Response<ProxyBody>, Infallible> {
    let request_data = request_data(&request);
    if is_ca_download(&request_data) {
        return Ok(certificate_response(&runtime));
    }
    if request.method() == Method::CONNECT {
        return Ok(handle_connect(
            &mut request,
            source,
            runtime,
            cancellation,
            tracker,
        ));
    }
    Ok(handle_http_request(
        request,
        request_data,
        source,
        runtime,
        cancellation,
        tracker,
    )
    .await)
}

enum RouteDecision {
    Session(SessionMetadata),
    Bypass(&'static str),
}

fn resolve_route(
    runtime: &Arc<ProxyCrab>,
    request: &RequestData,
    authority: &str,
    phase: &str,
    source: SocketAddr,
) -> RouteDecision {
    let script = match runtime.selected_routing_script() {
        Ok(script) => script,
        Err(error) => {
            tracing::warn!("failed to load routing configuration: {error}");
            return RouteDecision::Bypass("routing_script_error");
        }
    };
    let Some(script) = script else {
        return active_route(runtime);
    };
    match evaluate_routing(
        &script.content,
        phase,
        request,
        authority,
        source,
        &script.name,
    ) {
        Ok(None | Some(false)) => RouteDecision::Bypass("script_bypass"),
        Ok(Some(true)) => active_route(runtime),
        Err(error) => {
            tracing::warn!("routing script {} failed: {error}", script.name);
            RouteDecision::Bypass("routing_script_error")
        }
    }
}

fn active_route(runtime: &ProxyCrab) -> RouteDecision {
    runtime
        .active_session()
        .map(RouteDecision::Session)
        .unwrap_or(RouteDecision::Bypass("no_active_session"))
}

fn handle_connect(
    request: &mut Request<Incoming>,
    source: SocketAddr,
    runtime: Arc<ProxyCrab>,
    cancellation: CancellationToken,
    tracker: TaskGroup,
) -> Response<ProxyBody> {
    let Some(authority) = request.uri().authority().cloned() else {
        return text_response(StatusCode::BAD_REQUEST, "invalid CONNECT authority");
    };
    let request_data = request_data(request);
    let decision = resolve_route(
        &runtime,
        &request_data,
        authority.as_str(),
        "connect",
        source,
    );
    if let RouteDecision::Bypass(reason) = decision {
        return handle_bypass_connect(
            request,
            BypassConnectContext {
                request_data,
                source,
                authority,
                reason,
                runtime,
                cancellation,
                tracker,
            },
        );
    }
    let RouteDecision::Session(session) = decision else {
        unreachable!()
    };
    let pin = match runtime.pin_session(session.id) {
        Ok(pin) => pin,
        Err(error) => {
            tracing::warn!("failed to pin routed CONNECT Session: {error}");
            return handle_bypass_connect(
                request,
                BypassConnectContext {
                    request_data,
                    source,
                    authority,
                    reason: "session_pin_failed",
                    runtime,
                    cancellation,
                    tracker,
                },
            );
        }
    };
    let capture = match begin_connect_capture(&pin, source, &request_data) {
        Ok(capture) => capture,
        Err(error) => {
            tracing::error!("failed to persist CONNECT request: {error}");
            return text_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "capture storage unavailable",
            );
        }
    };
    let on_upgrade = hyper::upgrade::on(request);
    tracker.clone().spawn(async move {
        let upgraded = tokio::select! {
            upgraded = on_upgrade => upgraded,
            _ = cancellation.cancelled() => return,
        };
        match upgraded {
            Ok(upgraded) => {
                process_connect(
                    TokioIo::new(upgraded),
                    authority,
                    source,
                    pin,
                    capture,
                    cancellation,
                    tracker,
                )
                .await;
            }
            Err(error) => record_connect_error(
                &capture,
                ErrorStage::Connect,
                "connect_upgrade_failed",
                &error.to_string(),
            ),
        }
    });
    Response::new(boxed_full(Bytes::new()))
}

struct BypassConnectContext {
    request_data: RequestData,
    source: SocketAddr,
    authority: hyper::http::uri::Authority,
    reason: &'static str,
    runtime: Arc<ProxyCrab>,
    cancellation: CancellationToken,
    tracker: TaskGroup,
}

fn handle_bypass_connect(
    request: &mut Request<Incoming>,
    context: BypassConnectContext,
) -> Response<ProxyBody> {
    let BypassConnectContext {
        request_data,
        source,
        authority,
        reason,
        runtime,
        cancellation,
        tracker,
    } = context;
    let entry_id = runtime
        .bypass_store()
        .begin(
            &source.to_string(),
            &request_data.method,
            &request_data.uri,
            &request_data.version,
            reason,
        )
        .map_err(|error| tracing::warn!("failed to persist bypass CONNECT: {error}"))
        .ok();
    let on_upgrade = hyper::upgrade::on(request);
    tracker.spawn(async move {
        let upgraded = tokio::select! {
            upgraded = on_upgrade => upgraded,
            _ = cancellation.cancelled() => return,
        };
        let mut client = match upgraded {
            Ok(upgraded) => TokioIo::new(upgraded),
            Err(error) => {
                if let Some(id) = entry_id {
                    let _ = runtime
                        .bypass_store()
                        .fail(id, &error.to_string(), None, None);
                }
                return;
            }
        };
        let mut upstream =
            match timeout(CONNECT_TIMEOUT, TcpStream::connect(authority.as_str())).await {
                Ok(Ok(upstream)) => upstream,
                Ok(Err(error)) => {
                    if let Some(id) = entry_id {
                        let _ = runtime
                            .bypass_store()
                            .fail(id, &error.to_string(), None, None);
                    }
                    return;
                }
                Err(_) => {
                    if let Some(id) = entry_id {
                        let _ = runtime.bypass_store().fail(
                            id,
                            "upstream connection timed out",
                            None,
                            None,
                        );
                    }
                    return;
                }
            };
        tokio::select! {
            result = tokio::io::copy_bidirectional(&mut client, &mut upstream) => {
                match result {
                    Ok((upload, download)) => {
                        if let Some(id) = entry_id {
                            let _ = runtime.bypass_store().complete(
                                id,
                                None,
                                Some(upload),
                                Some(download),
                            );
                        }
                    }
                    Err(error) => {
                        if let Some(id) = entry_id {
                            let _ = runtime.bypass_store().fail(
                                id,
                                &error.to_string(),
                                None,
                                None,
                            );
                        }
                    }
                }
            }
            _ = cancellation.cancelled() => {}
        }
    });
    Response::new(boxed_full(Bytes::new()))
}

async fn process_connect<C>(
    client: C,
    authority: hyper::http::uri::Authority,
    source: SocketAddr,
    pin: SessionPin,
    capture: ConnectCapture,
    cancellation: CancellationToken,
    tracker: TaskGroup,
) where
    C: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let runtime = pin.runtime();
    let mut client = BufStream::new(client);
    let inspected = tokio::select! {
        result = timeout(CONNECT_TIMEOUT, client.fill_buf()) => result,
        _ = cancellation.cancelled() => return,
    };
    let payload = match inspected {
        Ok(Ok(payload)) if !payload.is_empty() => payload,
        Ok(Ok(_)) => {
            record_connect_error(
                &capture,
                ErrorStage::Connect,
                "client_closed",
                "CONNECT tunnel closed before a payload was received",
            );
            return;
        }
        Ok(Err(error)) => {
            record_connect_error(
                &capture,
                ErrorStage::Connect,
                "connect_inspection_failed",
                &error.to_string(),
            );
            return;
        }
        Err(_) => {
            record_connect_error(
                &capture,
                ErrorStage::Connect,
                "connect_inspection_timeout",
                "CONNECT payload inspection timed out",
            );
            return;
        }
    };

    if payload.first().copied() != Some(0x16) {
        tunnel_connect(client, &authority, &capture, cancellation).await;
        return;
    }

    let config = match runtime.authority().server_config(authority.host()) {
        Ok(config) => config,
        Err(error) => {
            record_connect_error(
                &capture,
                ErrorStage::TlsHandshake,
                "certificate_generation_failed",
                &error.to_string(),
            );
            return;
        }
    };
    let acceptor = tokio_rustls::TlsAcceptor::from(config);
    let accepted = tokio::select! {
        result = timeout(CONNECT_TIMEOUT, acceptor.accept(client)) => result,
        _ = cancellation.cancelled() => return,
    };
    let tls = match accepted {
        Ok(Ok(tls)) => tls,
        Ok(Err(error)) => {
            let message = error.to_string();
            record_connect_error(
                &capture,
                ErrorStage::TlsHandshake,
                normalize_tls_error(&message),
                &message,
            );
            return;
        }
        Err(_) => {
            record_connect_error(
                &capture,
                ErrorStage::TlsHandshake,
                "client_tls_handshake_timeout",
                "client TLS handshake timed out",
            );
            return;
        }
    };
    let is_http2 = tls.get_ref().1.alpn_protocol() == Some(b"h2");
    if let Err(error) = capture.store.mitm_established(capture.id) {
        fail_capture(
            &capture.store,
            capture.id,
            ErrorStage::TlsHandshake,
            "connect_capture_finalize_failed",
            &error.to_string(),
        );
        return;
    }
    serve_mitm_tls(tls, authority, source, pin, cancellation, tracker, is_http2).await;
}

async fn serve_mitm_tls<C>(
    tls: C,
    authority: hyper::http::uri::Authority,
    source: SocketAddr,
    pin: SessionPin,
    cancellation: CancellationToken,
    tracker: TaskGroup,
    is_http2: bool,
) where
    C: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let runtime = pin.runtime();
    let session_id = pin.session_id();
    let _connection_pin = pin;
    let service_cancellation = cancellation.clone();
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
            Ok::<_, Infallible>(
                handle_session_http_request(
                    request,
                    source,
                    runtime,
                    session_id,
                    cancellation,
                    tracker,
                )
                .await,
            )
        }
    });
    if is_http2 {
        let connection = hyper::server::conn::http2::Builder::new(TokioExecutor::new())
            .serve_connection(TokioIo::new(tls), service);
        tokio::pin!(connection);
        tokio::select! {
            result = &mut connection => {
                if let Err(error) = result {
                    tracing::warn!("HTTP/2 MITM connection failed: {error}");
                }
            }
            _ = cancellation.cancelled() => {
                connection.as_mut().graceful_shutdown();
                let _ = timeout(SHUTDOWN_TIMEOUT, &mut connection).await;
            }
        }
    } else {
        let connection = hyper::server::conn::http1::Builder::new()
            .preserve_header_case(true)
            .title_case_headers(true)
            .serve_connection(TokioIo::new(tls), service)
            .with_upgrades();
        tokio::pin!(connection);
        tokio::select! {
            result = &mut connection => {
                if let Err(error) = result {
                    tracing::warn!("HTTPS MITM connection failed: {error}");
                }
            }
            _ = cancellation.cancelled() => {
                connection.as_mut().graceful_shutdown();
                let _ = timeout(SHUTDOWN_TIMEOUT, &mut connection).await;
            }
        }
    }
}

async fn handle_http_request(
    request: Request<Incoming>,
    request_data: RequestData,
    source: SocketAddr,
    runtime: Arc<ProxyCrab>,
    cancellation: CancellationToken,
    tracker: TaskGroup,
) -> Response<ProxyBody> {
    let authority = routing_authority(&request_data);
    match resolve_route(&runtime, &request_data, &authority, "http", source) {
        RouteDecision::Bypass(reason) => {
            handle_bypass_http(
                request,
                request_data,
                source,
                reason,
                runtime,
                cancellation,
                tracker,
            )
            .await
        }
        RouteDecision::Session(session) => {
            handle_session_http_request(request, source, runtime, session.id, cancellation, tracker)
                .await
        }
    }
}

struct BypassTransfer {
    store: crate::bypass::BypassStore,
    entry_id: Option<u64>,
    upload_bytes: AtomicU64,
    download_bytes: AtomicU64,
    finalized: AtomicBool,
}

impl BypassTransfer {
    fn new(runtime: &ProxyCrab, entry_id: Option<u64>) -> Arc<Self> {
        Arc::new(Self {
            store: runtime.bypass_store().clone(),
            entry_id,
            upload_bytes: AtomicU64::new(0),
            download_bytes: AtomicU64::new(0),
            finalized: AtomicBool::new(false),
        })
    }

    fn add_upload(&self, bytes: u64) {
        self.upload_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    fn add_download(&self, bytes: u64) {
        self.download_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    fn complete(&self, response_status: Option<u16>) {
        if self.finalized.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Some(id) = self.entry_id {
            let _ = self.store.complete(
                id,
                response_status,
                Some(self.upload_bytes.load(Ordering::Relaxed)),
                Some(self.download_bytes.load(Ordering::Relaxed)),
            );
        }
    }

    fn fail(&self, error: &str) {
        if self.finalized.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Some(id) = self.entry_id {
            let _ = self.store.fail(
                id,
                error,
                Some(self.upload_bytes.load(Ordering::Relaxed)),
                Some(self.download_bytes.load(Ordering::Relaxed)),
            );
        }
    }
}

struct TrackedBody<B> {
    inner: Pin<Box<B>>,
    transfer: Arc<BypassTransfer>,
    response_status: Option<u16>,
    response_remaining: Option<u64>,
    reached_eof: bool,
}

impl<B> TrackedBody<B> {
    fn request(inner: B, transfer: Arc<BypassTransfer>) -> Self {
        Self {
            inner: Box::pin(inner),
            transfer,
            response_status: None,
            response_remaining: None,
            reached_eof: false,
        }
    }

    fn response(inner: B, transfer: Arc<BypassTransfer>, response_status: u16) -> Self
    where
        B: Body,
    {
        let response_remaining = inner.size_hint().exact();
        let reached_eof = inner.is_end_stream() || response_remaining == Some(0);
        if reached_eof {
            transfer.complete(Some(response_status));
        }
        Self {
            inner: Box::pin(inner),
            transfer,
            response_status: Some(response_status),
            response_remaining,
            reached_eof,
        }
    }
}

impl<B> Body for TrackedBody<B>
where
    B: Body<Data = Bytes>,
    B::Error: StdError + Send + Sync + 'static,
{
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        context: &mut TaskContext<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        match this.inner.as_mut().poll_frame(context) {
            Poll::Ready(Some(Ok(frame))) => {
                if let Some(data) = frame.data_ref() {
                    if this.response_status.is_some() {
                        this.transfer.add_download(data.len() as u64);
                        if let Some(remaining) = &mut this.response_remaining {
                            *remaining = remaining.saturating_sub(data.len() as u64);
                            if *remaining == 0 {
                                this.reached_eof = true;
                                this.transfer.complete(this.response_status);
                            }
                        }
                    } else {
                        this.transfer.add_upload(data.len() as u64);
                    }
                }
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Ready(Some(Err(error))) => {
                this.reached_eof = true;
                this.transfer.fail(&error.to_string());
                Poll::Ready(Some(Err(Box::new(error))))
            }
            Poll::Ready(None) => {
                this.reached_eof = true;
                if let Some(status) = this.response_status {
                    this.transfer.complete(Some(status));
                }
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> hyper::body::SizeHint {
        self.inner.size_hint()
    }
}

impl<B> Drop for TrackedBody<B> {
    fn drop(&mut self) {
        if self.response_status.is_some() && !self.reached_eof {
            self.transfer
                .fail("downstream response body closed before completion");
        }
    }
}

struct PacedBody {
    bytes: Bytes,
    offset: usize,
    bytes_per_second: u64,
    pending_chunk_len: usize,
    sleep: Option<Pin<Box<Sleep>>>,
}

impl PacedBody {
    fn new(bytes: Bytes, bytes_per_second: u64) -> Self {
        debug_assert!(bytes_per_second > 0);
        Self {
            bytes,
            offset: 0,
            bytes_per_second,
            pending_chunk_len: 0,
            sleep: None,
        }
    }

    fn next_chunk_len(&self) -> usize {
        let remaining = self.bytes.len() - self.offset;
        let target = self
            .bytes_per_second
            .div_ceil(PACE_CHUNKS_PER_SECOND)
            .max(1);
        usize::try_from(target).unwrap_or(usize::MAX).min(remaining)
    }

    fn chunk_delay(&self, chunk_len: usize) -> Duration {
        let nanos =
            (chunk_len as u128 * 1_000_000_000_u128).div_ceil(self.bytes_per_second as u128);
        Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
    }
}

impl Body for PacedBody {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        context: &mut TaskContext<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        if this.offset == this.bytes.len() {
            return Poll::Ready(None);
        }
        if this.sleep.is_none() {
            this.pending_chunk_len = this.next_chunk_len();
            this.sleep = Some(Box::pin(tokio::time::sleep_until(
                Instant::now() + this.chunk_delay(this.pending_chunk_len),
            )));
        }
        if this
            .sleep
            .as_mut()
            .expect("paced body sleep was initialized")
            .as_mut()
            .poll(context)
            .is_pending()
        {
            return Poll::Pending;
        }
        this.sleep = None;
        let end = this.offset + this.pending_chunk_len;
        let chunk = this.bytes.slice(this.offset..end);
        this.offset = end;
        Poll::Ready(Some(Ok(Frame::data(chunk))))
    }

    fn is_end_stream(&self) -> bool {
        self.offset == self.bytes.len()
    }

    fn size_hint(&self) -> hyper::body::SizeHint {
        hyper::body::SizeHint::with_exact((self.bytes.len() - self.offset) as u64)
    }
}

async fn handle_bypass_http(
    mut request: Request<Incoming>,
    request_data: RequestData,
    source: SocketAddr,
    reason: &'static str,
    runtime: Arc<ProxyCrab>,
    cancellation: CancellationToken,
    tracker: TaskGroup,
) -> Response<ProxyBody> {
    let entry_id = runtime
        .bypass_store()
        .begin(
            &source.to_string(),
            &request_data.method,
            &request_data.uri,
            &request_data.version,
            reason,
        )
        .map_err(|error| tracing::warn!("failed to persist bypass request: {error}"))
        .ok();
    let transfer = BypassTransfer::new(&runtime, entry_id);
    let downstream_upgrade = is_upgrade_request(&request).then(|| hyper::upgrade::on(&mut request));
    let upstream_request = match streaming_upstream_request(request, transfer.clone()) {
        Ok(request) => request,
        Err(error) => {
            transfer.fail(&error.to_string());
            return text_response(StatusCode::BAD_REQUEST, "invalid upstream request");
        }
    };
    let mut upstream_response = match send_upstream(upstream_request, cancellation.clone()).await {
        Ok(response) => response,
        Err(error) => {
            transfer.fail(&error.to_string());
            return text_response(StatusCode::BAD_GATEWAY, "upstream request failed");
        }
    };
    let response_data = ResponseData {
        status: upstream_response.status().as_u16(),
        version: version_name(upstream_response.version()).to_string(),
        headers: headers_to_values(upstream_response.headers()),
    };
    if upstream_response.status() == StatusCode::SWITCHING_PROTOCOLS {
        if let Some(downstream_upgrade) = downstream_upgrade {
            let upstream_upgrade = hyper::upgrade::on(&mut upstream_response);
            let transfer = transfer.clone();
            tracker.spawn(async move {
                match (downstream_upgrade.await, upstream_upgrade.await) {
                    (Ok(downstream), Ok(upstream)) => {
                        let mut downstream = TokioIo::new(downstream);
                        let mut upstream = TokioIo::new(upstream);
                        tokio::select! {
                            result = tokio::io::copy_bidirectional(&mut downstream, &mut upstream) => {
                                match result {
                                    Ok((upload, download)) => {
                                        transfer.add_upload(upload);
                                        transfer.add_download(download);
                                        transfer.complete(Some(response_data.status));
                                    }
                                    Err(error) => transfer.fail(&error.to_string()),
                                }
                            }
                            _ = cancellation.cancelled() => {}
                        }
                    }
                    (Err(error), _) | (_, Err(error)) => transfer.fail(&error.to_string()),
                }
            });
        } else {
            transfer.complete(Some(response_data.status));
        }
        return response_from_data(&response_data, Vec::new(), false);
    }
    let (mut parts, incoming) = upstream_response.into_parts();
    strip_hop_by_hop_headers(&mut parts.headers, false);
    parts.headers.remove(TRANSFER_ENCODING);
    let body = TrackedBody::response(incoming, transfer, response_data.status).boxed_unsync();
    Response::from_parts(parts, body)
}

async fn handle_session_http_request(
    mut request: Request<Incoming>,
    source: SocketAddr,
    runtime: Arc<ProxyCrab>,
    session_id: u64,
    cancellation: CancellationToken,
    tracker: TaskGroup,
) -> Response<ProxyBody> {
    let pin = match runtime.pin_session(session_id) {
        Ok(pin) => pin,
        Err(error) => return text_response(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
    };
    let _capture_slot = runtime.acquire_capture_slot().await;
    let store = pin.store().clone();
    let interceptor_snapshot = match snapshot_session_interceptors(&runtime, pin.session_id()) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return text_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("failed to load session interceptors: {error}"),
            );
        }
    };
    let downstream_upgrade = is_upgrade_request(&request).then(|| hyper::upgrade::on(&mut request));
    let upgrade_request = downstream_upgrade.is_some();
    let (parts, incoming) = request.into_parts();
    let mut request_data = RequestData {
        method: parts.method.to_string(),
        uri: parts.uri.to_string(),
        version: version_name(parts.version).to_string(),
        headers: headers_to_values(&parts.headers),
        tags: Default::default(),
    };
    let capture_id = match store.begin(&source.to_string(), &request_data, "request") {
        Ok(id) => id,
        Err(error) => {
            tracing::error!("capture insert failed; refusing to forward request: {error}");
            return text_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "capture storage unavailable",
            );
        }
    };
    let collected_request = timeout(
        BODY_READ_TIMEOUT,
        Limited::new(incoming, MAX_CAPTURE_BODY_BYTES).collect(),
    )
    .await;
    let mut request_body = match collected_request {
        Ok(Ok(body)) => body.to_bytes().to_vec(),
        Ok(Err(error)) if error.downcast_ref::<LengthLimitError>().is_some() => {
            fail_capture(
                &store,
                capture_id,
                ErrorStage::RequestBody,
                "request_body_too_large",
                "request body exceeded the 64 MiB capture limit",
            );
            return text_response(StatusCode::PAYLOAD_TOO_LARGE, "request body too large");
        }
        Ok(Err(error)) => {
            fail_capture(
                &store,
                capture_id,
                ErrorStage::RequestBody,
                "request_body_read_failed",
                &error.to_string(),
            );
            return text_response(StatusCode::BAD_REQUEST, "failed to read request body");
        }
        Err(_) => {
            fail_capture(
                &store,
                capture_id,
                ErrorStage::RequestBody,
                "request_body_timeout",
                "request body read timed out",
            );
            return text_response(StatusCode::REQUEST_TIMEOUT, "request body read timed out");
        }
    };
    if let Err(error) = store.save_body(capture_id, BodySide::Request, false, &request_body) {
        fail_capture(
            &store,
            capture_id,
            ErrorStage::RequestBody,
            "request_body_store_failed",
            &error.to_string(),
        );
        return text_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to persist request body",
        );
    }

    let mut request_modifications = Vec::new();
    for (position, script) in interceptor_snapshot.request.iter().enumerate() {
        let state = SharedInterceptorState::new_request(
            request_data.method.clone(),
            request_data.uri.clone(),
            request_data.headers.clone(),
            request_data.tags.clone(),
        );
        let journal = ModificationJournal::new(request_data.headers.clone());
        let execution_id = match store.begin_interceptor_run(
            capture_id,
            &InterceptorRun {
                origin: InterceptorExecutionOrigin::Saved,
                completed: false,
                phase: InterceptorKind::Request,
                position,
                name: script.name.clone(),
                script_hash: script.hash.clone(),
                content: script.content.clone(),
                modifications: journal.snapshot(),
                error: None,
            },
        ) {
            Ok(id) => id,
            Err(error) => {
                fail_capture(
                    &store,
                    capture_id,
                    ErrorStage::Interceptor,
                    "interceptor_history_store_failed",
                    &error.to_string(),
                );
                return text_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "capture storage unavailable",
                );
            }
        };
        let breakpoint_context = BreakpointContext {
            session_id: pin.session_id(),
            capture_id,
            phase: InterceptorKind::Request,
            position,
            interceptor_name: script.name.clone(),
            parent_execution_id: execution_id,
            request: request_data.clone(),
            response: None,
            state: state.clone(),
            parent_journal: journal.clone(),
            store: store.clone(),
        };
        let source = script.content.clone();
        let script_name = script.name.clone();
        let request_snapshot = request_data.clone();
        let execution_state = state.clone();
        let execution_journal = journal.clone();
        let hook = BreakpointHook {
            registry: runtime.breakpoint_registry(),
            context: breakpoint_context,
        };
        let mut run_error = None;
        let execution = tokio::task::spawn_blocking(move || {
            execute_request_with_state(
                &source,
                &request_snapshot,
                execution_state,
                execution_journal,
                &script_name,
                Some(capture_id),
                Some(hook),
            )
        })
        .await;
        match execution {
            Ok(Ok((effects, error))) => {
                request_data.method = effects
                    .method
                    .expect("request interceptor effects always include a method");
                request_data.uri = effects
                    .uri
                    .expect("request interceptor effects always include a URI");
                request_data.headers = effects.headers;
                request_data.tags = effects.tags;
                if let Some(replacement) = effects.body {
                    match apply_body_replacement(&replacement) {
                        Ok(body) => {
                            request_body = body;
                            remove_header_value(&mut request_data.headers, "content-encoding");
                            if let Err(error) =
                                store.save_body(capture_id, BodySide::Request, true, &request_body)
                            {
                                note_script_error(
                                    &store,
                                    capture_id,
                                    "request-body",
                                    &error.to_string(),
                                );
                            }
                        }
                        Err(error) => {
                            let message = error.to_string();
                            note_script_error(&store, capture_id, &script.name, &message);
                            append_run_error(&mut run_error, message);
                        }
                    }
                }
                request_modifications.extend(effects.modifications);
                if let Some(error) = error {
                    note_script_error(&store, capture_id, &script.name, &error);
                    append_run_error(&mut run_error, error);
                }
            }
            Ok(Err(error)) => {
                let message = error.to_string();
                note_script_error(&store, capture_id, &script.name, &message);
                run_error = Some(message);
            }
            Err(error) => {
                let message = format!("interceptor worker failed: {error}");
                note_script_error(&store, capture_id, &script.name, &message);
                run_error = Some(message);
            }
        }
        let modifications = journal.snapshot();
        if let Err(error) =
            store.update_interceptor_run(execution_id, &modifications, run_error.as_deref(), true)
        {
            fail_capture(
                &store,
                capture_id,
                ErrorStage::Interceptor,
                "interceptor_history_store_failed",
                &error.to_string(),
            );
            return text_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "capture storage unavailable",
            );
        }
        if let Err(error) = store.update_tags(capture_id, &request_data.tags) {
            fail_capture(
                &store,
                capture_id,
                ErrorStage::Interceptor,
                "capture_tags_store_failed",
                &error.to_string(),
            );
            return text_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "capture storage unavailable",
            );
        }
    }
    if let Err(error) = store.update_request(capture_id, &request_data, &request_modifications) {
        fail_capture(
            &store,
            capture_id,
            ErrorStage::RequestBody,
            "capture_update_failed",
            &error.to_string(),
        );
        return text_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "capture storage unavailable",
        );
    }

    if is_ca_download(&request_data) {
        let bytes = runtime.certificate_pem().into_bytes();
        let response_data = ResponseData {
            status: 200,
            version: "HTTP/1.1".into(),
            headers: HeaderValues::from([(
                "content-type".into(),
                vec!["application/x-x509-ca-cert".into()],
            )]),
        };
        let _ = store.save_body(capture_id, BodySide::Response, false, &bytes);
        let _ = store.complete(capture_id, &response_data, &[]);
        return response_from_data(&response_data, bytes, false);
    }

    if request_data.tags.contains_key("_crab_skip") {
        return finish_session_response(
            SessionResponseContext {
                runtime,
                store,
                session_id: pin.session_id(),
                capture_id,
            },
            request_data,
            ResponseData {
                status: 200,
                version: "HTTP/1.1".into(),
                headers: HeaderValues::new(),
            },
            Vec::new(),
            &interceptor_snapshot.response,
        )
        .await;
    }

    let (request_speed, upstream_timeout) = if upgrade_request {
        (None, UPSTREAM_TIMEOUT)
    } else {
        (
            parse_special_tag(&request_data.tags, CRAB_REQ_SPEED_TAG, capture_id),
            parse_special_tag(&request_data.tags, CRAB_REQ_TIMEOUT_TAG, capture_id)
                .map(Duration::from_millis)
                .unwrap_or(UPSTREAM_TIMEOUT),
        )
    };
    let upstream_request = match request_from_data(&request_data, request_body, request_speed) {
        Ok(request) => request,
        Err(error) => {
            fail_capture(
                &store,
                capture_id,
                ErrorStage::Upstream,
                "invalid_upstream_request",
                &error.to_string(),
            );
            return text_response(StatusCode::BAD_REQUEST, "invalid upstream request");
        }
    };
    let mut upstream_response = match timeout(
        upstream_timeout,
        send_upstream(upstream_request, cancellation.clone()),
    )
    .await
    {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => {
            fail_capture(
                &store,
                capture_id,
                ErrorStage::Upstream,
                "upstream_failed",
                &error.to_string(),
            );
            return text_response(StatusCode::BAD_GATEWAY, "upstream request failed");
        }
        Err(_) => {
            fail_capture(
                &store,
                capture_id,
                ErrorStage::Upstream,
                "upstream_timeout",
                "upstream request timed out",
            );
            return text_response(StatusCode::GATEWAY_TIMEOUT, "upstream request timed out");
        }
    };

    let response_data = ResponseData {
        status: upstream_response.status().as_u16(),
        version: version_name(upstream_response.version()).to_string(),
        headers: headers_to_values(upstream_response.headers()),
    };
    if upstream_response.status() == StatusCode::SWITCHING_PROTOCOLS {
        if let Some(downstream_upgrade) = downstream_upgrade {
            let upstream_upgrade = hyper::upgrade::on(&mut upstream_response);
            let upgrade_cancellation = cancellation.clone();
            tracker.spawn(async move {
                if let (Ok(downstream), Ok(upstream)) =
                    (downstream_upgrade.await, upstream_upgrade.await)
                {
                    let mut downstream = TokioIo::new(downstream);
                    let mut upstream = TokioIo::new(upstream);
                    tokio::select! {
                        _ = tokio::io::copy_bidirectional(&mut downstream, &mut upstream) => {}
                        _ = upgrade_cancellation.cancelled() => {}
                    }
                }
            });
        }
        let _ = store.complete(capture_id, &response_data, &[]);
        return response_from_data(&response_data, Vec::new(), false);
    }

    let collected_response = timeout(
        BODY_READ_TIMEOUT,
        Limited::new(upstream_response, MAX_CAPTURE_BODY_BYTES).collect(),
    )
    .await;
    let response_body = match collected_response {
        Ok(Ok(body)) => body.to_bytes().to_vec(),
        Ok(Err(error)) if error.downcast_ref::<LengthLimitError>().is_some() => {
            fail_capture(
                &store,
                capture_id,
                ErrorStage::ResponseBody,
                "response_body_too_large",
                "response body exceeded the 64 MiB capture limit",
            );
            return text_response(StatusCode::BAD_GATEWAY, "upstream response body too large");
        }
        Ok(Err(error)) => {
            fail_capture(
                &store,
                capture_id,
                ErrorStage::ResponseBody,
                "response_body_read_failed",
                &error.to_string(),
            );
            return text_response(StatusCode::BAD_GATEWAY, "failed to read upstream response");
        }
        Err(_) => {
            fail_capture(
                &store,
                capture_id,
                ErrorStage::ResponseBody,
                "response_body_timeout",
                "response body read timed out",
            );
            return text_response(
                StatusCode::GATEWAY_TIMEOUT,
                "upstream response body timed out",
            );
        }
    };
    finish_session_response(
        SessionResponseContext {
            runtime,
            store,
            session_id: pin.session_id(),
            capture_id,
        },
        request_data,
        response_data,
        response_body,
        &interceptor_snapshot.response,
    )
    .await
}

struct SessionResponseContext {
    runtime: Arc<ProxyCrab>,
    store: CaptureStore,
    session_id: u64,
    capture_id: u64,
}

async fn finish_session_response(
    context: SessionResponseContext,
    mut request_data: RequestData,
    mut response_data: ResponseData,
    mut response_body: Vec<u8>,
    scripts: &[InterceptorSnapshot],
) -> Response<ProxyBody> {
    let SessionResponseContext {
        runtime,
        store,
        session_id,
        capture_id,
    } = context;
    if let Err(error) = store.save_body(capture_id, BodySide::Response, false, &response_body) {
        fail_capture(
            &store,
            capture_id,
            ErrorStage::ResponseBody,
            "response_body_store_failed",
            &error.to_string(),
        );
        return text_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to persist response body",
        );
    }
    if let Err(error) = store.update_response(capture_id, &response_data) {
        fail_capture(
            &store,
            capture_id,
            ErrorStage::ResponseBody,
            "capture_response_update_failed",
            &error.to_string(),
        );
        return text_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to persist response metadata",
        );
    }

    let mut response_modifications = Vec::new();
    for (position, script) in scripts.iter().enumerate() {
        let state = SharedInterceptorState::new_response(
            response_data.status,
            response_data.headers.clone(),
            request_data.tags.clone(),
        );
        let journal = ModificationJournal::new(response_data.headers.clone());
        let execution_id = match store.begin_interceptor_run(
            capture_id,
            &InterceptorRun {
                origin: InterceptorExecutionOrigin::Saved,
                completed: false,
                phase: InterceptorKind::Response,
                position,
                name: script.name.clone(),
                script_hash: script.hash.clone(),
                content: script.content.clone(),
                modifications: journal.snapshot(),
                error: None,
            },
        ) {
            Ok(id) => id,
            Err(error) => {
                fail_capture(
                    &store,
                    capture_id,
                    ErrorStage::Interceptor,
                    "interceptor_history_store_failed",
                    &error.to_string(),
                );
                return text_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "capture storage unavailable",
                );
            }
        };
        let breakpoint_context = BreakpointContext {
            session_id,
            capture_id,
            phase: InterceptorKind::Response,
            position,
            interceptor_name: script.name.clone(),
            parent_execution_id: execution_id,
            request: request_data.clone(),
            response: Some(response_data.clone()),
            state: state.clone(),
            parent_journal: journal.clone(),
            store: store.clone(),
        };
        let source = script.content.clone();
        let script_name = script.name.clone();
        let request_snapshot = request_data.clone();
        let response_snapshot = response_data.clone();
        let execution_state = state.clone();
        let execution_journal = journal.clone();
        let hook = BreakpointHook {
            registry: runtime.breakpoint_registry(),
            context: breakpoint_context,
        };
        let mut run_error = None;
        let execution = tokio::task::spawn_blocking(move || {
            execute_response_with_state(
                &source,
                ResponseScriptContext {
                    request: &request_snapshot,
                    response: &response_snapshot,
                },
                execution_state,
                execution_journal,
                &script_name,
                Some(capture_id),
                Some(hook),
            )
        })
        .await;
        match execution {
            Ok(Ok((effects, error))) => {
                response_data.status = effects
                    .status
                    .expect("response interceptor effects always include a status");
                response_data.headers = effects.headers;
                request_data.tags = effects.tags;
                if let Some(replacement) = effects.body {
                    match apply_body_replacement(&replacement) {
                        Ok(body) => {
                            response_body = body;
                            remove_header_value(&mut response_data.headers, "content-encoding");
                            if let Err(error) = store.save_body(
                                capture_id,
                                BodySide::Response,
                                true,
                                &response_body,
                            ) {
                                note_script_error(
                                    &store,
                                    capture_id,
                                    "response-body",
                                    &error.to_string(),
                                );
                            }
                        }
                        Err(error) => {
                            let message = error.to_string();
                            note_script_error(&store, capture_id, &script.name, &message);
                            append_run_error(&mut run_error, message);
                        }
                    }
                }
                response_modifications.extend(effects.modifications);
                if let Some(error) = error {
                    note_script_error(&store, capture_id, &script.name, &error);
                    append_run_error(&mut run_error, error);
                }
            }
            Ok(Err(error)) => {
                let message = error.to_string();
                note_script_error(&store, capture_id, &script.name, &message);
                run_error = Some(message);
            }
            Err(error) => {
                let message = format!("interceptor worker failed: {error}");
                note_script_error(&store, capture_id, &script.name, &message);
                run_error = Some(message);
            }
        }
        if let Err(error) = store.update_interceptor_run(
            execution_id,
            &journal.snapshot(),
            run_error.as_deref(),
            true,
        ) {
            fail_capture(
                &store,
                capture_id,
                ErrorStage::Interceptor,
                "interceptor_history_store_failed",
                &error.to_string(),
            );
            return text_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "capture storage unavailable",
            );
        }
        if let Err(error) = store.update_tags(capture_id, &request_data.tags) {
            fail_capture(
                &store,
                capture_id,
                ErrorStage::Interceptor,
                "capture_tags_store_failed",
                &error.to_string(),
            );
            return text_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "capture storage unavailable",
            );
        }
    }
    let _ = store.complete(capture_id, &response_data, &response_modifications);
    let response_speed = parse_special_tag(&request_data.tags, CRAB_RESP_SPEED_TAG, capture_id);
    response_from_data_with_speed(
        &response_data,
        response_body,
        request_data.method.eq_ignore_ascii_case("HEAD"),
        response_speed,
    )
}

fn snapshot_session_interceptors(
    runtime: &ProxyCrab,
    session_id: u64,
) -> Result<SessionInterceptorSnapshot> {
    let chains = runtime.session_interceptors(session_id)?;
    Ok(SessionInterceptorSnapshot {
        request: resolve_interceptor_snapshots(
            runtime,
            session_id,
            InterceptorKind::Request,
            ScriptKind::RequestInterceptor,
            chains.request,
        ),
        response: resolve_interceptor_snapshots(
            runtime,
            session_id,
            InterceptorKind::Response,
            ScriptKind::ResponseInterceptor,
            chains.response,
        ),
    })
}

fn resolve_interceptor_snapshots(
    runtime: &ProxyCrab,
    session_id: u64,
    phase: InterceptorKind,
    kind: ScriptKind,
    entries: Vec<SessionInterceptor>,
) -> Vec<InterceptorSnapshot> {
    entries
        .into_iter()
        .filter(|entry| entry.enabled)
        .filter_map(|entry| match runtime.script(kind, &entry.name) {
            Ok(script) => Some(InterceptorSnapshot {
                name: script.name,
                hash: script_content_hash(&script.content),
                content: script.content,
            }),
            Err(error) => {
                let message = format!(
                    "session {session_id} {} interceptor {} is unavailable and was skipped: {error}",
                    match phase {
                        InterceptorKind::Request => "request",
                        InterceptorKind::Response => "response",
                    },
                    entry.name
                );
                tracing::warn!("{message}");
                runtime.log_buffer().push("WARN", message);
                None
            }
        })
        .collect()
}

fn append_run_error(current: &mut Option<String>, next: String) {
    match current {
        Some(current) => {
            current.push_str("; ");
            current.push_str(&next);
        }
        None => *current = Some(next),
    }
}

async fn tunnel_connect<C>(
    mut client: C,
    authority: &hyper::http::uri::Authority,
    capture: &ConnectCapture,
    cancellation: CancellationToken,
) where
    C: AsyncRead + AsyncWrite + Unpin,
{
    let mut upstream = match timeout(CONNECT_TIMEOUT, TcpStream::connect(authority.as_str())).await
    {
        Ok(Ok(upstream)) => upstream,
        Ok(Err(error)) => {
            fail_capture(
                &capture.store,
                capture.id,
                ErrorStage::Connect,
                "upstream_connect_failed",
                &error.to_string(),
            );
            return;
        }
        Err(_) => {
            fail_capture(
                &capture.store,
                capture.id,
                ErrorStage::Connect,
                "upstream_connect_timeout",
                "upstream connection timed out",
            );
            return;
        }
    };
    let _ = capture.store.tunneled(capture.id);
    tokio::select! {
        result = tokio::io::copy_bidirectional(&mut client, &mut upstream) => {
            if let Err(error) = result {
                tracing::warn!("CONNECT tunnel for {authority} failed: {error}");
            }
        }
        _ = cancellation.cancelled() => {}
    }
}

async fn send_upstream(
    request: Request<ProxyBody>,
    cancellation: CancellationToken,
) -> Result<Response<Incoming>> {
    let uri = request.uri().clone();
    let host = uri
        .host()
        .ok_or_else(|| anyhow!("upstream URI has no host"))?;
    let https = uri.scheme_str() == Some("https");
    let port = uri.port_u16().unwrap_or(if https { 443 } else { 80 });
    let stream = timeout(CONNECT_TIMEOUT, TcpStream::connect((host, port)))
        .await
        .context("upstream connect timeout")??;
    if https {
        let roots = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let mut config = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        let connector = TlsConnector::from(Arc::new(config));
        let server_name = ServerName::try_from(host.to_string())
            .map_err(|error| anyhow!("invalid TLS server name: {error}"))?;
        let tls = connector.connect(server_name, stream).await?;
        let use_http2 = tls.get_ref().1.alpn_protocol() == Some(b"h2");
        send_on_io(tls, request, use_http2, cancellation).await
    } else {
        let use_http2 = request.version() == Version::HTTP_2;
        send_on_io(stream, request, use_http2, cancellation).await
    }
}

async fn send_on_io<T>(
    stream: T,
    mut request: Request<ProxyBody>,
    use_http2: bool,
    cancellation: CancellationToken,
) -> Result<Response<Incoming>>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    if use_http2 {
        let (mut sender, connection) =
            hyper::client::conn::http2::Builder::new(TokioExecutor::new())
                .handshake(TokioIo::new(stream))
                .await?;
        tokio::spawn(async move {
            tokio::select! {
                result = connection => {
                    if let Err(error) = result {
                        tracing::warn!("HTTP/2 upstream connection failed: {error}");
                    }
                }
                _ = cancellation.cancelled() => {}
            }
        });
        Ok(sender.send_request(request).await?)
    } else {
        let origin = request
            .uri()
            .path_and_query()
            .map(|value| value.as_str())
            .unwrap_or("/");
        *request.uri_mut() = Uri::from_str(origin)?;
        let (mut sender, connection) =
            hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;
        tokio::spawn(async move {
            tokio::select! {
                result = connection.with_upgrades() => {
                    if let Err(error) = result {
                        tracing::warn!("HTTP/1 upstream connection failed: {error}");
                    }
                }
                _ = cancellation.cancelled() => {}
            }
        });
        Ok(sender.send_request(request).await?)
    }
}

fn request_from_data(
    data: &RequestData,
    body: Vec<u8>,
    bytes_per_second: Option<u64>,
) -> Result<Request<ProxyBody>> {
    let uri = Uri::from_str(&data.uri)?;
    let mut builder = Request::builder()
        .method(Method::from_bytes(data.method.as_bytes())?)
        .uri(uri.clone())
        .version(parse_version(&data.version));
    let headers = builder.headers_mut().expect("request builder is valid");
    values_to_headers(&data.headers, headers)?;
    let preserve_upgrade = headers.contains_key(UPGRADE);
    strip_hop_by_hop_headers(headers, preserve_upgrade);
    if !headers.contains_key(HOST)
        && let Some(authority) = uri.authority()
    {
        headers.insert(HOST, HeaderValue::from_str(authority.as_str())?);
    }
    headers.remove(TRANSFER_ENCODING);
    headers.insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&body.len().to_string())?,
    );
    let body = Bytes::from(body);
    let body = match bytes_per_second.filter(|_| !body.is_empty()) {
        Some(bytes_per_second) => PacedBody::new(body, bytes_per_second).boxed_unsync(),
        None => boxed_full(body),
    };
    Ok(builder.body(body)?)
}

fn streaming_upstream_request(
    request: Request<Incoming>,
    transfer: Arc<BypassTransfer>,
) -> Result<Request<ProxyBody>> {
    let (mut parts, incoming) = request.into_parts();
    let uri = parts.uri.clone();
    let preserve_upgrade = parts.headers.contains_key(UPGRADE);
    strip_hop_by_hop_headers(&mut parts.headers, preserve_upgrade);
    if !parts.headers.contains_key(HOST)
        && let Some(authority) = uri.authority()
    {
        parts
            .headers
            .insert(HOST, HeaderValue::from_str(authority.as_str())?);
    }
    let body = TrackedBody::request(incoming, transfer).boxed_unsync();
    Ok(Request::from_parts(parts, body))
}

fn response_from_data(
    data: &ResponseData,
    body: Vec<u8>,
    preserve_content_length: bool,
) -> Response<ProxyBody> {
    response_from_data_with_speed(data, body, preserve_content_length, None)
}

fn response_from_data_with_speed(
    data: &ResponseData,
    body: Vec<u8>,
    preserve_content_length: bool,
    bytes_per_second: Option<u64>,
) -> Response<ProxyBody> {
    let mut builder = Response::builder()
        .status(data.status)
        .version(parse_version(&data.version));
    if let Some(headers) = builder.headers_mut() {
        if let Err(error) = values_to_headers(&data.headers, headers) {
            tracing::error!("failed to build downstream response headers: {error}");
            return text_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "invalid downstream response headers",
            );
        }
        strip_hop_by_hop_headers(
            headers,
            data.status == StatusCode::SWITCHING_PROTOCOLS.as_u16(),
        );
        headers.remove(TRANSFER_ENCODING);
        if !preserve_content_length
            && response_status_has_body(data.status)
            && let Ok(value) = HeaderValue::from_str(&body.len().to_string())
        {
            headers.insert(CONTENT_LENGTH, value);
        }
    }
    let body = Bytes::from(body);
    let body = match bytes_per_second.filter(|_| !body.is_empty()) {
        Some(bytes_per_second) => PacedBody::new(body, bytes_per_second).boxed_unsync(),
        None => boxed_full(body),
    };
    builder
        .body(body)
        .unwrap_or_else(|_| text_response(StatusCode::INTERNAL_SERVER_ERROR, "invalid response"))
}

fn boxed_full(bytes: Bytes) -> ProxyBody {
    Full::new(bytes)
        .map_err(|never| match never {})
        .boxed_unsync()
}

fn parse_positive_decimal(value: Option<&str>) -> Result<Option<u64>, ()> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(());
    }
    value
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .map(Some)
        .ok_or(())
}

fn parse_special_tag(tags: &RequestTags, key: &'static str, capture_id: u64) -> Option<u64> {
    match parse_positive_decimal(tags.get(key).map(String::as_str)) {
        Ok(value) => value,
        Err(()) => {
            tracing::warn!(capture_id, tag = key, "ignoring invalid special tag value");
            None
        }
    }
}

fn response_status_has_body(status: u16) -> bool {
    !(100..200).contains(&status)
        && status != StatusCode::NO_CONTENT.as_u16()
        && status != StatusCode::NOT_MODIFIED.as_u16()
}

fn values_to_headers(values: &HeaderValues, headers: &mut HeaderMap) -> Result<()> {
    headers.clear();
    for (name, values) in values {
        let name = HeaderName::from_bytes(name.as_bytes())?;
        for value in values {
            let value = if let Some(encoded) = value.strip_prefix(BINARY_HEADER_PREFIX) {
                HeaderValue::from_bytes(&decode_hex(encoded)?)?
            } else {
                HeaderValue::from_str(value)?
            };
            headers.append(name.clone(), value);
        }
    }
    Ok(())
}

fn headers_to_values(headers: &HeaderMap) -> HeaderValues {
    let mut result = HeaderValues::new();
    for name in headers.keys() {
        result.insert(
            name.as_str().to_string(),
            headers
                .get_all(name)
                .iter()
                .map(|value| {
                    value.to_str().map(str::to_owned).unwrap_or_else(|_| {
                        format!("{BINARY_HEADER_PREFIX}{}", encode_hex(value.as_bytes()))
                    })
                })
                .collect(),
        );
    }
    result
}

fn remove_header_value(headers: &mut HeaderValues, name: &str) {
    if let Some(key) = headers
        .keys()
        .find(|key| key.eq_ignore_ascii_case(name))
        .cloned()
    {
        headers.remove(&key);
    }
}

fn strip_hop_by_hop_headers(headers: &mut HeaderMap, preserve_upgrade: bool) {
    let connection_headers = headers
        .get_all(CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    for name in connection_headers {
        if !preserve_upgrade || name != "upgrade" {
            headers.remove(name);
        }
    }
    for name in [
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "proxy-connection",
        "te",
        "trailer",
    ] {
        headers.remove(name);
    }
    if !preserve_upgrade {
        headers.remove(CONNECTION);
        headers.remove(UPGRADE);
    } else {
        headers.insert(CONNECTION, HeaderValue::from_static("upgrade"));
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn decode_hex(encoded: &str) -> Result<Vec<u8>> {
    if !encoded.len().is_multiple_of(2) {
        bail!("invalid encoded binary header");
    }
    encoded
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = hex_digit(pair[0])?;
            let low = hex_digit(pair[1])?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn hex_digit(digit: u8) -> Result<u8> {
    match digit {
        b'0'..=b'9' => Ok(digit - b'0'),
        b'a'..=b'f' => Ok(digit - b'a' + 10),
        b'A'..=b'F' => Ok(digit - b'A' + 10),
        _ => bail!("invalid encoded binary header"),
    }
}

fn inject_https_authority<B>(request: &mut Request<B>, authority: &hyper::http::uri::Authority) {
    let mut parts = request.uri().clone().into_parts();
    parts.scheme = Some(hyper::http::uri::Scheme::HTTPS);
    if parts.authority.is_none() {
        parts.authority = Some(authority.clone());
    }
    if let Ok(uri) = Uri::from_parts(parts) {
        *request.uri_mut() = uri;
    }
}

fn request_data<B>(request: &Request<B>) -> RequestData {
    RequestData {
        method: request.method().to_string(),
        uri: request.uri().to_string(),
        version: version_name(request.version()).to_string(),
        headers: headers_to_values(request.headers()),
        tags: Default::default(),
    }
}

fn routing_authority(request: &RequestData) -> String {
    request
        .uri
        .parse::<Uri>()
        .ok()
        .and_then(|uri| uri.authority().map(ToString::to_string))
        .or_else(|| {
            request.headers.iter().find_map(|(name, values)| {
                name.eq_ignore_ascii_case("host")
                    .then(|| values.first().cloned())
                    .flatten()
            })
        })
        .unwrap_or_default()
}

fn begin_connect_capture(
    pin: &SessionPin,
    source: SocketAddr,
    request: &RequestData,
) -> Result<ConnectCapture> {
    let store = pin.store().clone();
    let id = store.begin(&source.to_string(), request, "connect")?;
    Ok(ConnectCapture { store, id })
}

fn record_connect_error(capture: &ConnectCapture, stage: ErrorStage, kind: &str, message: &str) {
    fail_capture(&capture.store, capture.id, stage, kind, message);
}

fn fail_capture(store: &CaptureStore, id: u64, stage: ErrorStage, kind: &str, message: &str) {
    let error = CaptureError {
        stage,
        kind: kind.into(),
        message: message.into(),
    };
    if let Err(storage_error) = store.fail(id, &error) {
        tracing::error!("failed to update capture {id}: {storage_error}");
    }
    tracing::warn!("{kind}: {message}");
}

fn note_script_error(store: &CaptureStore, id: u64, name: &str, message: &str) {
    if let Err(error) = store.note_error(
        id,
        &CaptureError {
            stage: ErrorStage::Interceptor,
            kind: "interceptor_runtime_error".into(),
            message: format!("{name}: {message}"),
        },
    ) {
        tracing::error!("failed to record interceptor error: {error}");
    }
    tracing::warn!("interceptor {name} failed: {message}");
}

fn apply_body_replacement(replacement: &BodyReplacement) -> Result<Vec<u8>> {
    read_body_replacement(replacement)
}

fn is_upgrade_request(request: &Request<Incoming>) -> bool {
    request.headers().contains_key(UPGRADE)
        || request
            .headers()
            .get(CONNECTION)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.to_ascii_lowercase().contains("upgrade"))
}

fn is_ca_download(request: &RequestData) -> bool {
    if request.method != "GET" {
        return false;
    }
    let Ok(uri) = request.uri.parse::<Uri>() else {
        return false;
    };
    if uri.path() != "/ca.crt" {
        return false;
    }
    uri.host() == Some("proxy.crab")
        || routing_authority(request)
            .parse::<hyper::http::uri::Authority>()
            .ok()
            .is_some_and(|authority| authority.host() == "proxy.crab")
}

fn certificate_response(runtime: &ProxyCrab) -> Response<ProxyBody> {
    let response = ResponseData {
        status: 200,
        version: "HTTP/1.1".into(),
        headers: HeaderValues::from([(
            "content-type".into(),
            vec!["application/x-x509-ca-cert".into()],
        )]),
    };
    response_from_data(&response, runtime.certificate_pem().into_bytes(), false)
}

fn normalize_tls_error(message: &str) -> &'static str {
    let normalized = message.to_ascii_lowercase().replace([' ', '_'], "");
    if normalized.contains("unknownca")
        || normalized.contains("badcertificate")
        || normalized.contains("certificateunknown")
    {
        "client_rejected_ca"
    } else {
        "tls_protocol_error"
    }
}

fn parse_version(version: &str) -> Version {
    match version {
        "HTTP/0.9" => Version::HTTP_09,
        "HTTP/1.0" => Version::HTTP_10,
        "HTTP/2.0" | "HTTP/2" => Version::HTTP_2,
        "HTTP/3.0" | "HTTP/3" => Version::HTTP_3,
        _ => Version::HTTP_11,
    }
}

fn version_name(version: Version) -> &'static str {
    match version {
        Version::HTTP_09 => "HTTP/0.9",
        Version::HTTP_10 => "HTTP/1.0",
        Version::HTTP_11 => "HTTP/1.1",
        Version::HTTP_2 => "HTTP/2",
        Version::HTTP_3 => "HTTP/3",
        _ => "HTTP/1.1",
    }
}

fn text_response(status: StatusCode, message: &str) -> Response<ProxyBody> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain; charset=utf-8")
        .body(boxed_full(Bytes::copy_from_slice(message.as_bytes())))
        .expect("static error response is valid")
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bytes::Bytes;
    use http_body_util::BodyExt;
    use hyper::{
        HeaderMap,
        body::Body,
        header::{CONNECTION, HeaderValue, UPGRADE},
    };
    use tokio::time::{Instant, sleep};

    use super::{
        PacedBody, headers_to_values, normalize_tls_error, parse_positive_decimal,
        remove_header_value, response_from_data, strip_hop_by_hop_headers, values_to_headers,
        version_name,
    };
    use crate::model::{HeaderValues, ResponseData};

    #[test]
    fn recognizes_client_ca_rejection() {
        assert_eq!(
            normalize_tls_error("received fatal alert: UnknownCA"),
            "client_rejected_ca"
        );
        assert_eq!(
            normalize_tls_error("invalid peer message"),
            "tls_protocol_error"
        );
    }

    #[test]
    fn formats_http_versions() {
        assert_eq!(version_name(hyper::Version::HTTP_2), "HTTP/2");
    }

    #[test]
    fn binary_headers_round_trip_without_loss() {
        let mut original = HeaderMap::new();
        original.insert("x-binary", HeaderValue::from_bytes(b"\xff\x80").unwrap());
        let values = headers_to_values(&original);
        let mut restored = HeaderMap::new();
        values_to_headers(&values, &mut restored).unwrap();
        assert_eq!(restored["x-binary"].as_bytes(), b"\xff\x80");
    }

    #[test]
    fn upgrade_connection_header_is_normalized() {
        let mut headers = HeaderMap::new();
        headers.insert(CONNECTION, HeaderValue::from_static("Upgrade, X-Remove"));
        headers.insert(UPGRADE, HeaderValue::from_static("websocket"));
        headers.insert("x-remove", HeaderValue::from_static("private"));
        strip_hop_by_hop_headers(&mut headers, true);
        assert_eq!(headers[CONNECTION], "upgrade");
        assert!(headers.contains_key(UPGRADE));
        assert!(!headers.contains_key("x-remove"));
    }

    #[test]
    fn replacement_removes_content_encoding_case_insensitively() {
        let mut headers = HeaderValues::from([("Content-Encoding".into(), vec!["gzip".into()])]);
        remove_header_value(&mut headers, "content-encoding");
        assert!(headers.is_empty());
    }

    #[test]
    fn parses_strict_positive_ascii_decimal_values() {
        assert_eq!(parse_positive_decimal(None), Ok(None));
        assert_eq!(parse_positive_decimal(Some("1")), Ok(Some(1)));
        assert_eq!(parse_positive_decimal(Some("001")), Ok(Some(1)));
        assert_eq!(
            parse_positive_decimal(Some("18446744073709551615")),
            Ok(Some(u64::MAX))
        );

        for invalid in [
            "",
            "0",
            "-1",
            "+1",
            " 1",
            "1 ",
            "1.0",
            "1e3",
            "1KB",
            "１",
            "18446744073709551616",
        ] {
            assert_eq!(parse_positive_decimal(Some(invalid)), Err(()), "{invalid}");
        }
    }

    #[tokio::test]
    async fn paced_body_waits_before_each_chunk_without_catching_up() {
        let started_at = Instant::now();
        let mut body = PacedBody::new(Bytes::from_static(b"abcd"), 40);

        let frame = body.frame().await.unwrap().unwrap();
        let first_at = Instant::now();
        let first_bytes = frame.into_data().unwrap();
        assert!(first_at.duration_since(started_at) >= Duration::from_millis(20));
        assert_eq!(first_bytes, Bytes::from_static(b"a"));

        sleep(Duration::from_millis(75)).await;
        let late_poll_at = Instant::now();
        let frame = body.frame().await.unwrap().unwrap();
        let second_at = Instant::now();
        let second_bytes = frame.into_data().unwrap();
        assert!(second_at.duration_since(late_poll_at) >= Duration::from_millis(20));
        assert_eq!(second_bytes, Bytes::from_static(b"b"));
        assert_eq!(body.size_hint().exact(), Some(2));

        assert_eq!(
            body.collect().await.unwrap().to_bytes(),
            Bytes::from_static(b"cd")
        );

        let mut empty = PacedBody::new(Bytes::new(), 1);
        assert!(empty.is_end_stream());
        assert!(empty.frame().await.is_none());
    }

    #[test]
    fn head_and_not_modified_preserve_content_length() {
        let response = ResponseData {
            status: 200,
            version: "HTTP/1.1".into(),
            headers: HeaderValues::from([("content-length".into(), vec!["42".into()])]),
        };
        assert_eq!(
            response_from_data(&response, Vec::new(), true).headers()["content-length"],
            "42"
        );
        let response = ResponseData {
            status: 304,
            ..response
        };
        assert_eq!(
            response_from_data(&response, Vec::new(), false).headers()["content-length"],
            "42"
        );
    }
}
