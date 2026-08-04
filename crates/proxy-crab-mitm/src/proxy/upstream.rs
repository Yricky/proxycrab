use std::{str::FromStr, sync::Arc};

use anyhow::{Context, Result, anyhow, bail};
use hyper::{Request, Response, Uri, Version, body::Incoming, header::UPGRADE};
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::{
    client::legacy::{Client, connect::HttpConnector},
    rt::{TokioExecutor, TokioIo, TokioTimer},
};
use rustls::{
    ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{CryptoProvider, WebPkiSupportedAlgorithms},
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
    time::timeout,
};
use tokio_rustls::TlsConnector;
use tokio_util::sync::CancellationToken;

use super::{CONNECT_TIMEOUT, ProxyBody};

type PooledClient = Client<HttpsConnector<HttpConnector>, ProxyBody>;

#[derive(Clone)]
pub(crate) struct UpstreamClient {
    verified: PooledClient,
    insecure: PooledClient,
}

impl UpstreamClient {
    pub(crate) fn new() -> Self {
        Self {
            verified: pooled_client(client_config(false)),
            insecure: pooled_client(client_config(true)),
        }
    }

    pub(crate) async fn send(
        &self,
        mut request: Request<ProxyBody>,
        tls_insecure: bool,
        cancellation: CancellationToken,
    ) -> Result<Response<Incoming>> {
        let tls_insecure = tls_insecure && request.uri().scheme_str() == Some("https");
        if request.headers().contains_key(UPGRADE)
            || (request.uri().scheme_str() == Some("http") && request.version() == Version::HTTP_2)
        {
            return send_dedicated(request, tls_insecure, cancellation).await;
        }
        normalize_pooled_request_version(&mut request);
        let client = if tls_insecure {
            &self.insecure
        } else {
            &self.verified
        };
        tokio::select! {
            response = client.request(request) => Ok(response?),
            _ = cancellation.cancelled() => bail!("proxy stopped"),
        }
    }
}

fn pooled_client(config: ClientConfig) -> PooledClient {
    let mut http = HttpConnector::new();
    http.enforce_http(false);
    http.set_connect_timeout(Some(CONNECT_TIMEOUT));
    let https = HttpsConnectorBuilder::new()
        .with_tls_config(config)
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .wrap_connector(http);
    Client::builder(TokioExecutor::new())
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .pool_timer(TokioTimer::new())
        .pool_max_idle_per_host(32)
        .build(https)
}

fn client_config(tls_insecure: bool) -> ClientConfig {
    let roots = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let mut config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    if tls_insecure {
        let algorithms = CryptoProvider::get_default()
            .expect("rustls crypto provider is installed by ClientConfig::builder")
            .signature_verification_algorithms;
        config
            .dangerous()
            .set_certificate_verifier(Arc::new(InsecureServerCertVerifier { algorithms }));
    }
    config
}

#[derive(Debug)]
struct InsecureServerCertVerifier {
    algorithms: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for InsecureServerCertVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

fn normalize_pooled_request_version(request: &mut Request<ProxyBody>) {
    // The captured version describes the downstream connection. For pooled HTTPS, ALPN chooses the
    // upstream version; hyper-util otherwise treats HTTP/2 here as a hard requirement and rejects
    // an origin that negotiates HTTP/1.1 instead of performing the coercion used by hyper's h1 API.
    if request.version() == Version::HTTP_2 {
        *request.version_mut() = Version::HTTP_11;
    }
}

async fn send_dedicated(
    request: Request<ProxyBody>,
    tls_insecure: bool,
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
        let mut config = client_config(tls_insecure);
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

#[cfg(test)]
mod tests {
    use std::{
        convert::Infallible,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use bytes::Bytes;
    use http_body_util::{BodyExt, Full};
    use hyper::{
        Request, Response, StatusCode,
        header::{CONNECTION, UPGRADE},
        server::conn::http1,
        service::service_fn,
    };
    use hyper_util::rt::TokioIo;
    use rcgen::{CertifiedKey, generate_simple_self_signed};
    use rustls::{
        ServerConfig,
        pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer},
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::oneshot,
    };
    use tokio_rustls::TlsAcceptor;
    use tokio_util::sync::CancellationToken;

    use super::{UpstreamClient, normalize_pooled_request_version};
    use crate::proxy::boxed_full;

    #[tokio::test]
    async fn reuses_http1_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let accepted = Arc::new(AtomicUsize::new(0));
        let accepted_by_server = accepted.clone();
        let (stop_tx, mut stop_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut stop_rx => break,
                    accepted = listener.accept() => {
                        let (stream, _) = accepted.unwrap();
                        accepted_by_server.fetch_add(1, Ordering::Relaxed);
                        tokio::spawn(async move {
                            let service = service_fn(|_| async {
                                Ok::<_, Infallible>(Response::new(Full::new(Bytes::from_static(b"ok"))))
                            });
                            let _ = http1::Builder::new()
                                .serve_connection(TokioIo::new(stream), service)
                                .await;
                        });
                    }
                }
            }
        });

        let client = UpstreamClient::new();
        for _ in 0..2 {
            let request = Request::builder()
                .uri(format!("http://{address}/"))
                .body(boxed_full(Bytes::new()))
                .unwrap();
            let response = client
                .send(request, false, CancellationToken::new())
                .await
                .unwrap();
            assert_eq!(
                response.into_body().collect().await.unwrap().to_bytes(),
                Bytes::from_static(b"ok")
            );
        }

        assert_eq!(accepted.load(Ordering::Relaxed), 1);
        let _ = stop_tx.send(());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn dedicated_https_upgrade_honors_insecure_tls_policy() {
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
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut tls = acceptor.accept(stream).await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                tls.read_exact(&mut byte).await.unwrap();
                request.push(byte[0]);
            }
            tls.write_all(
                b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n",
            )
            .await
            .unwrap();
        });

        let request = Request::builder()
            .uri(format!("https://{address}/upgrade"))
            .header(CONNECTION, "upgrade")
            .header(UPGRADE, "websocket")
            .body(boxed_full(Bytes::new()))
            .unwrap();
        let response = UpstreamClient::new()
            .send(request, true, CancellationToken::new())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
        server.await.unwrap();
    }

    #[test]
    fn pooled_https_allows_alpn_to_choose_http1_for_downstream_http2() {
        let mut request = Request::builder()
            .uri("https://example.com/")
            .version(hyper::Version::HTTP_2)
            .body(boxed_full(Bytes::new()))
            .unwrap();

        normalize_pooled_request_version(&mut request);

        assert_eq!(request.version(), hyper::Version::HTTP_11);
    }
}
