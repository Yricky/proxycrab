use std::{
    collections::HashMap,
    convert::Infallible,
    future::Future,
    net::SocketAddr,
    str::FromStr,
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use anyhow::{Result, bail};
use bytes::Bytes;
use futures::TryStreamExt;
use http_body_util::{BodyExt, StreamBody};
use hyper::{
    HeaderMap, Method, Request, Response, StatusCode, Uri, Version,
    body::Incoming,
    header::{
        CONNECTION, CONTENT_LENGTH, HOST, HeaderName, HeaderValue, TRANSFER_ENCODING, UPGRADE,
    },
    service::service_fn,
};
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, BufStream},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    task::{AbortHandle, JoinHandle},
    time::timeout,
};
use tokio_util::io::ReaderStream;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::{
    ProxyCrab,
    breakpoint::BreakpointContext,
    bypass::BypassStore,
    lua::{
        BodyReplacement, BreakpointHook, ModificationJournal, ResponseScriptContext,
        SharedInterceptorState, evaluate_routing, execute_request_with_state,
        execute_response_with_state,
    },
    model::{
        CaptureError, ErrorStage, HeaderValues, InterceptorExecutionOrigin, InterceptorKind,
        InterceptorRun, ProxyStatus, RequestData, RequestTags, ResponseData, ScriptKind,
        SessionInterceptor, SessionMetadata, script_content_hash,
    },
    runtime::SessionPin,
    storage::{BodySide, CaptureStore},
};

mod body;
mod bypass;
mod mitm;
mod upstream;

use body::{
    BoxError, BypassTransfer, PacedBody, ProxyBody, PumpOutcome, PumpResult, TrackedBody,
    boxed_full, pump_body,
};
pub(crate) use upstream::UpstreamClient;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(60);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CLIENT_CONNECTIONS: usize = 256;
const BINARY_HEADER_PREFIX: &str = "\u{e000}proxy-crab-binary:v1:";
const CRAB_REQ_SPEED_TAG: &str = "_crab_req_speed";
const CRAB_RESP_SPEED_TAG: &str = "_crab_resp_speed";
const CRAB_RESP_BODYFRAME_TIMEOUT_TAG: &str = "_crab_resp_bodyframe_timeout";
const CRAB_REQ_TIMEOUT_TAG: &str = "_crab_req_timeout";
const CRAB_TLS_INSECURE_TAG: &str = "_crab_tls_insecure";

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
    connections: Mutex<Option<Arc<ConnectionRegistry>>>,
    task: Mutex<Option<JoinHandle<()>>>,
    operation: tokio::sync::Mutex<()>,
}

impl ProxyController {
    pub fn new() -> Self {
        Self {
            status: Arc::new(RwLock::new(ProxyStatus::Stopped)),
            cancellation: Mutex::new(None),
            connections: Mutex::new(None),
            task: Mutex::new(None),
            operation: tokio::sync::Mutex::new(()),
        }
    }

    pub fn status(&self) -> ProxyStatus {
        self.status
            .read()
            .expect("proxy status lock poisoned")
            .clone()
    }

    pub(crate) async fn mutate_sessions<T>(&self, action: impl FnOnce() -> Result<T>) -> Result<T> {
        let _operation = self.operation.lock().await;
        action()
    }

    pub(crate) async fn mutate_configuration<T>(
        &self,
        runtime: &ProxyCrab,
        action: impl FnOnce() -> Result<(T, bool)>,
    ) -> Result<T> {
        let _operation = self.operation.lock().await;
        let (value, changed) = action()?;
        if changed && matches!(self.status(), ProxyStatus::Running { .. }) {
            runtime.release_all_breakpoints();
            runtime.reset_upstream_client();
            let registry = self
                .connections
                .lock()
                .expect("proxy connections lock poisoned")
                .clone();
            if let Some(registry) = registry {
                let previous = registry.rotate();
                previous.cancellation.cancel();
                previous.tasks.shutdown(SHUTDOWN_TIMEOUT).await;
            }
        }
        Ok(value)
    }

    pub async fn start(&self, runtime: Arc<ProxyCrab>) -> Result<ProxyStatus> {
        let _operation = self.operation.lock().await;
        if !matches!(
            self.status(),
            ProxyStatus::Stopped | ProxyStatus::Failed { .. }
        ) {
            bail!("proxy is already running or changing state");
        }
        *self.status.write().expect("proxy status lock poisoned") = ProxyStatus::Starting;
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
        let connections = Arc::new(ConnectionRegistry::new());
        *self
            .cancellation
            .lock()
            .expect("proxy cancellation lock poisoned") = Some(cancellation.clone());
        *self
            .connections
            .lock()
            .expect("proxy connections lock poisoned") = Some(connections.clone());
        let status = self.status.clone();
        let host = config.proxy_host.clone();
        let port = config.proxy_port;
        *status.write().expect("proxy status lock poisoned") = ProxyStatus::Running {
            host: host.clone(),
            port,
        };
        tracing::info!("MITM proxy listening on {host}:{port}");

        let task = tokio::spawn(async move {
            accept_loop(listener, runtime, cancellation, connections).await;
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
        self.connections
            .lock()
            .expect("proxy connections lock poisoned")
            .take();
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
    capture_high_watermarks: HashMap<u64, (CaptureStore, u64)>,
    bypass_high_watermark: Option<(BypassStore, u64)>,
}

#[derive(Clone)]
struct ConnectionGeneration {
    cancellation: CancellationToken,
    tasks: TaskGroup,
}

struct ConnectionRegistry {
    generation: Mutex<ConnectionGeneration>,
}

impl ConnectionRegistry {
    fn new() -> Self {
        Self {
            generation: Mutex::new(ConnectionGeneration {
                cancellation: CancellationToken::new(),
                tasks: TaskGroup::new(),
            }),
        }
    }

    fn current(&self) -> ConnectionGeneration {
        self.generation
            .lock()
            .expect("proxy connection generation lock poisoned")
            .clone()
    }

    fn rotate(&self) -> ConnectionGeneration {
        let mut generation = self
            .generation
            .lock()
            .expect("proxy connection generation lock poisoned");
        generation.tasks.close();
        std::mem::replace(
            &mut *generation,
            ConnectionGeneration {
                cancellation: CancellationToken::new(),
                tasks: TaskGroup::new(),
            },
        )
    }

    async fn shutdown(&self, timeout_duration: Duration) {
        let generation = self.current();
        generation.cancellation.cancel();
        generation.tasks.shutdown(timeout_duration).await;
    }
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
                capture_high_watermarks: HashMap::new(),
                bypass_high_watermark: None,
            })),
        }
    }

    fn close(&self) {
        self.state
            .lock()
            .expect("proxy task state lock poisoned")
            .closing = true;
    }

    fn begin_capture(
        &self,
        store: &CaptureStore,
        begin: impl FnOnce() -> Result<u64>,
    ) -> Result<u64> {
        let mut state = self.state.lock().expect("proxy task state lock poisoned");
        if state.closing {
            bail!("proxy connection is closing");
        }
        let id = begin()?;
        state
            .capture_high_watermarks
            .entry(store.session_id())
            .and_modify(|(_, max_id)| *max_id = (*max_id).max(id))
            .or_insert((store.clone(), id));
        Ok(id)
    }

    fn begin_bypass(
        &self,
        store: &BypassStore,
        begin: impl FnOnce() -> Result<u64>,
    ) -> Result<u64> {
        let mut state = self.state.lock().expect("proxy task state lock poisoned");
        if state.closing {
            bail!("proxy connection is closing");
        }
        let id = begin()?;
        match &mut state.bypass_high_watermark {
            Some((_, max_id)) => *max_id = (*max_id).max(id),
            watermark @ None => *watermark = Some((store.clone(), id)),
        }
        Ok(id)
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
        self.close();
        self.tracker.close();
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
        let (capture_high_watermarks, bypass_high_watermark) = {
            let mut state = self.state.lock().expect("proxy task state lock poisoned");
            (
                std::mem::take(&mut state.capture_high_watermarks),
                state.bypass_high_watermark.take(),
            )
        };
        for (_, (store, max_id)) in capture_high_watermarks {
            let _ = store.mark_in_progress_through_as_shutdown(max_id);
        }
        if let Some((store, max_id)) = bypass_high_watermark {
            let _ = store.mark_in_progress_through_as_shutdown(max_id);
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
    connections: Arc<ConnectionRegistry>,
) {
    let connection_slots = Arc::new(Semaphore::new(MAX_CLIENT_CONNECTIONS));
    loop {
        tokio::select! {
            _ = cancellation.cancelled() => break,
            accepted = listener.accept() => match accepted {
                Ok((stream, address)) => {
                    let generation = connections.current();
                    match connection_slots.clone().try_acquire_owned() {
                        Ok(permit) => {
                            let client_runtime = runtime.clone();
                            let client_cancellation = generation.cancellation.clone();
                            let client_tracker = generation.tasks.clone();
                            generation.tasks.spawn(async move {
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
    connections.shutdown(SHUTDOWN_TIMEOUT).await;
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
        return bypass::handle_bypass_connect(
            request,
            bypass::BypassConnectContext {
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
            return bypass::handle_bypass_connect(
                request,
                bypass::BypassConnectContext {
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
    let capture = match begin_connect_capture(&tracker, &pin, source, &request_data) {
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
                mitm::process_connect(
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
            bypass::handle_bypass_http(
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
            mitm::handle_session_http_request(
                request,
                source,
                runtime,
                session.id,
                cancellation,
                tracker,
            )
            .await
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
    tracker: &TaskGroup,
    pin: &SessionPin,
    source: SocketAddr,
    request: &RequestData,
) -> Result<ConnectCapture> {
    let store = pin.store().clone();
    let id = tracker.begin_capture(&store, || {
        store.begin(&source.to_string(), request, "connect")
    })?;
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

fn note_capture_error(store: &CaptureStore, id: u64, stage: ErrorStage, kind: &str, message: &str) {
    if let Err(error) = store.note_error(
        id,
        &CaptureError {
            stage,
            kind: kind.into(),
            message: message.into(),
        },
    ) {
        tracing::error!("failed to record capture error: {error}");
    }
    tracing::warn!("{kind}: {message}");
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
    mitm::response_from_data(&response, runtime.certificate_pem().into_bytes(), false)
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

    use super::mitm::{parse_positive_decimal, parse_tls_insecure_tag, response_from_data};
    use super::{
        PacedBody, headers_to_values, normalize_tls_error, remove_header_value,
        strip_hop_by_hop_headers, values_to_headers, version_name,
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

    #[test]
    fn parses_tls_insecure_tag_only_for_explicit_true() {
        assert_eq!(parse_tls_insecure_tag(None), Ok(false));
        assert_eq!(parse_tls_insecure_tag(Some("true")), Ok(true));

        for invalid in ["", "false", "TRUE", "True", "1", " true", "true "] {
            assert_eq!(parse_tls_insecure_tag(Some(invalid)), Err(()), "{invalid}");
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
