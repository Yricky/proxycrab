use super::*;

enum PreparedBodyReplacement {
    Bytes(Bytes),
    File { file: tokio::fs::File, length: u64 },
}

impl PreparedBodyReplacement {
    fn length(&self) -> u64 {
        match self {
            Self::Bytes(bytes) => bytes.len() as u64,
            Self::File { length, .. } => *length,
        }
    }

    fn into_body(self) -> ProxyBody {
        match self {
            Self::Bytes(bytes) => boxed_full(bytes),
            Self::File { file, .. } => StreamBody::new(
                ReaderStream::new(file)
                    .map_ok(hyper::body::Frame::data)
                    .map_err(|error| Box::new(error) as BoxError),
            )
            .boxed_unsync(),
        }
    }
}

async fn prepare_body_replacement(
    replacement: &BodyReplacement,
) -> Result<PreparedBodyReplacement> {
    match replacement {
        BodyReplacement::String(content) => Ok(PreparedBodyReplacement::Bytes(
            Bytes::copy_from_slice(content.as_bytes()),
        )),
        BodyReplacement::Asset(asset) => {
            let path = asset.path();
            let file = tokio::fs::File::open(path).await.map_err(|error| {
                anyhow::anyhow!("failed to open asset {}: {error}", asset.metadata.id)
            })?;
            let metadata = file.metadata().await.map_err(|error| {
                anyhow::anyhow!("failed to inspect asset {}: {error}", asset.metadata.id)
            })?;
            if !metadata.is_file() {
                bail!("asset is not a file: {}", asset.metadata.id);
            }
            Ok(PreparedBodyReplacement::File {
                file,
                length: metadata.len(),
            })
        }
    }
}

pub(super) async fn process_connect<C>(
    client: C,
    authority: hyper::http::uri::Authority,
    source: SocketAddr,
    pin: SessionPin,
    mut capture: ConnectCapture,
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
        tunnel_connect(client, &authority, &mut capture, cancellation).await;
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
    capture.finish_activity();
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

pub(super) async fn handle_session_http_request(
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
    let (capture_id, activity) = match tracker.begin_capture(&store, || {
        store.begin(&source.to_string(), &request_data, "request")
    }) {
        Ok(record) => record,
        Err(error) => {
            tracing::error!("capture insert failed; refusing to forward request: {error}");
            return text_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "capture storage unavailable",
            );
        }
    };
    let raw_request_writer = match store
        .create_body_writer(capture_id, BodySide::Request)
        .await
    {
        Ok(writer) => writer,
        Err(error) => {
            fail_capture(
                &store,
                capture_id,
                ErrorStage::RequestBody,
                "request_body_store_failed",
                &error.to_string(),
            );
            return text_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "capture storage unavailable",
            );
        }
    };
    let raw_request = DeferredBody::new(incoming, raw_request_writer, &tracker);
    let raw_request_reader = raw_request.reader();
    let mut request_replacement = None;
    let mut request_replacement_source = None;
    for (position, script) in interceptor_snapshot.request.iter().enumerate() {
        let state = SharedInterceptorState::new_request(
            request_data.method.clone(),
            request_data.uri.clone(),
            request_data.headers.clone(),
            request_data.tags.clone(),
        );
        state.set_body(request_replacement_source.clone());
        state.set_raw_body(raw_request_reader.clone());
        state.set_asset_store(runtime.asset_store());
        let journal =
            ModificationJournal::for_request(&request_data, request_replacement_source.as_ref());
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
                let body_changed = effects.body != request_replacement_source;
                if body_changed && let Some(replacement) = effects.body {
                    match prepare_body_replacement(&replacement).await {
                        Ok(body) => {
                            request_replacement = Some(body);
                            request_replacement_source = Some(replacement);
                            remove_header_value(&mut request_data.headers, "content-encoding");
                        }
                        Err(error) => {
                            let message = error.to_string();
                            note_script_error(&store, capture_id, &script.name, &message);
                            append_run_error(&mut run_error, message);
                        }
                    }
                }
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
    let request_final_body = request_replacement_source
        .as_ref()
        .map(BodyReplacement::source_type)
        .unwrap_or_default();
    if let Err(error) = store.update_request(capture_id, &request_data, &request_final_body) {
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
    let local_response =
        is_ca_download(&request_data) || request_data.tags.contains_key("_crab_skip");
    let forward_original = request_replacement.is_none() && !local_response;
    let (original_request_body, raw_request_done) = raw_request.finish(
        forward_original,
        forward_original.then_some(request_speed).flatten(),
        None,
    );

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
        let _ = store.save_body(capture_id, BodySide::Response, &bytes);
        let _ = store.complete(capture_id, &response_data, &BodySourceType::Original);
        return response_from_data(&response_data, bytes, false);
    }

    if request_data.tags.contains_key("_crab_skip") {
        return finish_session_response(
            SessionResponseContext {
                runtime,
                store,
                session_id: pin.session_id(),
                capture_id,
                activity,
                raw_request_done,
                modified_request_done: None,
                tracker: tracker.clone(),
            },
            request_data,
            ResponseData {
                status: 200,
                version: "HTTP/1.1".into(),
                headers: HeaderValues::new(),
            },
            boxed_full(Bytes::new()),
            &interceptor_snapshot.response,
        )
        .await;
    }

    let tls_insecure = tls_insecure_from_tags(&request_data.tags, capture_id);
    let (request_body, replacement_length, modified_request_done) = match request_replacement {
        Some(replacement) => {
            let length = replacement.length();
            let (body, done) = pump_body(
                replacement.into_body(),
                None,
                None,
                request_speed,
                true,
                &tracker,
            );
            (
                body.expect("replacement request body is forwarded"),
                Some(length),
                Some(done),
            )
        }
        None => (
            original_request_body.expect("forwarded original request has a body"),
            None,
            None,
        ),
    };
    let upstream_request = match request_from_data(&request_data, request_body, replacement_length)
    {
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
        runtime
            .upstream_client()
            .send(upstream_request, tls_insecure, cancellation.clone()),
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
        let _ = store.complete(capture_id, &response_data, &BodySourceType::Original);
        return response_from_data(&response_data, Vec::new(), false);
    }

    let response_body = upstream_response
        .into_body()
        .map_err(|error| Box::new(error) as BoxError)
        .boxed_unsync();
    finish_session_response(
        SessionResponseContext {
            runtime,
            store,
            session_id: pin.session_id(),
            capture_id,
            activity,
            raw_request_done,
            modified_request_done,
            tracker: tracker.clone(),
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
    activity: ActivityGuard,
    raw_request_done: tokio::sync::oneshot::Receiver<PumpResult>,
    modified_request_done: Option<tokio::sync::oneshot::Receiver<PumpResult>>,
    tracker: TaskGroup,
}

async fn finish_session_response(
    context: SessionResponseContext,
    mut request_data: RequestData,
    mut response_data: ResponseData,
    raw_response_body: ProxyBody,
    scripts: &[InterceptorSnapshot],
) -> Response<ProxyBody> {
    let SessionResponseContext {
        runtime,
        store,
        session_id,
        capture_id,
        activity,
        raw_request_done,
        modified_request_done,
        tracker,
    } = context;
    if let Err(error) = store.update_response(capture_id, &response_data, &BodySourceType::Original)
    {
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
    let raw_response_writer = match store
        .create_body_writer(capture_id, BodySide::Response)
        .await
    {
        Ok(writer) => writer,
        Err(error) => {
            fail_capture(
                &store,
                capture_id,
                ErrorStage::ResponseBody,
                "response_body_store_failed",
                &error.to_string(),
            );
            return text_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "capture storage unavailable",
            );
        }
    };
    let raw_response = DeferredBody::new(raw_response_body, raw_response_writer, &tracker);
    let raw_response_reader = raw_response.reader();

    let mut response_replacement = None;
    let mut response_replacement_source = None;
    for (position, script) in scripts.iter().enumerate() {
        let state = SharedInterceptorState::new_response(
            response_data.status,
            response_data.headers.clone(),
            request_data.tags.clone(),
        );
        state.set_body(response_replacement_source.clone());
        state.set_raw_body(raw_response_reader.clone());
        state.set_asset_store(runtime.asset_store());
        let journal =
            ModificationJournal::for_response(&response_data, response_replacement_source.as_ref());
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
                let body_changed = effects.body != response_replacement_source;
                if body_changed && let Some(replacement) = effects.body {
                    match prepare_body_replacement(&replacement).await {
                        Ok(body) => {
                            response_replacement = Some(body);
                            response_replacement_source = Some(replacement);
                            remove_header_value(&mut response_data.headers, "content-encoding");
                        }
                        Err(error) => {
                            let message = error.to_string();
                            note_script_error(&store, capture_id, &script.name, &message);
                            append_run_error(&mut run_error, message);
                        }
                    }
                }
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
    let response_final_body = response_replacement_source
        .as_ref()
        .map(BodyReplacement::source_type)
        .unwrap_or_default();
    if let Err(error) = store.update_response(capture_id, &response_data, &response_final_body) {
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
    let response_speed = parse_special_tag(&request_data.tags, CRAB_RESP_SPEED_TAG, capture_id);
    let response_frame_timeout = parse_special_tag(
        &request_data.tags,
        CRAB_RESP_BODYFRAME_TIMEOUT_TAG,
        capture_id,
    )
    .map(Duration::from_millis);
    let head_response = request_data.method.eq_ignore_ascii_case("HEAD");
    let replacement_used = response_replacement.is_some();
    let forward_raw = !replacement_used && !head_response;
    let (forwarded_response_body, raw_response_done) = raw_response.finish(
        forward_raw,
        forward_raw.then_some(response_speed).flatten(),
        response_frame_timeout,
    );

    let (replacement_response_body, modified_response_done) = match response_replacement {
        Some(replacement) => {
            let length = replacement.length();
            let (body, done) = pump_body(
                replacement.into_body(),
                None,
                None,
                response_speed,
                true,
                &tracker,
            );
            (body.map(|body| (body, length)), Some(done))
        }
        None => (None, None),
    };

    let final_store = store.clone();
    let final_response = response_data.clone();
    tracker.spawn(async move {
        let _activity = activity;
        let request_result = raw_request_done.await.ok();
        let modified_request_result = match modified_request_done {
            Some(done) => done.await.ok(),
            None => None,
        };
        let response_result = raw_response_done.await.ok();
        let modified_response_result = match modified_response_done {
            Some(done) => done.await.ok(),
            None => None,
        };
        if let Some(result) = &request_result {
            if let Some(error) = &result.storage_error {
                note_capture_error(
                    &final_store,
                    capture_id,
                    ErrorStage::RequestBody,
                    "request_body_store_failed",
                    error,
                );
            }
            if let PumpOutcome::InputError(error) = &result.outcome {
                note_capture_error(
                    &final_store,
                    capture_id,
                    ErrorStage::RequestBody,
                    "request_body_read_failed",
                    error,
                );
            }
        }
        if let Some(result) = &modified_request_result {
            if let Some(error) = &result.storage_error {
                note_capture_error(
                    &final_store,
                    capture_id,
                    ErrorStage::RequestBody,
                    "modified_request_body_store_failed",
                    error,
                );
            }
            if let PumpOutcome::InputError(error) = &result.outcome {
                note_capture_error(
                    &final_store,
                    capture_id,
                    ErrorStage::RequestBody,
                    "modified_request_body_read_failed",
                    error,
                );
            }
        }
        let Some(response_result) = response_result else {
            fail_capture(
                &final_store,
                capture_id,
                ErrorStage::ResponseBody,
                "response_body_task_cancelled",
                "response body capture task was cancelled",
            );
            return;
        };
        if replacement_used {
            let Some(modified_result) = modified_response_result else {
                fail_capture(
                    &final_store,
                    capture_id,
                    ErrorStage::ResponseBody,
                    "modified_response_body_task_cancelled",
                    "modified response body task was cancelled",
                );
                return;
            };
            if let Some(error) = &modified_result.storage_error {
                note_capture_error(
                    &final_store,
                    capture_id,
                    ErrorStage::ResponseBody,
                    "modified_response_body_store_failed",
                    error,
                );
            }
            match &modified_result.outcome {
                PumpOutcome::Complete => {}
                PumpOutcome::InputError(error) => {
                    fail_capture(
                        &final_store,
                        capture_id,
                        ErrorStage::ResponseBody,
                        "modified_response_body_read_failed",
                        error,
                    );
                    return;
                }
                PumpOutcome::OutputClosed => {
                    fail_capture(
                        &final_store,
                        capture_id,
                        ErrorStage::ResponseBody,
                        "downstream_response_closed",
                        "downstream response body closed before replacement completed",
                    );
                    return;
                }
                PumpOutcome::FrameTimeout => unreachable!("replacement bodies have no timeout"),
            }
            if let Some(error) = &response_result.storage_error {
                note_capture_error(
                    &final_store,
                    capture_id,
                    ErrorStage::ResponseBody,
                    "raw_response_body_store_failed",
                    error,
                );
            }
            match &response_result.outcome {
                PumpOutcome::Complete => {}
                PumpOutcome::FrameTimeout => note_capture_error(
                    &final_store,
                    capture_id,
                    ErrorStage::ResponseBody,
                    "raw_response_body_timeout",
                    "upstream response body frame timed out",
                ),
                PumpOutcome::InputError(error) => note_capture_error(
                    &final_store,
                    capture_id,
                    ErrorStage::ResponseBody,
                    "raw_response_body_read_failed",
                    error,
                ),
                PumpOutcome::OutputClosed => {}
            }
            let _ = final_store.complete(capture_id, &final_response, &response_final_body);
            return;
        }
        if let Some(error) = &response_result.storage_error {
            fail_capture(
                &final_store,
                capture_id,
                ErrorStage::ResponseBody,
                "response_body_store_failed",
                error,
            );
            return;
        }
        match &response_result.outcome {
            PumpOutcome::Complete => {
                let _ = final_store.complete(capture_id, &final_response, &response_final_body);
            }
            PumpOutcome::FrameTimeout => fail_capture(
                &final_store,
                capture_id,
                ErrorStage::ResponseBody,
                "response_body_timeout",
                "upstream response body frame timed out",
            ),
            PumpOutcome::InputError(error) => fail_capture(
                &final_store,
                capture_id,
                ErrorStage::ResponseBody,
                "response_body_read_failed",
                error,
            ),
            PumpOutcome::OutputClosed => fail_capture(
                &final_store,
                capture_id,
                ErrorStage::ResponseBody,
                "downstream_response_closed",
                "downstream response body closed before completion",
            ),
        }
    });

    match replacement_response_body {
        Some((body, length)) => {
            response_from_stream(&response_data, body, Some(length), head_response)
        }
        None if head_response => {
            response_from_stream(&response_data, boxed_full(Bytes::new()), None, true)
        }
        None => response_from_stream(
            &response_data,
            forwarded_response_body.expect("forwarded response body is available"),
            None,
            false,
        ),
    }
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
    capture: &mut ConnectCapture,
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
    capture.finish_activity();
    tokio::select! {
        result = tokio::io::copy_bidirectional(&mut client, &mut upstream) => {
            if let Err(error) = result {
                tracing::warn!("CONNECT tunnel for {authority} failed: {error}");
            }
        }
        _ = cancellation.cancelled() => {}
    }
}

fn request_from_data(
    data: &RequestData,
    body: ProxyBody,
    replacement_length: Option<u64>,
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
    if let Some(length) = replacement_length {
        headers.insert(CONTENT_LENGTH, HeaderValue::from_str(&length.to_string())?);
    }
    Ok(builder.body(body)?)
}

pub(super) fn response_from_data(
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

fn response_from_stream(
    data: &ResponseData,
    body: ProxyBody,
    replacement_length: Option<u64>,
    preserve_content_length: bool,
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
            && let Some(length) = replacement_length
            && let Ok(value) = HeaderValue::from_str(&length.to_string())
        {
            headers.insert(CONTENT_LENGTH, value);
        }
    }
    builder
        .body(body)
        .unwrap_or_else(|_| text_response(StatusCode::INTERNAL_SERVER_ERROR, "invalid response"))
}

pub(super) fn parse_positive_decimal(value: Option<&str>) -> Result<Option<u64>, ()> {
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

pub(super) fn parse_tls_insecure_tag(value: Option<&str>) -> Result<bool, ()> {
    match value {
        None => Ok(false),
        Some("true") => Ok(true),
        Some(_) => Err(()),
    }
}

fn tls_insecure_from_tags(tags: &RequestTags, capture_id: u64) -> bool {
    match parse_tls_insecure_tag(tags.get(CRAB_TLS_INSECURE_TAG).map(String::as_str)) {
        Ok(value) => value,
        Err(()) => {
            tracing::warn!(
                capture_id,
                tag = CRAB_TLS_INSECURE_TAG,
                "ignoring invalid special tag value"
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::asset::AssetStore;

    use super::{BodyReplacement, PreparedBodyReplacement, prepare_body_replacement};

    #[tokio::test]
    async fn prepares_a_replacement_file_larger_than_the_old_limit_without_reading_it() {
        let root = tempfile::TempDir::new().unwrap();
        let store = AssetStore::open(root.path()).unwrap();
        let length = 65 * 1024 * 1024;
        let mut upload = store
            .begin_upload("large.bin", "application/octet-stream".into())
            .await
            .unwrap();
        upload.write(&vec![0; 1024 * 1024]).await.unwrap();
        upload.finish().await.unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(store.get("large.bin").unwrap().unwrap().path())
            .unwrap()
            .set_len(length)
            .unwrap();
        let asset = store.get("large.bin").unwrap().unwrap();

        let replacement = prepare_body_replacement(&BodyReplacement::Asset(asset))
            .await
            .unwrap();

        assert!(matches!(
            replacement,
            PreparedBodyReplacement::File {
                length: actual,
                ..
            } if actual == length
        ));
    }
}
