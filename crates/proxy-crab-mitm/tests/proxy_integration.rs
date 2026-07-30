use std::{io::Cursor, sync::Arc, time::Duration};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Request, Version};
use hyper_util::rt::{TokioExecutor, TokioIo};
use proxy_crab_mitm::{
    ProxyCrab,
    log_buffer::LogBuffer,
    model::{
        CaptureOutcome, ProxyStatus, Script, ScriptKind, SessionInterceptor, SessionInterceptors,
    },
};
use rustls::{ClientConfig, RootCertStore, pki_types::ServerName};
use tempfile::tempdir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::{sleep, timeout},
};
use tokio_rustls::TlsConnector;

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
    runtime.replace_config(config).unwrap();
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
async fn captures_plain_http_and_hot_session_switches() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let (upstream_port, upstream) = fixed_http_upstream().await;
    let first = runtime.ensure_active_session().unwrap();

    let response = proxy_get(
        proxy_port,
        &format!("http://127.0.0.1:{upstream_port}/first"),
        &format!("127.0.0.1:{upstream_port}"),
    )
    .await;
    assert!(response.contains("\r\n\r\nok"));

    let second = runtime.create_session(Some("second".into()), None).unwrap();
    runtime.activate_session(second.id).unwrap();
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
async fn session_interceptor_chain_executes_and_records_source() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let (upstream_port, upstream) = fixed_http_upstream().await;
    let session = runtime.ensure_active_session().unwrap();
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
async fn pinned_session_cannot_be_deleted_until_request_finishes() {
    let app_data = tempdir().unwrap();
    let runtime = ProxyCrab::open(app_data.path(), Arc::new(LogBuffer::default())).unwrap();
    let first = runtime.ensure_active_session().unwrap();
    let pin = runtime.pin_active_session().unwrap();
    let second = runtime.create_session(Some("second".into()), None).unwrap();
    runtime.activate_session(second.id).unwrap();

    assert!(
        runtime
            .delete_session(first.id)
            .unwrap_err()
            .to_string()
            .contains("requests in progress")
    );
    drop(pin);
    runtime.delete_session(first.id).unwrap();
}

#[tokio::test]
async fn connect_diagnostic_stays_in_session_pinned_at_connect() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let first = runtime.ensure_active_session().unwrap();
    let stream = connect_tunnel(proxy_port).await;
    let second = runtime.create_session(Some("second".into()), None).unwrap();
    runtime.activate_session(second.id).unwrap();

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
async fn records_and_forwards_non_tls_connect_tunnel() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let session = runtime.ensure_active_session().unwrap();
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
async fn stop_cancels_idle_connect_without_waiting_for_client_payload() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let session = runtime.ensure_active_session().unwrap();
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
    let session = runtime.ensure_active_session().unwrap();
    let (upstream_port, upstream) = upgrade_echo_upstream().await;
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
    runtime.replace_config(config).unwrap();

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
async fn trusted_ca_serves_certificate_and_untrusted_ca_is_diagnostic() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let session = runtime.ensure_active_session().unwrap();

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
async fn serves_decrypted_http2_requests() {
    let (_app_data, runtime, proxy_port) = runtime().await;
    let session = runtime.ensure_active_session().unwrap();
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
            .any(|capture| capture.request.version == "HTTP/2")
    );
    runtime.stop_proxy().await.unwrap();
}
