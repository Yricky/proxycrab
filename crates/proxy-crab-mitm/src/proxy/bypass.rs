use super::*;

pub(super) struct BypassConnectContext {
    pub(super) request_data: RequestData,
    pub(super) source: SocketAddr,
    pub(super) authority: hyper::http::uri::Authority,
    pub(super) reason: &'static str,
    pub(super) runtime: Arc<ProxyCrab>,
    pub(super) cancellation: CancellationToken,
    pub(super) tracker: TaskGroup,
}

pub(super) fn handle_bypass_connect(
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
    let entry = tracker
        .begin_bypass(runtime.bypass_store(), || {
            runtime.bypass_store().begin(
                &source.to_string(),
                &request_data.method,
                &request_data.uri,
                &request_data.version,
                reason,
            )
        })
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
                if let Some((id, _)) = &entry {
                    let _ = runtime
                        .bypass_store()
                        .fail(*id, &error.to_string(), None, None);
                }
                return;
            }
        };
        let mut upstream =
            match timeout(CONNECT_TIMEOUT, TcpStream::connect(authority.as_str())).await {
                Ok(Ok(upstream)) => upstream,
                Ok(Err(error)) => {
                    if let Some((id, _)) = &entry {
                        let _ = runtime
                            .bypass_store()
                            .fail(*id, &error.to_string(), None, None);
                    }
                    return;
                }
                Err(_) => {
                    if let Some((id, _)) = &entry {
                        let _ = runtime.bypass_store().fail(
                            *id,
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
                        if let Some((id, _)) = &entry {
                            let _ = runtime.bypass_store().complete(
                                *id,
                                None,
                                Some(upload),
                                Some(download),
                            );
                        }
                    }
                    Err(error) => {
                        if let Some((id, _)) = &entry {
                            let _ = runtime.bypass_store().fail(
                                *id,
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

pub(super) async fn handle_bypass_http(
    mut request: Request<Incoming>,
    request_data: RequestData,
    source: SocketAddr,
    reason: &'static str,
    runtime: Arc<ProxyCrab>,
    cancellation: CancellationToken,
    tracker: TaskGroup,
) -> Response<ProxyBody> {
    let entry = tracker
        .begin_bypass(runtime.bypass_store(), || {
            runtime.bypass_store().begin(
                &source.to_string(),
                &request_data.method,
                &request_data.uri,
                &request_data.version,
                reason,
            )
        })
        .map_err(|error| tracing::warn!("failed to persist bypass request: {error}"))
        .ok();
    let (entry_id, activity) = match entry {
        Some((id, activity)) => (Some(id), Some(activity)),
        None => (None, None),
    };
    let transfer = BypassTransfer::new(runtime.bypass_store().clone(), entry_id, activity);
    let downstream_upgrade = is_upgrade_request(&request).then(|| hyper::upgrade::on(&mut request));
    let upstream_request = match streaming_upstream_request(request, transfer.clone()) {
        Ok(request) => request,
        Err(error) => {
            transfer.fail(&error.to_string());
            return text_response(StatusCode::BAD_REQUEST, "invalid upstream request");
        }
    };
    let mut upstream_response = match runtime
        .upstream_client()
        .send(upstream_request, false, cancellation.clone())
        .await
    {
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
        return mitm::response_from_data(&response_data, Vec::new(), false);
    }
    let (mut parts, incoming) = upstream_response.into_parts();
    strip_hop_by_hop_headers(&mut parts.headers, false);
    parts.headers.remove(TRANSFER_ENCODING);
    let body = TrackedBody::response(incoming, transfer, response_data.status).boxed_unsync();
    Response::from_parts(parts, body)
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
