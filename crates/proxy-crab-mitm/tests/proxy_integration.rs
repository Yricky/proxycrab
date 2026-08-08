use std::{
    convert::Infallible,
    io::Cursor,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Request, Response, Version, server::conn::http1, service::service_fn};
use hyper_util::rt::{TokioExecutor, TokioIo};
use proxy_crab_mitm::{
    ProxyCrab,
    bypass::BypassOutcome,
    log_buffer::LogBuffer,
    model::{
        BodyPayload, BreakpointListFilter, CaptureOutcome, ErrorStage, InterceptorExecutionOrigin,
        InterceptorKind, Modification, ProxyStatus, Script, ScriptKind, SessionInterceptor,
        SessionInterceptors,
    },
    storage::{BodySide, BodySourceData},
};
use rcgen::{CertifiedKey, generate_simple_self_signed};
use rustls::{
    ClientConfig, RootCertStore, ServerConfig,
    pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer, ServerName},
};
use tempfile::tempdir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc,
    time::{sleep, timeout},
};
use tokio_rustls::{TlsAcceptor, TlsConnector};

fn unused_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

async fn runtime() -> (tempfile::TempDir, Arc<ProxyCrab>, u16) {
    let app_data = tempdir().unwrap();
    let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
    let port = unused_port();
    let mut config = runtime.config();
    config.proxy_host = "127.0.0.1".into();
    config.proxy_port = port;
    runtime.replace_config(config).await.unwrap();
    runtime.start_proxy().await.unwrap();
    (app_data, runtime, port)
}

async fn fixed_http_upstream() -> (u16, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut request = vec![0; 4096];
                let _ = stream.read(&mut request).await;
                let _ = stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                    )
                    .await;
            });
        }
    });
    (port, task)
}

async fn keepalive_counting_upstream() -> (u16, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let accepted = Arc::new(AtomicUsize::new(0));
    let task_accepted = accepted.clone();
    let task = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            task_accepted.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let service = service_fn(|_| async {
                    Ok::<_, Infallible>(Response::new(Full::new(Bytes::from_static(b"ok"))))
                });
                let _ = http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    (port, accepted, task)
}

async fn recording_http_upstream() -> (
    u16,
    tokio::sync::oneshot::Receiver<String>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (request_tx, request_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        loop {
            let mut bytes = [0; 1024];
            let count = stream.read(&mut bytes).await.unwrap();
            if count == 0 {
                break;
            }
            request.extend_from_slice(&bytes[..count]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let _ = request_tx.send(String::from_utf8_lossy(&request).into_owned());
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
            )
            .await
            .unwrap();
    });
    (port, request_rx, task)
}

async fn staged_http_upstream() -> (
    u16,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (release, wait_for_release) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        let _ = stream.read(&mut request).await;
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 10\r\nConnection: close\r\n\r\nhello",
            )
            .await
            .unwrap();
        wait_for_release.await.unwrap();
        stream.write_all(b"world").await.unwrap();
    });
    (port, release, task)
}

async fn delayed_response_frame_upstream(delay: Duration) -> (u16, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        let _ = stream.read(&mut request).await;
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 10\r\nConnection: close\r\n\r\nhello",
            )
            .await
            .unwrap();
        sleep(delay).await;
        let _ = stream.write_all(b"world").await;
    });
    (port, task)
}

async fn delayed_first_response_frame_upstream(
    delay: Duration,
) -> (u16, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        let _ = stream.read(&mut request).await;
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 5\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        sleep(delay).await;
        let _ = stream.write_all(b"hello").await;
    });
    (port, task)
}

#[derive(Debug)]
struct UploadObservation {
    body: Vec<u8>,
    first_body_after: Option<Duration>,
    complete_after: Option<Duration>,
}

async fn body_gated_http_upstream() -> (
    u16,
    tokio::sync::oneshot::Receiver<UploadObservation>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (observation_tx, observation_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let started_at = Instant::now();
        let mut request = Vec::new();
        let mut header_end = None;
        let mut content_length = None;
        let mut first_body_after = None;
        loop {
            let mut bytes = [0; 1024];
            let count = stream.read(&mut bytes).await.unwrap();
            if count == 0 {
                break;
            }
            request.extend_from_slice(&bytes[..count]);
            if header_end.is_none()
                && let Some(position) = request.windows(4).position(|window| window == b"\r\n\r\n")
            {
                let end = position + 4;
                let headers = String::from_utf8_lossy(&request[..end]);
                content_length = headers.lines().find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                });
                header_end = Some(end);
            }
            if let Some(end) = header_end {
                let body_len = request.len() - end;
                if body_len > 0 && first_body_after.is_none() {
                    first_body_after = Some(started_at.elapsed());
                }
                if content_length.is_some_and(|length| body_len >= length) {
                    break;
                }
            }
        }
        let end = header_end.unwrap_or(request.len());
        let expected = content_length.unwrap_or(0);
        let body = request[end..]
            .get(..expected.min(request.len() - end))
            .unwrap_or_default()
            .to_vec();
        let complete_after = (body.len() == expected).then(|| started_at.elapsed());
        let completed = complete_after.is_some();
        let _ = observation_tx.send(UploadObservation {
            body,
            first_body_after,
            complete_after,
        });
        if completed {
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                )
                .await
                .unwrap();
        }
    });
    (port, observation_rx, task)
}

async fn proxy_get(proxy_port: u16, uri: &str, host: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
    stream
        .write_all(
            format!(
                "GET {uri} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut response = Vec::new();
    timeout(Duration::from_secs(3), stream.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();
    String::from_utf8_lossy(&response).into_owned()
}

async fn proxy_https_get(
    runtime: &ProxyCrab,
    proxy_port: u16,
    authority: &str,
    path: &str,
) -> String {
    let mut roots = RootCertStore::empty();
    for certificate in rustls_pemfile::certs(&mut Cursor::new(runtime.certificate_pem())) {
        roots.add(certificate.unwrap()).unwrap();
    }
    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(config));
    let stream = connect_tunnel_to(proxy_port, authority).await;
    let server_name =
        ServerName::try_from(authority.split(':').next().unwrap().to_string()).unwrap();
    let mut tls = connector.connect(server_name, stream).await.unwrap();
    tls.write_all(
        format!(
            "GET {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
        )
        .as_bytes(),
    )
    .await
    .unwrap();
    let mut response = Vec::new();
    timeout(Duration::from_secs(3), tls.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();
    String::from_utf8_lossy(&response).into_owned()
}

async fn self_signed_https_upstream() -> (
    u16,
    mpsc::UnboundedReceiver<hyper::HeaderMap>,
    tokio::task::JoinHandle<()>,
) {
    let CertifiedKey { cert, signing_key } =
        generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone()],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(signing_key.serialize_der())),
        )
        .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (request_tx, request_rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let acceptor = acceptor.clone();
            let request_tx = request_tx.clone();
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(stream).await else {
                    return;
                };
                let service = service_fn(move |request: Request<hyper::body::Incoming>| {
                    let _ = request_tx.send(request.headers().clone());
                    async {
                        Ok::<_, Infallible>(Response::new(Full::new(Bytes::from_static(b"ok"))))
                    }
                });
                let _ = http1::Builder::new()
                    .serve_connection(TokioIo::new(tls), service)
                    .await;
            });
        }
    });
    (port, request_rx, task)
}

async fn proxy_post(proxy_port: u16, uri: &str, host: &str, body: &[u8]) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
    stream
        .write_all(
            format!(
                "POST {uri} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
                body.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    stream.write_all(body).await.unwrap();
    let mut response = Vec::new();
    timeout(Duration::from_secs(3), stream.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();
    String::from_utf8_lossy(&response).into_owned()
}

async fn proxy_get_body_timing(
    proxy_port: u16,
    uri: &str,
    host: &str,
    body_len: usize,
) -> (String, Vec<u8>, Duration, Duration) {
    let mut stream = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
    stream
        .write_all(
            format!(
                "GET {uri} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut headers = Vec::new();
    while !headers.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        timeout(Duration::from_secs(3), stream.read_exact(&mut byte))
            .await
            .unwrap()
            .unwrap();
        headers.push(byte[0]);
    }
    let headers_at = Instant::now();
    let mut body = vec![0; body_len];
    timeout(Duration::from_secs(3), stream.read_exact(&mut body[..1]))
        .await
        .unwrap()
        .unwrap();
    let first_body_after = headers_at.elapsed();
    timeout(Duration::from_secs(3), stream.read_exact(&mut body[1..]))
        .await
        .unwrap()
        .unwrap();
    let complete_after = headers_at.elapsed();
    (
        String::from_utf8_lossy(&headers).into_owned(),
        body,
        first_body_after,
        complete_after,
    )
}

async fn connect_tunnel(proxy_port: u16) -> TcpStream {
    connect_tunnel_to(proxy_port, "proxy.crab:443").await
}

async fn connect_tunnel_to(proxy_port: u16, authority: &str) -> TcpStream {
    let mut stream = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
    stream
        .write_all(format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut response = Vec::new();
    loop {
        let mut byte = [0];
        stream.read_exact(&mut byte).await.unwrap();
        response.push(byte[0]);
        if response.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    assert!(String::from_utf8_lossy(&response).starts_with("HTTP/1.1 200"));
    stream
}

async fn echo_upstream() -> (u16, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut bytes = [0; 4];
        stream.read_exact(&mut bytes).await.unwrap();
        stream.write_all(&bytes).await.unwrap();
    });
    (port, task)
}

async fn upgrade_echo_upstream() -> (u16, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        loop {
            let mut byte = [0];
            stream.read_exact(&mut byte).await.unwrap();
            request.push(byte[0]);
            if request.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        stream
            .write_all(
                b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n",
            )
            .await
            .unwrap();
        let mut bytes = [0; 4];
        stream.read_exact(&mut bytes).await.unwrap();
        stream.write_all(&bytes).await.unwrap();
    });
    (port, task)
}

#[tokio::test]
async fn captures_plain_http_and_hot_active_session_switches() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let (upstream_port, upstream) = fixed_http_upstream().await;
    let first = runtime.create_session(None, None).unwrap();

    let response = proxy_get(
        proxy_port,
        &format!("http://127.0.0.1:{upstream_port}/first"),
        &format!("127.0.0.1:{upstream_port}"),
    )
    .await;
    assert!(response.contains("\r\n\r\nok"));

    let second = runtime.create_session(Some("second".into()), None).unwrap();
    runtime
        .replace_active_session(Some(second.id))
        .await
        .unwrap();
    let response = proxy_get(
        proxy_port,
        &format!("http://127.0.0.1:{upstream_port}/second"),
        &format!("127.0.0.1:{upstream_port}"),
    )
    .await;
    assert!(response.contains("\r\n\r\nok"));

    assert_eq!(runtime.list_captures(first.id, 10, None).unwrap().len(), 1);
    assert_eq!(runtime.list_captures(second.id, 10, None).unwrap().len(), 1);
    runtime.stop_proxy().await.unwrap();
    upstream.abort();
}

#[tokio::test]
async fn forwards_plain_http_to_bypass_when_no_active_session_exists() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let (upstream_port, upstream) = fixed_http_upstream().await;

    let response = proxy_get(
        proxy_port,
        &format!("http://127.0.0.1:{upstream_port}/bypass"),
        &format!("127.0.0.1:{upstream_port}"),
    )
    .await;

    assert!(response.contains("\r\n\r\nok"));
    assert!(runtime.sessions().is_empty());
    let entries = runtime.bypass_entries(10, None).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].reason, "no_active_session");
    assert_eq!(entries[0].outcome, BypassOutcome::Success);
    assert_eq!(entries[0].response_status, Some(200));
    runtime.stop_proxy().await.unwrap();
    upstream.abort();
}

#[tokio::test]
async fn streams_plain_http_bypass_before_the_upstream_response_finishes() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let (upstream_port, release, upstream) = staged_http_upstream().await;
    let mut stream = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
    stream
        .write_all(
            format!(
                "GET http://127.0.0.1:{upstream_port}/stream HTTP/1.1\r\n\
                 Host: 127.0.0.1:{upstream_port}\r\n\
                 Connection: close\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();

    let mut response = Vec::new();
    timeout(Duration::from_secs(1), async {
        let mut chunk = [0; 1024];
        while !response.windows(5).any(|window| window == b"hello") {
            let read = stream.read(&mut chunk).await.unwrap();
            assert_ne!(read, 0, "response ended before the first body chunk");
            response.extend_from_slice(&chunk[..read]);
        }
    })
    .await
    .expect("the proxy buffered the bypass response instead of streaming it");

    release.send(()).unwrap();
    timeout(Duration::from_secs(2), stream.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert!(response.windows(10).any(|window| window == b"helloworld"));
    upstream.await.unwrap();

    timeout(Duration::from_secs(1), async {
        loop {
            let entries = runtime.bypass_entries(10, None).unwrap();
            if entries
                .first()
                .is_some_and(|entry| entry.outcome == BypassOutcome::Success)
            {
                assert_eq!(entries[0].download_bytes, Some(10));
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    runtime.stop_proxy().await.unwrap();
}

#[tokio::test]
async fn streams_session_response_before_the_upstream_response_finishes() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let session = runtime.create_session(None, None).unwrap();
    let (upstream_port, release, upstream) = staged_http_upstream().await;
    let mut stream = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
    stream
        .write_all(
            format!(
                "GET http://127.0.0.1:{upstream_port}/session-stream HTTP/1.1\r\n\
                 Host: 127.0.0.1:{upstream_port}\r\n\
                 Connection: close\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();

    let mut response = Vec::new();
    timeout(Duration::from_secs(1), async {
        let mut chunk = [0; 1024];
        while !response.windows(5).any(|window| window == b"hello") {
            let read = stream.read(&mut chunk).await.unwrap();
            assert_ne!(read, 0, "response ended before the first body chunk");
            response.extend_from_slice(&chunk[..read]);
        }
    })
    .await
    .expect("the Session buffered the response instead of streaming it");

    release.send(()).unwrap();
    timeout(Duration::from_secs(2), stream.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert!(response.windows(10).any(|window| window == b"helloworld"));
    upstream.await.unwrap();

    let capture = timeout(Duration::from_secs(1), async {
        loop {
            let capture = runtime.list_captures(session.id, 1, None).unwrap()[0].clone();
            if capture.outcome == CaptureOutcome::Success {
                break capture;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let source = runtime
        .capture_body_source(session.id, capture.id, BodySide::Response)
        .unwrap()
        .unwrap();
    let BodySourceData::File(path) = source.data else {
        panic!("captured response body was not stored in a file");
    };
    assert_eq!(tokio::fs::read(path).await.unwrap(), b"helloworld");
    runtime.stop_proxy().await.unwrap();
}

#[tokio::test]
async fn response_replacement_does_not_wait_for_the_raw_upstream_body() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let session = runtime.create_session(None, None).unwrap();
    let (upstream_port, release, upstream) = staged_http_upstream().await;
    runtime
        .create_script(
            ScriptKind::ResponseInterceptor,
            Script {
                name: "replace-stream".into(),
                content: "resp.body:replace_with_string('replacement')".into(),
            },
        )
        .unwrap();
    runtime
        .replace_session_interceptors(
            session.id,
            SessionInterceptors {
                request: Vec::new(),
                response: vec![SessionInterceptor {
                    name: "replace-stream".into(),
                    enabled: true,
                }],
            },
        )
        .unwrap();

    let request = tokio::spawn(async move {
        proxy_get(
            proxy_port,
            &format!("http://127.0.0.1:{upstream_port}/replace-stream"),
            &format!("127.0.0.1:{upstream_port}"),
        )
        .await
    });
    let response = timeout(Duration::from_secs(1), request)
        .await
        .expect("replacement waited for the raw upstream body")
        .unwrap();
    assert!(response.ends_with("replacement"), "{response}");

    release.send(()).unwrap();
    upstream.await.unwrap();
    let capture = timeout(Duration::from_secs(1), async {
        loop {
            let capture = runtime.list_captures(session.id, 1, None).unwrap()[0].clone();
            if capture.outcome == CaptureOutcome::Success {
                break capture;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let raw_path = runtime
        .workspace()
        .session_dir(session.id)
        .join("blob")
        .join(format!("{}-response.body", capture.id));
    let modified_path = runtime
        .workspace()
        .session_dir(session.id)
        .join("blob")
        .join(format!("{}-response.body.modified", capture.id));
    assert_eq!(tokio::fs::read(raw_path).await.unwrap(), b"helloworld");
    assert_eq!(
        tokio::fs::read(modified_path).await.unwrap(),
        b"replacement"
    );
    runtime.stop_proxy().await.unwrap();
}

#[tokio::test]
async fn request_replacement_is_sent_while_the_raw_client_body_keeps_draining() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let session = runtime.create_session(None, None).unwrap();
    let (upstream_port, observation, upstream) = body_gated_http_upstream().await;
    runtime
        .create_script(
            ScriptKind::RequestInterceptor,
            Script {
                name: "replace-upload".into(),
                content: "req.body:replace_with_string('pong')".into(),
            },
        )
        .unwrap();
    runtime
        .replace_session_interceptors(
            session.id,
            SessionInterceptors {
                request: vec![SessionInterceptor {
                    name: "replace-upload".into(),
                    enabled: true,
                }],
                response: Vec::new(),
            },
        )
        .unwrap();

    let mut stream = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
    stream
        .write_all(
            format!(
                "POST http://127.0.0.1:{upstream_port}/replace-upload HTTP/1.1\r\n\
                 Host: 127.0.0.1:{upstream_port}\r\n\
                 Connection: close\r\n\
                 Content-Length: 10\r\n\r\nhello"
            )
            .as_bytes(),
        )
        .await
        .unwrap();

    let observed = timeout(Duration::from_secs(1), observation)
        .await
        .expect("replacement upload waited for the raw client body")
        .unwrap();
    assert_eq!(observed.body, b"pong");
    stream.write_all(b"world").await.unwrap();
    let mut response = Vec::new();
    timeout(Duration::from_secs(2), stream.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert!(response.ends_with(b"ok"), "{:?}", response);
    upstream.await.unwrap();

    let capture = timeout(Duration::from_secs(1), async {
        loop {
            let capture = runtime.list_captures(session.id, 1, None).unwrap()[0].clone();
            if capture.outcome == CaptureOutcome::Success {
                break capture;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let raw_path = runtime
        .workspace()
        .session_dir(session.id)
        .join("blob")
        .join(format!("{}-request.body", capture.id));
    let modified_path = runtime
        .workspace()
        .session_dir(session.id)
        .join("blob")
        .join(format!("{}-request.body.modified", capture.id));
    assert_eq!(tokio::fs::read(raw_path).await.unwrap(), b"helloworld");
    assert_eq!(tokio::fs::read(modified_path).await.unwrap(), b"pong");

    runtime.stop_proxy().await.unwrap();
}

#[tokio::test]
async fn response_body_frame_timeout_terminates_the_stream_and_fails_the_capture() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let session = runtime.create_session(None, None).unwrap();
    let (upstream_port, upstream) = delayed_response_frame_upstream(Duration::from_secs(2)).await;
    runtime
        .create_script(
            ScriptKind::ResponseInterceptor,
            Script {
                name: "response-timeout".into(),
                content: "req:set_tag('_crab_resp_bodyframe_timeout', '50')".into(),
            },
        )
        .unwrap();
    runtime
        .replace_session_interceptors(
            session.id,
            SessionInterceptors {
                request: Vec::new(),
                response: vec![SessionInterceptor {
                    name: "response-timeout".into(),
                    enabled: true,
                }],
            },
        )
        .unwrap();

    let response = proxy_get(
        proxy_port,
        &format!("http://127.0.0.1:{upstream_port}/response-timeout"),
        &format!("127.0.0.1:{upstream_port}"),
    )
    .await;
    assert!(response.ends_with("hello"), "{response}");

    let capture = timeout(Duration::from_secs(1), async {
        loop {
            let capture = runtime.list_captures(session.id, 1, None).unwrap()[0].clone();
            if capture.outcome == CaptureOutcome::Failed {
                break capture;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(capture.error.unwrap().kind, "response_body_timeout");
    let source = runtime
        .capture_body_source(session.id, capture.id, BodySide::Response)
        .unwrap()
        .unwrap();
    let BodySourceData::File(path) = source.data else {
        panic!("captured response body was not stored in a file");
    };
    assert_eq!(tokio::fs::read(path).await.unwrap(), b"hello");

    runtime.stop_proxy().await.unwrap();
    upstream.abort();
}

#[tokio::test]
async fn response_body_frame_timeout_also_bounds_the_first_frame() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let session = runtime.create_session(None, None).unwrap();
    let (upstream_port, upstream) =
        delayed_first_response_frame_upstream(Duration::from_secs(2)).await;
    runtime
        .create_script(
            ScriptKind::ResponseInterceptor,
            Script {
                name: "first-frame-timeout".into(),
                content: "req:set_tag('_crab_resp_bodyframe_timeout', '50')".into(),
            },
        )
        .unwrap();
    runtime
        .replace_session_interceptors(
            session.id,
            SessionInterceptors {
                request: Vec::new(),
                response: vec![SessionInterceptor {
                    name: "first-frame-timeout".into(),
                    enabled: true,
                }],
            },
        )
        .unwrap();

    let response = proxy_get(
        proxy_port,
        &format!("http://127.0.0.1:{upstream_port}/first-frame-timeout"),
        &format!("127.0.0.1:{upstream_port}"),
    )
    .await;
    assert!(!response.ends_with("hello"), "{response}");
    let capture = timeout(Duration::from_secs(1), async {
        loop {
            let capture = runtime.list_captures(session.id, 1, None).unwrap()[0].clone();
            if capture.outcome == CaptureOutcome::Failed {
                break capture;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(capture.error.unwrap().kind, "response_body_timeout");

    runtime.stop_proxy().await.unwrap();
    upstream.abort();
}

#[tokio::test]
async fn response_replacement_survives_a_raw_body_frame_timeout() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let session = runtime.create_session(None, None).unwrap();
    let (upstream_port, upstream) = delayed_response_frame_upstream(Duration::from_secs(2)).await;
    runtime
        .create_script(
            ScriptKind::ResponseInterceptor,
            Script {
                name: "replace-with-timeout".into(),
                content: "req:set_tag('_crab_resp_bodyframe_timeout', '50'); resp.body:replace_with_string('replacement')".into(),
            },
        )
        .unwrap();
    runtime
        .replace_session_interceptors(
            session.id,
            SessionInterceptors {
                request: Vec::new(),
                response: vec![SessionInterceptor {
                    name: "replace-with-timeout".into(),
                    enabled: true,
                }],
            },
        )
        .unwrap();

    let response = proxy_get(
        proxy_port,
        &format!("http://127.0.0.1:{upstream_port}/replace-with-timeout"),
        &format!("127.0.0.1:{upstream_port}"),
    )
    .await;
    assert!(response.ends_with("replacement"), "{response}");

    let capture = timeout(Duration::from_secs(1), async {
        loop {
            let capture = runtime.list_captures(session.id, 1, None).unwrap()[0].clone();
            if capture.outcome == CaptureOutcome::Success {
                break capture;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(capture.error.unwrap().kind, "raw_response_body_timeout");
    let raw_path = runtime
        .workspace()
        .session_dir(session.id)
        .join("blob")
        .join(format!("{}-response.body", capture.id));
    let modified_path = runtime
        .workspace()
        .session_dir(session.id)
        .join("blob")
        .join(format!("{}-response.body.modified", capture.id));
    assert_eq!(tokio::fs::read(raw_path).await.unwrap(), b"hello");
    assert_eq!(
        tokio::fs::read(modified_path).await.unwrap(),
        b"replacement"
    );

    runtime.stop_proxy().await.unwrap();
    upstream.abort();
}

#[tokio::test]
async fn routing_script_receives_http_authority_and_captures_into_active_session() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let (upstream_port, upstream) = fixed_http_upstream().await;
    let session = runtime
        .create_session(Some("capture".into()), None)
        .unwrap();
    runtime
        .create_script(
            ScriptKind::Routing,
            Script {
                name: "route".into(),
                content: format!(
                    "if phase == 'http' and req.authority == '127.0.0.1:{upstream_port}' \
                     and source.ip == '127.0.0.1' then return true end return false"
                ),
            },
        )
        .unwrap();
    let mut config = runtime.config();
    config.routing_script_name = Some("route".into());
    runtime.replace_config(config).await.unwrap();

    let response = proxy_get(
        proxy_port,
        &format!("http://127.0.0.1:{upstream_port}/routed"),
        &format!("127.0.0.1:{upstream_port}"),
    )
    .await;

    assert!(response.contains("\r\n\r\nok"));
    assert_eq!(
        runtime.list_captures(session.id, 10, None).unwrap().len(),
        1
    );
    assert_eq!(runtime.sessions().len(), 1);
    runtime.stop_proxy().await.unwrap();
    upstream.abort();
}

#[tokio::test]
async fn routing_script_false_and_invalid_returns_bypass_even_with_an_active_session() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let (upstream_port, upstream) = fixed_http_upstream().await;
    let session = runtime
        .create_session(Some("capture".into()), None)
        .unwrap();
    runtime
        .create_script(
            ScriptKind::Routing,
            Script {
                name: "route".into(),
                content: "return false".into(),
            },
        )
        .unwrap();
    let mut config = runtime.config();
    config.routing_script_name = Some("route".into());
    runtime.replace_config(config).await.unwrap();

    let first = proxy_get(
        proxy_port,
        &format!("http://127.0.0.1:{upstream_port}/false"),
        &format!("127.0.0.1:{upstream_port}"),
    )
    .await;
    assert!(first.contains("\r\n\r\nok"));

    runtime
        .update_routing_script("route", "return 'legacy-tag'".into())
        .await
        .unwrap();
    let second = proxy_get(
        proxy_port,
        &format!("http://127.0.0.1:{upstream_port}/invalid"),
        &format!("127.0.0.1:{upstream_port}"),
    )
    .await;
    assert!(second.contains("\r\n\r\nok"));

    assert!(
        runtime
            .list_captures(session.id, 10, None)
            .unwrap()
            .is_empty()
    );
    let entries = runtime.bypass_entries(10, None).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].reason, "routing_script_error");
    assert_eq!(entries[1].reason, "script_bypass");
    runtime.stop_proxy().await.unwrap();
    upstream.abort();
}

#[tokio::test]
async fn session_interceptor_chain_executes_and_records_source() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let (upstream_port, upstream) = fixed_http_upstream().await;
    let session = runtime.create_session(None, None).unwrap();
    let source = "req.headers:set(\"x-session\", \"one\")";
    runtime
        .create_script(
            ScriptKind::RequestInterceptor,
            Script {
                name: "session-header".into(),
                content: source.into(),
            },
        )
        .unwrap();
    runtime
        .replace_session_interceptors(
            session.id,
            SessionInterceptors {
                request: vec![SessionInterceptor {
                    name: "session-header".into(),
                    enabled: true,
                }],
                response: vec![],
            },
        )
        .unwrap();

    let response = proxy_get(
        proxy_port,
        &format!("http://127.0.0.1:{upstream_port}/intercepted"),
        &format!("127.0.0.1:{upstream_port}"),
    )
    .await;
    assert!(response.contains("\r\n\r\nok"));

    let capture = runtime.list_captures(session.id, 1, None).unwrap()[0].clone();
    assert_eq!(capture.request.headers["x-session"], vec!["one"]);
    let detail = runtime.capture(session.id, capture.id).unwrap().unwrap();
    assert_eq!(detail.request_interceptors.len(), 1);
    assert_eq!(detail.request_interceptors[0].name, "session-header");
    assert_eq!(detail.request_interceptors[0].content, source);

    runtime.stop_proxy().await.unwrap();
    upstream.abort();
}

#[tokio::test]
async fn request_interceptor_rewrites_method_and_upstream_uri() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let (target_port, target_request, target) = recording_http_upstream().await;
    let session = runtime.create_session(None, None).unwrap();
    let request_source =
        format!("req.method = 'BREW'; req.uri = 'http://127.0.0.1:{target_port}/rewritten?q=1'");
    runtime
        .create_script(
            ScriptKind::RequestInterceptor,
            Script {
                name: "rewrite-request-line".into(),
                content: request_source,
            },
        )
        .unwrap();
    runtime
        .create_script(
            ScriptKind::ResponseInterceptor,
            Script {
                name: "verify-rewritten-request".into(),
                content: "assert(req.method == 'BREW'); \
                          assert(req.uri.path == '/rewritten' and req.uri.query == 'q=1'); \
                          resp.headers:set('x-rewritten-request', '1')"
                    .into(),
            },
        )
        .unwrap();
    runtime
        .replace_session_interceptors(
            session.id,
            SessionInterceptors {
                request: vec![SessionInterceptor {
                    name: "rewrite-request-line".into(),
                    enabled: true,
                }],
                response: vec![SessionInterceptor {
                    name: "verify-rewritten-request".into(),
                    enabled: true,
                }],
            },
        )
        .unwrap();

    let original_port = unused_port();
    let response = proxy_get(
        proxy_port,
        &format!("http://127.0.0.1:{original_port}/original"),
        &format!("127.0.0.1:{original_port}"),
    )
    .await;
    assert!(
        response
            .to_ascii_lowercase()
            .contains("x-rewritten-request: 1"),
        "{response}"
    );
    let upstream_request = timeout(Duration::from_secs(2), target_request)
        .await
        .unwrap()
        .unwrap();
    assert!(
        upstream_request.starts_with("BREW /rewritten?q=1 HTTP/1.1"),
        "{upstream_request}"
    );

    let detail = runtime
        .capture(
            session.id,
            runtime.list_captures(session.id, 1, None).unwrap()[0].id,
        )
        .unwrap()
        .unwrap();
    assert_eq!(detail.summary.request.method, "BREW");
    assert_eq!(
        detail.summary.request.uri,
        format!("http://127.0.0.1:{target_port}/rewritten?q=1")
    );
    assert!(detail.request_interceptors[0]
        .modifications
        .iter()
        .any(|modification| matches!(modification, Modification::MethodSet { method } if method == "BREW")));
    assert!(detail.request_interceptors[0]
        .modifications
        .iter()
        .any(|modification| matches!(modification, Modification::UriSet { uri } if uri.ends_with("/rewritten?q=1"))));

    runtime.stop_proxy().await.unwrap();
    target.abort();
}

#[tokio::test]
async fn request_interceptors_read_raw_json_and_observe_prior_replacement() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let (upstream_port, upstream) = fixed_http_upstream().await;
    let session = runtime.create_session(None, None).unwrap();
    for (name, content) in [
        (
            "read-original-json",
            "local body = req.body:as_json(); assert(body.original == true); \
             req.headers:set('x-original-json', '1'); \
             req.body:replace_with_string('{\"replacement\":true}')",
        ),
        (
            "read-replacement-json",
            "local body = req.body:as_json(); assert(body.replacement == true); \
             req.headers:set('x-replacement-json', '1')",
        ),
    ] {
        runtime
            .create_script(
                ScriptKind::RequestInterceptor,
                Script {
                    name: name.into(),
                    content: content.into(),
                },
            )
            .unwrap();
    }
    runtime
        .replace_session_interceptors(
            session.id,
            SessionInterceptors {
                request: ["read-original-json", "read-replacement-json"]
                    .into_iter()
                    .map(|name| SessionInterceptor {
                        name: name.into(),
                        enabled: true,
                    })
                    .collect(),
                response: Vec::new(),
            },
        )
        .unwrap();

    let mut stream = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
    let body = br#"{"original":true}"#;
    stream
        .write_all(
            format!(
                "POST http://127.0.0.1:{upstream_port}/body-json HTTP/1.1\r\n\
                 Host: 127.0.0.1:{upstream_port}\r\n\
                 Content-Type: application/json\r\n\
                 Content-Length: {}\r\n\
                 Connection: close\r\n\r\n",
                body.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    stream.write_all(body).await.unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    assert!(response.ends_with(b"ok"));

    let capture = runtime.list_captures(session.id, 1, None).unwrap()[0].clone();
    assert_eq!(capture.request.headers["x-original-json"], vec!["1"]);
    assert_eq!(capture.request.headers["x-replacement-json"], vec!["1"]);
    let raw_path = runtime
        .workspace()
        .session_dir(session.id)
        .join("blob")
        .join(format!("{}-request.body", capture.id));
    let modified_path = runtime
        .workspace()
        .session_dir(session.id)
        .join("blob")
        .join(format!("{}-request.body.modified", capture.id));
    assert_eq!(tokio::fs::read(raw_path).await.unwrap(), body);
    assert_eq!(
        tokio::fs::read(modified_path).await.unwrap(),
        br#"{"replacement":true}"#
    );

    runtime.stop_proxy().await.unwrap();
    upstream.abort();
}

#[tokio::test]
async fn response_body_getter_waits_for_complete_raw_body_then_replays_it() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let session = runtime.create_session(None, None).unwrap();
    let (upstream_port, release, upstream) = staged_http_upstream().await;
    runtime
        .create_script(
            ScriptKind::ResponseInterceptor,
            Script {
                name: "read-response-body".into(),
                content: "local body = resp.body:as_string(); \
                          assert(body == 'helloworld'); \
                          resp.headers:set('x-body-read', '1')"
                    .into(),
            },
        )
        .unwrap();
    runtime
        .replace_session_interceptors(
            session.id,
            SessionInterceptors {
                request: Vec::new(),
                response: vec![SessionInterceptor {
                    name: "read-response-body".into(),
                    enabled: true,
                }],
            },
        )
        .unwrap();

    let request = tokio::spawn(async move {
        proxy_get(
            proxy_port,
            &format!("http://127.0.0.1:{upstream_port}/body-read"),
            &format!("127.0.0.1:{upstream_port}"),
        )
        .await
    });
    sleep(Duration::from_millis(100)).await;
    assert!(
        !request.is_finished(),
        "body getter did not wait for completion"
    );
    release.send(()).unwrap();
    let response = request.await.unwrap();
    assert!(response.to_ascii_lowercase().contains("x-body-read: 1"));
    assert!(response.ends_with("helloworld"));
    upstream.await.unwrap();
    runtime.stop_proxy().await.unwrap();
}

#[tokio::test]
async fn response_body_getter_timeout_terminates_the_downstream_body() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let session = runtime.create_session(None, None).unwrap();
    let (upstream_port, upstream) = delayed_response_frame_upstream(Duration::from_secs(2)).await;
    runtime
        .create_script(
            ScriptKind::ResponseInterceptor,
            Script {
                name: "read-response-timeout".into(),
                content: "req:set_tag('_crab_resp_bodyframe_timeout', '50'); resp.body:as_string()"
                    .into(),
            },
        )
        .unwrap();
    runtime
        .replace_session_interceptors(
            session.id,
            SessionInterceptors {
                request: Vec::new(),
                response: vec![SessionInterceptor {
                    name: "read-response-timeout".into(),
                    enabled: true,
                }],
            },
        )
        .unwrap();

    let response = proxy_get(
        proxy_port,
        &format!("http://127.0.0.1:{upstream_port}/body-read-timeout"),
        &format!("127.0.0.1:{upstream_port}"),
    )
    .await;
    assert!(!response.ends_with("helloworld"), "{response}");
    let capture = timeout(Duration::from_secs(1), async {
        loop {
            let capture = runtime.list_captures(session.id, 1, None).unwrap()[0].clone();
            if capture.outcome == CaptureOutcome::Failed {
                break capture;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(capture.error.unwrap().kind, "response_body_timeout");

    runtime.stop_proxy().await.unwrap();
    upstream.abort();
}

#[tokio::test]
async fn asset_replacement_exposes_metadata_and_current_body() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let (upstream_port, upstream) = fixed_http_upstream().await;
    let session = runtime.create_session(None, None).unwrap();
    let mut upload = runtime
        .begin_asset_upload("fixtures/replacement.json", "application/json".into())
        .await
        .unwrap();
    upload.write(br#"{"asset":true}"#).await.unwrap();
    upload.finish().await.unwrap();
    runtime
        .create_script(
            ScriptKind::ResponseInterceptor,
            Script {
                name: "asset-response".into(),
                content: "local asset = get_asset('fixtures/replacement.json'); \
                          assert(asset ~= nil and asset.size == 14); \
                          resp.body:replace_with_asset(asset); \
                          resp.headers:set('content-type', asset.content_type); \
                          local body = resp.body:as_json(); assert(body.asset == true)"
                    .into(),
            },
        )
        .unwrap();
    runtime
        .replace_session_interceptors(
            session.id,
            SessionInterceptors {
                request: Vec::new(),
                response: vec![SessionInterceptor {
                    name: "asset-response".into(),
                    enabled: true,
                }],
            },
        )
        .unwrap();

    let response = proxy_get(
        proxy_port,
        &format!("http://127.0.0.1:{upstream_port}/asset-response"),
        &format!("127.0.0.1:{upstream_port}"),
    )
    .await;
    assert!(response.ends_with(r#"{"asset":true}"#), "{response}");
    let detail = runtime
        .capture(
            session.id,
            runtime.list_captures(session.id, 1, None).unwrap()[0].id,
        )
        .unwrap()
        .unwrap();
    assert!(
        detail.response_interceptors[0]
            .modifications
            .iter()
            .any(|item| {
                matches!(
                    item,
                    Modification::BodyReplaceAsset { asset_id }
                        if asset_id == "fixtures/replacement.json"
                )
            })
    );

    runtime.stop_proxy().await.unwrap();
    upstream.abort();
}

#[tokio::test]
async fn crab_skip_avoids_upstream_and_runs_response_interceptors() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let session = runtime.create_session(None, None).unwrap();
    runtime
        .create_script(
            ScriptKind::RequestInterceptor,
            Script {
                name: "skip-upstream".into(),
                content: "req:set_tag('_crab_skip', ''); req:set_tag('team', 'checkout')".into(),
            },
        )
        .unwrap();
    runtime
        .create_script(
            ScriptKind::ResponseInterceptor,
            Script {
                name: "build-response".into(),
                content: "assert(req.method == 'GET'); \
                          assert(req.version == 'HTTP/1.1'); \
                          assert(req.uri.path == '/skipped'); \
                          assert(req.headers:get('host') ~= nil); \
                          assert(req:get_tag('team') == 'checkout'); \
                          req:set_tag('_crab_resp_speed', '60'); \
                          resp.status = 777; \
                          resp.headers:set('x-skipped', '1'); \
                          resp.body:replace_with_string('mocked')"
                    .into(),
            },
        )
        .unwrap();
    runtime
        .replace_session_interceptors(
            session.id,
            SessionInterceptors {
                request: vec![SessionInterceptor {
                    name: "skip-upstream".into(),
                    enabled: true,
                }],
                response: vec![SessionInterceptor {
                    name: "build-response".into(),
                    enabled: true,
                }],
            },
        )
        .unwrap();

    let unreachable_port = unused_port();
    let started_at = Instant::now();
    let response = proxy_get(
        proxy_port,
        &format!("http://127.0.0.1:{unreachable_port}/skipped"),
        &format!("127.0.0.1:{unreachable_port}"),
    )
    .await;
    assert!(started_at.elapsed() >= Duration::from_millis(80));
    assert!(response.starts_with("HTTP/1.1 777"), "{response}");
    assert!(
        response.to_ascii_lowercase().contains("x-skipped: 1"),
        "{response}"
    );
    assert!(response.ends_with("mocked"), "{response}");

    let capture = runtime.list_captures(session.id, 1, None).unwrap()[0].clone();
    assert_eq!(capture.request.tags["_crab_skip"], "");
    assert_eq!(capture.request.tags["team"], "checkout");
    let detail = runtime.capture(session.id, capture.id).unwrap().unwrap();
    assert_eq!(detail.response_interceptors.len(), 1);
    assert_eq!(detail.summary.response.as_ref().unwrap().status, 777);
    assert!(
        detail.response_interceptors[0]
            .modifications
            .iter()
            .any(|modification| matches!(modification, Modification::StatusSet { status: 777 }))
    );
    runtime.stop_proxy().await.unwrap();
}

#[tokio::test]
async fn request_speed_paces_the_final_body_sent_upstream() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let session = runtime.create_session(None, None).unwrap();
    let (upstream_port, observation, upstream) = body_gated_http_upstream().await;
    runtime
        .create_script(
            ScriptKind::RequestInterceptor,
            Script {
                name: "pace-request".into(),
                content: "req.body:replace_with_string('pong'); req:set_tag('_crab_req_speed', '20'); req:set_tag('_crab_req_timeout', '1000')".into(),
            },
        )
        .unwrap();
    runtime
        .replace_session_interceptors(
            session.id,
            SessionInterceptors {
                request: vec![SessionInterceptor {
                    name: "pace-request".into(),
                    enabled: true,
                }],
                response: Vec::new(),
            },
        )
        .unwrap();

    let response = proxy_post(
        proxy_port,
        &format!("http://127.0.0.1:{upstream_port}/paced-upload"),
        &format!("127.0.0.1:{upstream_port}"),
        b"ping",
    )
    .await;
    assert!(response.ends_with("ok"), "{response}");
    let observation = observation.await.unwrap();
    assert_eq!(observation.body, b"pong");
    assert!(observation.first_body_after.unwrap() >= Duration::from_millis(35));
    assert!(observation.complete_after.unwrap() >= Duration::from_millis(170));

    let capture = runtime.list_captures(session.id, 1, None).unwrap()[0].clone();
    assert_eq!(capture.request.tags["_crab_req_speed"], "20");
    assert_eq!(capture.request.tags["_crab_req_timeout"], "1000");

    runtime.stop_proxy().await.unwrap();
    upstream.await.unwrap();
}

#[tokio::test]
async fn request_timeout_includes_paced_upload_waiting() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let session = runtime.create_session(None, None).unwrap();
    let (upstream_port, observation, upstream) = body_gated_http_upstream().await;
    runtime
        .create_script(
            ScriptKind::RequestInterceptor,
            Script {
                name: "timeout-paced-request".into(),
                content:
                    "req:set_tag('_crab_req_speed', '20'); req:set_tag('_crab_req_timeout', '100')"
                        .into(),
            },
        )
        .unwrap();
    runtime
        .replace_session_interceptors(
            session.id,
            SessionInterceptors {
                request: vec![SessionInterceptor {
                    name: "timeout-paced-request".into(),
                    enabled: true,
                }],
                response: Vec::new(),
            },
        )
        .unwrap();

    let response = proxy_post(
        proxy_port,
        &format!("http://127.0.0.1:{upstream_port}/timed-out-upload"),
        &format!("127.0.0.1:{upstream_port}"),
        b"ping",
    )
    .await;
    assert!(response.starts_with("HTTP/1.1 504"), "{response}");
    let observation = timeout(Duration::from_secs(2), observation)
        .await
        .unwrap()
        .unwrap();
    assert!(observation.complete_after.is_none());

    let capture = runtime.list_captures(session.id, 1, None).unwrap()[0].clone();
    assert_eq!(capture.outcome, CaptureOutcome::Failed);
    let error = capture.error.unwrap();
    assert_eq!(error.stage, ErrorStage::Upstream);
    assert_eq!(error.kind, "upstream_timeout");

    runtime.stop_proxy().await.unwrap();
    upstream.await.unwrap();
}

#[tokio::test]
async fn response_speed_paces_the_final_interceptor_body() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let session = runtime.create_session(None, None).unwrap();
    let (upstream_port, upstream) = fixed_http_upstream().await;
    runtime
        .create_script(
            ScriptKind::ResponseInterceptor,
            Script {
                name: "pace-response".into(),
                content:
                    "resp.body:replace_with_string('pong'); req:set_tag('_crab_resp_speed', '20')"
                        .into(),
            },
        )
        .unwrap();
    runtime
        .replace_session_interceptors(
            session.id,
            SessionInterceptors {
                request: Vec::new(),
                response: vec![SessionInterceptor {
                    name: "pace-response".into(),
                    enabled: true,
                }],
            },
        )
        .unwrap();

    let (headers, body, first_body_after, complete_after) = proxy_get_body_timing(
        proxy_port,
        &format!("http://127.0.0.1:{upstream_port}/paced-download"),
        &format!("127.0.0.1:{upstream_port}"),
        4,
    )
    .await;
    assert!(headers.starts_with("HTTP/1.1 200"), "{headers}");
    assert!(headers.to_ascii_lowercase().contains("content-length: 4"));
    assert_eq!(body, b"pong");
    assert!(first_body_after >= Duration::from_millis(35));
    assert!(complete_after >= Duration::from_millis(170));

    let capture = runtime.list_captures(session.id, 1, None).unwrap()[0].clone();
    assert_eq!(capture.request.tags["_crab_resp_speed"], "20");

    runtime.stop_proxy().await.unwrap();
    upstream.abort();
}

#[tokio::test]
async fn breakpoint_accepts_temporary_changes_and_resumes_saved_script() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let (upstream_port, upstream) = fixed_http_upstream().await;
    let session = runtime.create_session(None, None).unwrap();
    runtime
        .create_script(
            ScriptKind::RequestInterceptor,
            Script {
                name: "replace-before-hold".into(),
                content: "req.headers:set('content-type', 'text/plain'); req.body:replace_with_string('before hold')".into(),
            },
        )
        .unwrap();
    runtime
        .create_script(
            ScriptKind::RequestInterceptor,
            Script {
                name: "hold-request".into(),
                content: "breakpoint(5000); req.headers:set('x-after', req:get_tag('temp'))".into(),
            },
        )
        .unwrap();
    runtime
        .replace_session_interceptors(
            session.id,
            SessionInterceptors {
                request: vec![
                    SessionInterceptor {
                        name: "replace-before-hold".into(),
                        enabled: true,
                    },
                    SessionInterceptor {
                        name: "hold-request".into(),
                        enabled: true,
                    },
                ],
                response: vec![],
            },
        )
        .unwrap();

    let uri = format!("http://127.0.0.1:{upstream_port}/breakpoint");
    let host = format!("127.0.0.1:{upstream_port}");
    let request_task = tokio::spawn(async move { proxy_get(proxy_port, &uri, &host).await });
    let filter = BreakpointListFilter {
        session_id: session.id,
        phase: Some(InterceptorKind::Request),
        interceptor_name: Some("hold-request".into()),
    };
    let breakpoint = timeout(Duration::from_secs(2), async {
        loop {
            if let Some(item) = runtime.breakpoints(&filter).into_iter().next() {
                break item;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();

    let source = runtime
        .breakpoint_body_source(breakpoint.id, BodySide::Request)
        .unwrap()
        .unwrap();
    let preview = match source.data {
        BodySourceData::File(path) => std::fs::read(path).unwrap(),
        BodySourceData::Bytes(bytes) => bytes,
    };
    assert_eq!(preview, b"before hold");

    let temporary = runtime
        .execute_breakpoint_script(
            breakpoint.id,
            &format!(
                "req.method = 'PATCH'; \
                 req.uri = 'http://127.0.0.1:{upstream_port}/temporary'; \
                 req:set_tag('temp', 'applied'); \
                 req.headers:set('content-type', 'text/plain'); \
                 req.body:replace_with_string('temporary request body')"
            ),
        )
        .unwrap();
    assert_eq!(
        temporary.execution.origin,
        InterceptorExecutionOrigin::Temporary
    );
    assert_eq!(runtime.breakpoints(&filter).len(), 1);
    let live = runtime.breakpoint(breakpoint.id).unwrap();
    assert_eq!(live.capture.summary.request.method, "PATCH");
    assert!(live.capture.summary.request.uri.ends_with("/temporary"));
    runtime.release_breakpoint(breakpoint.id).unwrap();
    let response = request_task.await.unwrap();
    assert!(response.contains("\r\n\r\nok"));

    let capture = runtime.list_captures(session.id, 1, None).unwrap()[0].clone();
    assert_eq!(capture.request.method, "PATCH");
    assert!(capture.request.uri.ends_with("/temporary"));
    assert_eq!(capture.request.headers["x-after"], ["applied"]);
    assert_eq!(capture.request.tags["temp"], "applied");
    let detail = runtime.capture(session.id, capture.id).unwrap().unwrap();
    assert!(matches!(
        detail.request_body,
        BodyPayload::Text { ref content, .. } if content == "temporary request body"
    ));
    assert_eq!(detail.request_interceptors.len(), 3);
    assert_eq!(
        detail.request_interceptors[2].origin,
        InterceptorExecutionOrigin::Temporary
    );

    runtime.stop_proxy().await.unwrap();
    upstream.abort();
}

#[tokio::test]
async fn response_breakpoint_exposes_live_response_and_applies_temporary_body() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let (upstream_port, upstream) = fixed_http_upstream().await;
    let session = runtime.create_session(None, None).unwrap();
    runtime
        .create_script(
            ScriptKind::ResponseInterceptor,
            Script {
                name: "hold-response".into(),
                content: "breakpoint(5000); resp.headers:set('x-after', req:get_tag('temp'))"
                    .into(),
            },
        )
        .unwrap();
    runtime
        .replace_session_interceptors(
            session.id,
            SessionInterceptors {
                request: vec![],
                response: vec![SessionInterceptor {
                    name: "hold-response".into(),
                    enabled: true,
                }],
            },
        )
        .unwrap();

    let uri = format!("http://127.0.0.1:{upstream_port}/response-breakpoint");
    let host = format!("127.0.0.1:{upstream_port}");
    let request_task = tokio::spawn(async move { proxy_get(proxy_port, &uri, &host).await });
    let filter = BreakpointListFilter {
        session_id: session.id,
        phase: Some(InterceptorKind::Response),
        interceptor_name: Some("hold-response".into()),
    };
    let breakpoint = timeout(Duration::from_secs(2), async {
        loop {
            if let Some(item) = runtime.breakpoints(&filter).into_iter().next() {
                break item;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();

    let live = runtime.breakpoint(breakpoint.id).unwrap();
    assert_eq!(live.capture.summary.response.as_ref().unwrap().status, 200);
    assert!(matches!(live.capture.response_body, BodyPayload::Empty));
    runtime
        .execute_breakpoint_script(
            breakpoint.id,
            "assert(req.method == 'GET'); \
             assert(req.uri.path == '/response-breakpoint'); \
             req:set_tag('temp', 'yes'); resp.status = 599; \
             resp.body:replace_with_string('changed')",
        )
        .unwrap();
    let changed = runtime.breakpoint(breakpoint.id).unwrap();
    assert_eq!(
        changed.capture.summary.response.as_ref().unwrap().status,
        599
    );
    assert!(matches!(
        changed.capture.response_body,
        BodyPayload::Text { ref content, .. } if content == "changed"
    ));

    runtime.release_breakpoint(breakpoint.id).unwrap();
    let response = request_task.await.unwrap();
    assert!(response.starts_with("HTTP/1.1 599"), "{response}");
    assert!(response.to_ascii_lowercase().contains("x-after: yes"));
    assert!(response.ends_with("changed"));
    let capture = runtime.list_captures(session.id, 1, None).unwrap()[0].clone();
    let persisted = runtime.capture(session.id, capture.id).unwrap().unwrap();
    assert!(matches!(
        persisted.response_body,
        BodyPayload::Text { ref content, .. } if content == "changed"
    ));
    runtime.stop_proxy().await.unwrap();
    upstream.abort();
}

#[tokio::test]
async fn pinned_session_cannot_be_archived_until_request_finishes() {
    let app_data = tempdir().unwrap();
    let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
    runtime.create_session(None, None).unwrap();
    let second = runtime.create_session(Some("second".into()), None).unwrap();
    let pin = runtime.pin_session(second.id).unwrap();

    assert!(
        runtime
            .archive_session(second.id)
            .await
            .unwrap_err()
            .to_string()
            .contains("requests in progress")
    );
    drop(pin);
    runtime.archive_session(second.id).await.unwrap();
}

#[tokio::test]
async fn inactive_session_can_be_archived_while_proxy_runs_but_active_session_cannot() {
    let (_app_data, runtime, _proxy_port) = runtime().await;
    let active = runtime.create_session(None, None).unwrap();
    let inactive = runtime
        .create_session(Some("inactive".into()), None)
        .unwrap();

    assert!(
        runtime
            .archive_session(active.id)
            .await
            .unwrap_err()
            .to_string()
            .contains("active session")
    );
    runtime.archive_session(inactive.id).await.unwrap();
    assert_eq!(runtime.archived_sessions(), vec![inactive]);
    runtime.stop_proxy().await.unwrap();
}

#[tokio::test]
async fn connect_diagnostic_stays_in_session_pinned_at_connect() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let first = runtime.create_session(None, None).unwrap();
    let stream = connect_tunnel(proxy_port).await;
    let second = runtime.create_session(Some("second".into()), None).unwrap();
    runtime
        .replace_active_session(Some(second.id))
        .await
        .unwrap();

    let config = ClientConfig::builder()
        .with_root_certificates(RootCertStore::empty())
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(config));
    assert!(
        connector
            .connect(ServerName::try_from("proxy.crab").unwrap(), stream)
            .await
            .is_err()
    );
    sleep(Duration::from_millis(100)).await;

    assert!(
        runtime
            .list_captures(first.id, 10, None)
            .unwrap()
            .iter()
            .any(|capture| capture.request.method == "CONNECT"
                && capture.outcome == CaptureOutcome::Failed)
    );
    assert!(
        runtime
            .list_captures(second.id, 10, None)
            .unwrap()
            .is_empty()
    );
    runtime.stop_proxy().await.unwrap();
}

#[tokio::test]
async fn active_session_change_disconnects_existing_connect_but_noop_does_not() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let first = runtime.create_session(Some("first".into()), None).unwrap();
    let mut stream = connect_tunnel(proxy_port).await;

    runtime
        .replace_active_session(Some(first.id))
        .await
        .unwrap();
    let mut byte = [0_u8; 1];
    assert!(
        timeout(Duration::from_millis(100), stream.read(&mut byte))
            .await
            .is_err()
    );

    let second = runtime.create_session(Some("second".into()), None).unwrap();
    runtime
        .replace_active_session(Some(second.id))
        .await
        .unwrap();
    assert_eq!(
        timeout(Duration::from_secs(1), stream.read(&mut byte))
            .await
            .unwrap()
            .unwrap(),
        0
    );

    runtime.stop_proxy().await.unwrap();
}

#[tokio::test]
async fn active_session_change_replaces_upstream_connection_pool_but_noop_does_not() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let first = runtime.create_session(Some("first".into()), None).unwrap();
    let (upstream_port, accepted, upstream) = keepalive_counting_upstream().await;
    let uri = format!("http://127.0.0.1:{upstream_port}/pooled");
    let host = format!("127.0.0.1:{upstream_port}");

    assert!(proxy_get(proxy_port, &uri, &host).await.contains("ok"));
    runtime
        .replace_active_session(Some(first.id))
        .await
        .unwrap();
    assert!(proxy_get(proxy_port, &uri, &host).await.contains("ok"));
    assert_eq!(accepted.load(Ordering::SeqCst), 1);

    let second = runtime.create_session(Some("second".into()), None).unwrap();
    runtime
        .replace_active_session(Some(second.id))
        .await
        .unwrap();
    assert!(proxy_get(proxy_port, &uri, &host).await.contains("ok"));
    assert_eq!(accepted.load(Ordering::SeqCst), 2);

    runtime.stop_proxy().await.unwrap();
    upstream.abort();
}

#[tokio::test]
async fn only_effective_routing_script_changes_disconnect_existing_connections() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    runtime
        .create_session(Some("capture".into()), None)
        .unwrap();
    runtime
        .create_script(
            ScriptKind::Routing,
            Script {
                name: "selected".into(),
                content: "return true".into(),
            },
        )
        .unwrap();
    runtime
        .create_script(
            ScriptKind::Routing,
            Script {
                name: "unused".into(),
                content: "return true".into(),
            },
        )
        .unwrap();
    runtime
        .replace_routing_selection(Some("selected".into()))
        .await
        .unwrap();
    let mut stream = connect_tunnel(proxy_port).await;
    let mut byte = [0_u8; 1];

    runtime
        .update_routing_script("unused", "return false".into())
        .await
        .unwrap();
    assert!(
        timeout(Duration::from_millis(100), stream.read(&mut byte))
            .await
            .is_err()
    );

    runtime
        .update_routing_script("selected", "return true".into())
        .await
        .unwrap();
    assert!(
        timeout(Duration::from_millis(100), stream.read(&mut byte))
            .await
            .is_err()
    );

    runtime
        .update_routing_script("selected", "return false".into())
        .await
        .unwrap();
    assert_eq!(
        timeout(Duration::from_secs(1), stream.read(&mut byte))
            .await
            .unwrap()
            .unwrap(),
        0
    );

    runtime.stop_proxy().await.unwrap();
}

#[tokio::test]
async fn records_and_forwards_non_tls_connect_tunnel() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let session = runtime.create_session(None, None).unwrap();
    let (upstream_port, upstream) = echo_upstream().await;
    let mut stream = connect_tunnel_to(proxy_port, &format!("127.0.0.1:{upstream_port}")).await;
    stream.write_all(b"ping").await.unwrap();
    let mut echoed = [0; 4];
    timeout(Duration::from_secs(2), stream.read_exact(&mut echoed))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&echoed, b"ping");
    upstream.await.unwrap();
    sleep(Duration::from_millis(50)).await;

    assert!(
        runtime
            .list_captures(session.id, 10, None)
            .unwrap()
            .iter()
            .any(|capture| capture.outcome == CaptureOutcome::Tunneled)
    );
    runtime.stop_proxy().await.unwrap();
}

#[tokio::test]
async fn bypass_connect_tunnels_bytes_without_creating_a_session() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let (upstream_port, upstream) = echo_upstream().await;
    let mut stream = connect_tunnel_to(proxy_port, &format!("127.0.0.1:{upstream_port}")).await;
    stream.write_all(b"ping").await.unwrap();
    let mut echoed = [0; 4];
    timeout(Duration::from_secs(2), stream.read_exact(&mut echoed))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&echoed, b"ping");
    upstream.await.unwrap();
    drop(stream);

    assert!(runtime.sessions().is_empty());
    let entries = timeout(Duration::from_secs(2), async {
        loop {
            let entries = runtime.bypass_entries(10, None).unwrap();
            if entries
                .first()
                .is_some_and(|entry| entry.outcome != BypassOutcome::InProgress)
            {
                break entries;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].method, "CONNECT");
    assert_eq!(entries[0].reason, "no_active_session");
    assert_eq!(entries[0].outcome, BypassOutcome::Success);
    runtime.stop_proxy().await.unwrap();
}

#[tokio::test]
async fn stop_cancels_idle_connect_without_waiting_for_client_payload() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let session = runtime.create_session(None, None).unwrap();
    let _idle = connect_tunnel(proxy_port).await;
    timeout(Duration::from_secs(2), runtime.stop_proxy())
        .await
        .expect("proxy stop timed out")
        .unwrap();
    assert!(matches!(runtime.proxy_status(), ProxyStatus::Stopped));
    let captures = runtime.list_captures(session.id, 10, None).unwrap();
    assert!(captures.iter().any(|capture| {
        capture.request.method == "CONNECT"
            && capture
                .error
                .as_ref()
                .is_some_and(|error| error.kind == "proxy_shutdown")
    }));
}

#[tokio::test]
async fn forwards_http_upgrade_bidirectionally() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let session = runtime.create_session(None, None).unwrap();
    let (upstream_port, upstream) = upgrade_echo_upstream().await;
    runtime
        .create_script(
            ScriptKind::RequestInterceptor,
            Script {
                name: "upgrade-limits-ignored".into(),
                content: "req:set_tag('_crab_req_speed', '1'); req:set_tag('_crab_resp_speed', '1'); req:set_tag('_crab_req_timeout', '1')".into(),
            },
        )
        .unwrap();
    runtime
        .replace_session_interceptors(
            session.id,
            SessionInterceptors {
                request: vec![SessionInterceptor {
                    name: "upgrade-limits-ignored".into(),
                    enabled: true,
                }],
                response: Vec::new(),
            },
        )
        .unwrap();
    let mut stream = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
    stream
        .write_all(
            format!(
                "GET http://127.0.0.1:{upstream_port}/socket HTTP/1.1\r\n\
                 Host: 127.0.0.1:{upstream_port}\r\n\
                 Connection: Upgrade\r\nUpgrade: websocket\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut response = Vec::new();
    loop {
        let mut byte = [0];
        stream.read_exact(&mut byte).await.unwrap();
        response.push(byte[0]);
        if response.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    assert!(String::from_utf8_lossy(&response).starts_with("HTTP/1.1 101"));
    stream.write_all(b"ping").await.unwrap();
    let mut echoed = [0; 4];
    timeout(Duration::from_secs(2), stream.read_exact(&mut echoed))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&echoed, b"ping");
    upstream.await.unwrap();
    sleep(Duration::from_millis(50)).await;

    assert!(
        runtime
            .list_captures(session.id, 10, None)
            .unwrap()
            .iter()
            .any(|capture| capture
                .response
                .as_ref()
                .is_some_and(|response| response.status == 101))
    );
    runtime.stop_proxy().await.unwrap();
}

#[tokio::test]
async fn concurrent_lifecycle_calls_are_serialized() {
    let app_data = tempdir().unwrap();
    let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
    let mut config = runtime.config();
    config.proxy_host = "127.0.0.1".into();
    config.proxy_port = unused_port();
    runtime.replace_config(config).await.unwrap();

    let (first, second) = tokio::join!(runtime.start_proxy(), runtime.start_proxy());
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    assert!(matches!(
        runtime.proxy_status(),
        ProxyStatus::Running { .. }
    ));

    let (first, second) = tokio::join!(runtime.stop_proxy(), runtime.stop_proxy());
    first.unwrap();
    second.unwrap();
    assert!(matches!(runtime.proxy_status(), ProxyStatus::Stopped));
}

#[tokio::test]
async fn origin_form_ca_download_is_local_and_not_recorded() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let mut stream = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
    stream
        .write_all(
            b"GET /ca.crt HTTP/1.1\r\nHost: proxy.crab\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
        )
        .await
        .unwrap();
    let mut response = Vec::new();
    timeout(Duration::from_secs(3), stream.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();

    assert!(String::from_utf8_lossy(&response).contains("BEGIN CERTIFICATE"));
    assert!(runtime.sessions().is_empty());
    assert!(runtime.bypass_entries(10, None).unwrap().is_empty());
    runtime.stop_proxy().await.unwrap();
}

#[tokio::test]
async fn tls_insecure_tag_allows_self_signed_upstream_without_weakening_default_pool() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let session = runtime.create_session(None, None).unwrap();
    let (upstream_port, mut upstream_requests, upstream) = self_signed_https_upstream().await;
    let authority = format!("127.0.0.1:{upstream_port}");

    let rejected = proxy_https_get(&runtime, proxy_port, &authority, "/rejected").await;
    assert!(rejected.starts_with("HTTP/1.1 502"), "{rejected}");

    runtime
        .create_script(
            ScriptKind::RequestInterceptor,
            Script {
                name: "allow-self-signed-upstream".into(),
                content: "req:set_tag('_crab_tls_insecure', 'true')".into(),
            },
        )
        .unwrap();
    runtime
        .replace_session_interceptors(
            session.id,
            SessionInterceptors {
                request: vec![SessionInterceptor {
                    name: "allow-self-signed-upstream".into(),
                    enabled: true,
                }],
                response: vec![],
            },
        )
        .unwrap();

    let allowed = proxy_https_get(&runtime, proxy_port, &authority, "/allowed").await;
    assert!(allowed.starts_with("HTTP/1.1 200"), "{allowed}");
    assert!(allowed.ends_with("ok"), "{allowed}");
    let upstream_headers = timeout(Duration::from_secs(2), upstream_requests.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(!upstream_headers.contains_key("_crab_tls_insecure"));

    let allowed_capture = runtime
        .list_captures(session.id, 20, None)
        .unwrap()
        .into_iter()
        .find(|capture| capture.request.uri.ends_with("/allowed"))
        .unwrap();
    assert_eq!(allowed_capture.request.tags["_crab_tls_insecure"], "true");

    runtime
        .replace_session_interceptors(session.id, SessionInterceptors::default())
        .unwrap();
    let rejected_again = proxy_https_get(&runtime, proxy_port, &authority, "/rejected-again").await;
    assert!(
        rejected_again.starts_with("HTTP/1.1 502"),
        "{rejected_again}"
    );

    runtime.stop_proxy().await.unwrap();
    upstream.abort();
}

#[tokio::test]
async fn trusted_ca_serves_certificate_and_untrusted_ca_is_diagnostic() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let session = runtime.create_session(None, None).unwrap();

    let mut roots = RootCertStore::empty();
    for certificate in rustls_pemfile::certs(&mut Cursor::new(runtime.certificate_pem())) {
        roots.add(certificate.unwrap()).unwrap();
    }
    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(config));
    let stream = connect_tunnel(proxy_port).await;
    let mut tls = connector
        .connect(ServerName::try_from("proxy.crab").unwrap(), stream)
        .await
        .unwrap();
    tls.write_all(
        b"GET /ca.crt HTTP/1.1\r\nHost: proxy.crab\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
    )
    .await
    .unwrap();
    let mut response = Vec::new();
    timeout(Duration::from_secs(3), tls.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&response).contains("BEGIN CERTIFICATE"));
    let captures = runtime.list_captures(session.id, 10, None).unwrap();
    assert!(captures.iter().any(|capture| {
        capture.outcome == CaptureOutcome::Success
            && capture.stage == "tls_mitm"
            && capture.request.method == "CONNECT"
            && capture
                .response
                .as_ref()
                .is_some_and(|response| response.status == 200)
    }));

    let empty_roots = RootCertStore::empty();
    let config = ClientConfig::builder()
        .with_root_certificates(empty_roots)
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(config));
    let stream = connect_tunnel(proxy_port).await;
    assert!(
        connector
            .connect(ServerName::try_from("proxy.crab").unwrap(), stream)
            .await
            .is_err()
    );
    sleep(Duration::from_millis(100)).await;

    let captures = runtime.list_captures(session.id, 10, None).unwrap();
    assert!(captures.iter().any(|capture| {
        capture.outcome == CaptureOutcome::Failed
            && capture.request.method == "CONNECT"
            && capture.error.is_some()
    }));
    assert!(matches!(
        runtime.stop_proxy().await.unwrap(),
        ProxyStatus::Stopped
    ));
}

#[tokio::test]
async fn serves_ca_over_http2_without_recording_the_local_request() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let session = runtime.create_session(None, None).unwrap();
    let mut roots = RootCertStore::empty();
    for certificate in rustls_pemfile::certs(&mut Cursor::new(runtime.certificate_pem())) {
        roots.add(certificate.unwrap()).unwrap();
    }
    let mut config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.alpn_protocols = vec![b"h2".to_vec()];
    let connector = TlsConnector::from(Arc::new(config));
    let stream = connect_tunnel(proxy_port).await;
    let tls = connector
        .connect(ServerName::try_from("proxy.crab").unwrap(), stream)
        .await
        .unwrap();
    let (mut sender, connection) = hyper::client::conn::http2::Builder::new(TokioExecutor::new())
        .handshake(TokioIo::new(tls))
        .await
        .unwrap();
    let connection = tokio::spawn(connection);
    let request = Request::builder()
        .method("GET")
        .uri("https://proxy.crab/ca.crt")
        .version(Version::HTTP_2)
        .body(Full::new(Bytes::new()))
        .unwrap();
    let response = sender.send_request(request).await.unwrap();
    assert_eq!(response.version(), Version::HTTP_2);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&body).contains("BEGIN CERTIFICATE"));
    drop(sender);
    connection.abort();

    assert!(
        runtime
            .list_captures(session.id, 10, None)
            .unwrap()
            .iter()
            .all(|capture| capture.request.version != "HTTP/2")
    );
    assert!(runtime.bypass_entries(10, None).unwrap().is_empty());
    runtime.stop_proxy().await.unwrap();
}
