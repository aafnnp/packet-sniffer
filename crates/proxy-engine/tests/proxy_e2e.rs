//! 端到端测试：真实走一遍「客户端 → 代理 → 上游」的全链路。
//!
//! 覆盖三条路径：
//! 1. 明文 HTTP 代理（absolute-form 请求）；
//! 2. CONNECT 隧道 + MITM 解密（客户端信任代理根证书，HTTP/1.1）；
//! 3. CONNECT 隧道 + MITM 解密 + ALPN 协商出 HTTP/2（上游仍为 HTTP/1.1）。

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use bytes::Bytes;
use common::types::{CapturedExchange, EngineEvent, Scheme};
use http_body_util::{BodyExt, Full};
use hyper::body::{Body, Frame, Incoming};
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::{TokioExecutor, TokioIo};
use proxy_engine::rules::{Rule, RuleAction};
use proxy_engine::{EngineConfig, MemoryStore, ProxyEngine};
use rcgen::{BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair};
use rustls_pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer, ServerName};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio_rustls::{TlsAcceptor, TlsConnector};

const IO_TIMEOUT: Duration = Duration::from_secs(10);

fn temp_dir(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("packet-sniffer-e2e-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn crypto_provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// 启动一个明文 HTTP 上游，响应体回显请求行。
async fn spawn_plain_upstream() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buffer = vec![0u8; 8192];
                let Ok(read) = stream.read(&mut buffer).await else {
                    return;
                };
                let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
                let request_line = request.lines().next().unwrap_or_default().to_string();
                let body = format!("echo:{request_line}");
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            });
        }
    });

    addr
}

/// 启动一个使用自签 CA 的 HTTPS 上游（不声明 ALPN，因此只会协商到 HTTP/1.1）。
async fn spawn_tls_upstream() -> (SocketAddr, String) {
    spawn_tls_upstream_with_alpn(&[]).await
}

/// 启动一个按 ALPN 选择 HTTP/1.1 或 HTTP/2 的 HTTPS 上游。
///
/// 响应体回显上游实际看到的协议版本、URI 与 `TE` 首部，并附带一个尾部首部，
/// 用于验证「h2 上游 + 绝对 URI + TE: trailers + 尾部首部抓取」。
async fn spawn_tls_upstream_with_alpn(alpns: &[&[u8]]) -> (SocketAddr, String) {
    let (addr, pem, _accepted) = spawn_tls_upstream_counting(alpns).await;
    (addr, pem)
}

/// 同上，但额外返回上游累计接受的 TCP 连接数，用于验证连接复用。
async fn spawn_tls_upstream_counting(alpns: &[&[u8]]) -> (SocketAddr, String, Arc<AtomicUsize>) {
    let ca_key = KeyPair::generate().unwrap();
    let mut ca_params = CertificateParams::new(Vec::new()).unwrap();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.distinguished_name = DistinguishedName::new();
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "Test Upstream CA");
    let ca_cert = ca_params.self_signed(&ca_key).unwrap();

    let leaf_key = KeyPair::generate().unwrap();
    let mut leaf_params = CertificateParams::new(vec!["localhost".to_string()]).unwrap();
    leaf_params.distinguished_name = DistinguishedName::new();
    leaf_params
        .distinguished_name
        .push(DnType::CommonName, "localhost");
    let leaf = leaf_params.signed_by(&leaf_key, &ca_cert, &ca_key).unwrap();

    let mut server_config = rustls::ServerConfig::builder_with_provider(crypto_provider())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![leaf.der().clone()],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf_key.serialize_der())),
        )
        .unwrap();
    server_config.alpn_protocols = alpns.iter().map(|alpn| alpn.to_vec()).collect();
    let acceptor = TlsAcceptor::from(Arc::new(server_config));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let accepted = Arc::new(AtomicUsize::new(0));
    let accept_counter = accepted.clone();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            accept_counter.fetch_add(1, Ordering::SeqCst);
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(stream).await else {
                    return;
                };
                let http2 = tls.get_ref().1.alpn_protocol() == Some(b"h2");
                let service = service_fn(|req: Request<Incoming>| async move {
                    // 回显上游实际看到的协议、URI 与 TE，便于断言代理的转发形态。
                    let te = req
                        .headers()
                        .get("te")
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or("-");
                    let summary = format!(
                        "hello from upstream; {:?}; {}; te={te}",
                        req.version(),
                        req.uri()
                    );
                    let mut trailers = http::HeaderMap::new();
                    trailers.insert("x-upstream-trailer", http::HeaderValue::from_static("ok"));
                    let body = TrailersBody {
                        data: Some(Bytes::from(summary)),
                        trailers: Some(trailers),
                    };
                    Ok::<_, std::convert::Infallible>(Response::new(body))
                });
                if http2 {
                    let _ = hyper::server::conn::http2::Builder::new(TokioExecutor::new())
                        .serve_connection(TokioIo::new(tls), service)
                        .await;
                } else {
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(TokioIo::new(tls), service)
                        .await;
                }
            });
        }
    });

    (addr, ca_cert.pem(), accepted)
}

/// 只产出一段数据与一个尾部首部的最小 body，用于验证 trailers 的抓取与转发。
#[derive(Debug)]
struct TrailersBody {
    data: Option<Bytes>,
    trailers: Option<http::HeaderMap>,
}

impl Body for TrailersBody {
    type Data = Bytes;
    type Error = std::convert::Infallible;

    fn poll_frame(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        if let Some(data) = this.data.take() {
            return Poll::Ready(Some(Ok(Frame::data(data))));
        }
        if let Some(trailers) = this.trailers.take() {
            return Poll::Ready(Some(Ok(Frame::trailers(trailers))));
        }
        Poll::Ready(None)
    }
}

/// 建立 CONNECT 隧道并与 MITM 端点完成 TLS 握手，可选声明 ALPN。
async fn connect_tunnel_tls(
    proxy_addr: SocketAddr,
    proxy_ca_path: &Path,
    authority: &str,
    alpn: Option<&[u8]>,
) -> tokio_rustls::client::TlsStream<TcpStream> {
    let mut raw = TcpStream::connect(proxy_addr).await.unwrap();
    let connect_request = format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n");
    raw.write_all(connect_request.as_bytes()).await.unwrap();
    let response = read_headers(&mut raw).await;
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "CONNECT 失败：{response}"
    );

    let mut roots = rustls::RootCertStore::empty();
    let pem = std::fs::read(proxy_ca_path).unwrap();
    for cert in rustls_pemfile::certs(&mut pem.as_slice()) {
        roots.add(cert.unwrap()).unwrap();
    }
    let mut config = rustls::ClientConfig::builder_with_provider(crypto_provider())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    if let Some(alpn) = alpn {
        config.alpn_protocols = vec![alpn.to_vec()];
    }

    TlsConnector::from(Arc::new(config))
        .connect(ServerName::try_from("localhost").unwrap(), raw)
        .await
        .expect("MITM TLS 握手失败（代理根证书未被信任？）")
}

/// 在一条 HTTP/1.1 隧道上连续发若干个请求并读完响应体。
async fn send_sequential_requests(
    proxy_addr: SocketAddr,
    proxy_ca_path: &Path,
    authority: &str,
    count: usize,
) {
    let tls = connect_tunnel_tls(proxy_addr, proxy_ca_path, authority, None).await;
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(tls))
        .await
        .expect("HTTP/1.1 握手失败");
    tokio::spawn(async move {
        let _ = connection.await;
    });

    for index in 0..count {
        let request = Request::builder()
            .uri(format!("/seq/{index}"))
            .header("host", "localhost")
            .body(Full::new(Bytes::new()))
            .unwrap();
        let response = sender.send_request(request).await.expect("请求失败");
        assert_eq!(response.status(), 200);
        response.into_body().collect().await.unwrap();
    }
}

async fn wait_for_started(events: &mut broadcast::Receiver<EngineEvent>) -> SocketAddr {
    let event = tokio::time::timeout(IO_TIMEOUT, events.recv())
        .await
        .expect("等待启动事件超时")
        .expect("事件通道已关闭");
    match event {
        EngineEvent::Started { listen_addr, .. } => listen_addr.parse().expect("监听地址可解析"),
        other => panic!("首个事件应为 Started，实际为 {other:?}"),
    }
}

/// 轮询内存存储，直到出现满足条件且已录完 body 的记录。
async fn wait_for_exchange(
    store: &MemoryStore,
    predicate: impl Fn(&CapturedExchange) -> bool,
) -> CapturedExchange {
    let deadline = Instant::now() + IO_TIMEOUT;
    loop {
        if let Some(found) = store
            .list_all()
            .into_iter()
            .find(|exchange| exchange.is_completed() && predicate(exchange))
        {
            let body_done = found
                .response
                .as_ref()
                .map(|response| response.body_complete)
                .unwrap_or(true);
            if body_done {
                return found;
            }
        }

        assert!(
            Instant::now() < deadline,
            "等待抓取结果超时，当前记录：{:#?}",
            store.list_all()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// 读取响应直到对端关闭连接。
async fn read_to_end(mut stream: impl AsyncReadExt + Unpin) -> Vec<u8> {
    let mut buffer = Vec::new();
    let _ = tokio::time::timeout(IO_TIMEOUT, stream.read_to_end(&mut buffer)).await;
    buffer
}

/// 逐字节读到空行为止，避免把后续数据一起读走。
async fn read_headers(stream: &mut TcpStream) -> String {
    let mut buffer = Vec::new();
    let mut byte = [0u8; 1];
    for _ in 0..8192 {
        match tokio::time::timeout(IO_TIMEOUT, stream.read(&mut byte)).await {
            Ok(Ok(1)) => {
                buffer.push(byte[0]);
                if buffer.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            _ => break,
        }
    }
    String::from_utf8_lossy(&buffer).into_owned()
}

#[tokio::test]
async fn proxies_plain_http_and_captures_exchange() {
    let upstream = spawn_plain_upstream().await;
    let dir = temp_dir("plain");

    let engine = Arc::new(
        ProxyEngine::new(
            EngineConfig::new()
                .with_port(0)
                .with_ca_dir(dir.join("certs")),
        )
        .unwrap(),
    );
    let mut events = engine.subscribe();
    let store = engine.store();
    let shutdown = engine.shutdown_token();

    let run = {
        let engine = engine.clone();
        tokio::spawn(async move { engine.run().await })
    };
    let proxy_addr = wait_for_started(&mut events).await;

    let mut client = TcpStream::connect(proxy_addr).await.unwrap();
    let request = format!(
        "GET http://{upstream}/hello HTTP/1.1\r\nHost: {upstream}\r\nConnection: close\r\n\r\n"
    );
    client.write_all(request.as_bytes()).await.unwrap();

    let response = String::from_utf8_lossy(&read_to_end(client).await).into_owned();
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "代理响应异常：{response}"
    );
    assert!(
        response.contains("echo:GET /hello"),
        "响应体异常：{response}"
    );

    let exchange =
        wait_for_exchange(&store, |exchange| exchange.request.scheme == Scheme::Http).await;

    assert_eq!(exchange.request.method, "GET");
    assert_eq!(exchange.request.host, "127.0.0.1");
    assert_eq!(exchange.request.port, upstream.port());
    assert_eq!(exchange.request.path, "/hello");
    assert_eq!(exchange.request.url, format!("http://{upstream}/hello"));

    let captured = exchange.response.expect("应记录响应");
    assert_eq!(captured.status, 200);
    assert_eq!(captured.status_text, "OK");
    assert!(captured
        .header("content-type")
        .is_some_and(|value| value.starts_with("text/plain")));
    assert!(String::from_utf8_lossy(&captured.body).contains("echo:GET /hello"));

    shutdown.cancel();
    run.await.unwrap().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn failed_upstream_is_recorded_as_completed_exchange() {
    let dir = temp_dir("failed-upstream");
    let dead = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead_addr = dead.local_addr().unwrap();
    drop(dead);

    let engine = Arc::new(
        ProxyEngine::new(
            EngineConfig::new()
                .with_port(0)
                .with_ca_dir(dir.join("certs")),
        )
        .unwrap(),
    );
    let mut events = engine.subscribe();
    let store = engine.store();
    let shutdown = engine.shutdown_token();
    let run = {
        let engine = engine.clone();
        tokio::spawn(async move { engine.run().await })
    };
    let proxy_addr = wait_for_started(&mut events).await;

    let mut client = TcpStream::connect(proxy_addr).await.unwrap();
    let request = format!(
        "GET http://{dead_addr}/fail HTTP/1.1\r\nHost: {dead_addr}\r\nConnection: close\r\n\r\n"
    );
    client.write_all(request.as_bytes()).await.unwrap();
    let response = String::from_utf8_lossy(&read_to_end(client).await).into_owned();
    assert!(
        response.starts_with("HTTP/1.1 502"),
        "连接失败应返回 502：{response}"
    );

    let exchange = wait_for_exchange(&store, |exchange| exchange.request.path == "/fail").await;
    assert!(exchange.response.is_none());
    assert!(exchange.error.is_some(), "失败请求应记录错误");
    assert!(exchange.completed_at.is_some());

    shutdown.cancel();
    run.await.unwrap().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn rules_mock_responses_without_upstream() {
    let dir = temp_dir("rules");
    let rule = Rule::new(
        "mock-teapot",
        RuleAction::Mock {
            status: 418,
            content_type: "text/plain; charset=utf-8".into(),
            body: "teapot from rule".into(),
        },
    )
    .with_url("/mock");

    let engine = Arc::new(
        ProxyEngine::new(
            EngineConfig::new()
                .with_port(0)
                .with_ca_dir(dir.join("certs"))
                .with_rules(vec![rule]),
        )
        .unwrap(),
    );
    let mut events = engine.subscribe();
    let store = engine.store();
    let shutdown = engine.shutdown_token();
    let run = {
        let engine = engine.clone();
        tokio::spawn(async move { engine.run().await })
    };
    let proxy_addr = wait_for_started(&mut events).await;

    let mut client = TcpStream::connect(proxy_addr).await.unwrap();
    client
        .write_all(
            b"GET http://example.test/mock HTTP/1.1\r\nHost: example.test\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap();
    let response = String::from_utf8_lossy(&read_to_end(client).await).into_owned();
    assert!(
        response.starts_with("HTTP/1.1 418"),
        "规则响应异常：{response}"
    );
    assert!(response.contains("teapot from rule"));

    let exchange =
        wait_for_exchange(&store, |exchange| exchange.request.url.contains("/mock")).await;
    let captured = exchange.response.expect("规则响应应被记录");
    assert_eq!(captured.status, 418);
    assert_eq!(
        captured.header("x-packet-sniffer-rule"),
        Some("mock-teapot")
    );
    assert_eq!(String::from_utf8_lossy(&captured.body), "teapot from rule");

    shutdown.cancel();
    run.await.unwrap().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn decrypts_https_through_connect_tunnel() {
    let (upstream, upstream_ca_pem) = spawn_tls_upstream().await;
    let dir = temp_dir("mitm");
    let upstream_ca_path = dir.join("upstream-ca.pem");
    std::fs::write(&upstream_ca_path, upstream_ca_pem).unwrap();

    let config = EngineConfig::new()
        .with_port(0)
        .with_ca_dir(dir.join("certs"))
        .with_upstream_extra_root(Some(upstream_ca_path));
    let engine = Arc::new(ProxyEngine::new(config).unwrap());
    let mut events = engine.subscribe();
    let store = engine.store();
    let shutdown = engine.shutdown_token();
    let proxy_ca_path = engine.ca_cert_path();

    let run = {
        let engine = engine.clone();
        tokio::spawn(async move { engine.run().await })
    };
    let proxy_addr = wait_for_started(&mut events).await;

    // 1. 建立 CONNECT 隧道
    let mut raw = TcpStream::connect(proxy_addr).await.unwrap();
    let authority = format!("localhost:{}", upstream.port());
    let connect_request = format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n");
    raw.write_all(connect_request.as_bytes()).await.unwrap();
    let connect_response = read_headers(&mut raw).await;
    assert!(
        connect_response.starts_with("HTTP/1.1 200"),
        "CONNECT 失败：{connect_response}"
    );

    // 2. 信任代理根证书后与 MITM 端点握手
    let mut roots = rustls::RootCertStore::empty();
    let pem = std::fs::read(&proxy_ca_path).unwrap();
    for cert in rustls_pemfile::certs(&mut pem.as_slice()) {
        roots.add(cert.unwrap()).unwrap();
    }
    let client_config = rustls::ClientConfig::builder_with_provider(crypto_provider())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(client_config));
    let mut tls = connector
        .connect(ServerName::try_from("localhost").unwrap(), raw)
        .await
        .expect("MITM TLS 握手失败（代理根证书未被信任？）");

    // 3. 隧道内发送 origin-form 请求
    tls.write_all(b"GET /secret HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let response = String::from_utf8_lossy(&read_to_end(tls).await).into_owned();
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "MITM 转发响应异常：{response}"
    );
    assert!(
        response.contains("hello from upstream"),
        "响应体异常：{response}"
    );

    // 4. 校验解密后的记录已入库（上游目标取自 CONNECT，而非 Host 首部）
    let exchange =
        wait_for_exchange(&store, |exchange| exchange.request.scheme == Scheme::Https).await;

    assert_eq!(exchange.request.method, "GET");
    assert_eq!(exchange.request.host, "localhost");
    assert_eq!(exchange.request.port, upstream.port());
    assert_eq!(exchange.request.path, "/secret");
    assert_eq!(
        exchange.request.header("host").map(str::to_string),
        Some("localhost".to_string())
    );

    let captured = exchange.response.expect("应记录解密后的响应");
    assert_eq!(captured.status, 200);
    assert!(String::from_utf8_lossy(&captured.body).contains("hello from upstream"));

    shutdown.cancel();
    run.await.unwrap().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// 与 `decrypts_https_through_connect_tunnel` 同一条链路，但客户端经 ALPN 协商到 HTTP/2。
///
/// 代理需要按 h2 与客户端通信、按 h1 与上游通信，抓包记录里保留客户端侧的协议版本。
#[tokio::test]
async fn decrypts_http2_through_connect_tunnel() {
    let (upstream, upstream_ca_pem) = spawn_tls_upstream().await;
    let dir = temp_dir("mitm-h2");
    let upstream_ca_path = dir.join("upstream-ca.pem");
    std::fs::write(&upstream_ca_path, upstream_ca_pem).unwrap();

    let config = EngineConfig::new()
        .with_port(0)
        .with_ca_dir(dir.join("certs"))
        .with_upstream_extra_root(Some(upstream_ca_path));
    let engine = Arc::new(ProxyEngine::new(config).unwrap());
    let mut events = engine.subscribe();
    let store = engine.store();
    let shutdown = engine.shutdown_token();
    let proxy_ca_path = engine.ca_cert_path();

    let run = {
        let engine = engine.clone();
        tokio::spawn(async move { engine.run().await })
    };
    let proxy_addr = wait_for_started(&mut events).await;

    let mut raw = TcpStream::connect(proxy_addr).await.unwrap();
    let authority = format!("localhost:{}", upstream.port());
    let connect_request = format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n");
    raw.write_all(connect_request.as_bytes()).await.unwrap();
    let connect_response = read_headers(&mut raw).await;
    assert!(
        connect_response.starts_with("HTTP/1.1 200"),
        "CONNECT 失败：{connect_response}"
    );

    let mut roots = rustls::RootCertStore::empty();
    let pem = std::fs::read(&proxy_ca_path).unwrap();
    for cert in rustls_pemfile::certs(&mut pem.as_slice()) {
        roots.add(cert.unwrap()).unwrap();
    }
    let mut client_config = rustls::ClientConfig::builder_with_provider(crypto_provider())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    client_config.alpn_protocols = vec![b"h2".to_vec()];
    let tls = TlsConnector::from(Arc::new(client_config))
        .connect(ServerName::try_from("localhost").unwrap(), raw)
        .await
        .expect("MITM TLS 握手失败（代理根证书未被信任？）");
    assert_eq!(
        tls.get_ref().1.alpn_protocol(),
        Some(&b"h2"[..]),
        "代理应协商出 HTTP/2"
    );

    // 隧道内用 HTTP/2 发请求，URI 必须带 scheme 与 authority。
    let (mut sender, connection) =
        hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(tls))
            .await
            .expect("HTTP/2 握手失败");
    tokio::spawn(async move {
        let _ = connection.await;
    });

    let request = Request::builder()
        .uri(format!("https://localhost:{}/secret", upstream.port()))
        .header("x-captured-by", "h2-client")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let response = sender.send_request(request).await.expect("HTTP/2 请求失败");
    assert_eq!(response.status(), 200);
    assert_eq!(response.version(), hyper::Version::HTTP_2);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8_lossy(&body).into_owned();
    assert!(body.contains("hello from upstream"), "响应体异常：{body}");
    assert!(
        body.contains("HTTP/1.1"),
        "代理应把 h2 请求降级为 h1 转发，实际上游收到：{body}"
    );

    let exchange = wait_for_exchange(&store, |exchange| {
        exchange.request.scheme == Scheme::Https && exchange.request.path == "/secret"
    })
    .await;
    assert_eq!(exchange.request.version, "HTTP/2.0");
    assert_eq!(
        exchange.request.header("x-captured-by"),
        Some("h2-client"),
        "h2 首部应被记录"
    );

    let captured = exchange.response.expect("应记录解密后的响应");
    assert_eq!(captured.status, 200);
    assert_eq!(
        captured.version, "HTTP/1.1",
        "上游响应版本仍按实际收到的记录"
    );
    assert!(String::from_utf8_lossy(&captured.body).contains("hello from upstream"));

    shutdown.cancel();
    run.await.unwrap().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// 客户端走 HTTP/1.1，但上游声明 h2：代理应改用 h2 转发，并完整抓取尾部首部。
#[tokio::test]
async fn forwards_http2_to_upstream_and_captures_trailers() {
    let (upstream, upstream_ca_pem) = spawn_tls_upstream_with_alpn(&[b"h2"]).await;
    let dir = temp_dir("upstream-h2");
    let upstream_ca_path = dir.join("upstream-ca.pem");
    std::fs::write(&upstream_ca_path, upstream_ca_pem).unwrap();

    let config = EngineConfig::new()
        .with_port(0)
        .with_ca_dir(dir.join("certs"))
        .with_upstream_extra_root(Some(upstream_ca_path));
    let engine = Arc::new(ProxyEngine::new(config).unwrap());
    let mut events = engine.subscribe();
    let store = engine.store();
    let shutdown = engine.shutdown_token();
    let proxy_ca_path = engine.ca_cert_path();

    let run = {
        let engine = engine.clone();
        tokio::spawn(async move { engine.run().await })
    };
    let proxy_addr = wait_for_started(&mut events).await;

    // 客户端不声明 ALPN，因此下行是 HTTP/1.1。
    let tls = connect_tunnel_tls(
        proxy_addr,
        &proxy_ca_path,
        &format!("localhost:{}", upstream.port()),
        None,
    )
    .await;
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(tls))
        .await
        .expect("HTTP/1.1 握手失败");
    tokio::spawn(async move {
        let _ = connection.await;
    });

    let request = Request::builder()
        .uri("/secret")
        .header("host", "localhost")
        .header("te", "trailers")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let response = sender.send_request(request).await.expect("请求失败");
    assert_eq!(response.status(), 200);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8_lossy(&body).into_owned();
    assert!(body.contains("HTTP/2.0"), "上游应收到 h2 请求：{body}");
    assert!(
        body.contains("https://localhost:"),
        "h2 上游必须收到绝对 URI：{body}"
    );
    assert!(
        body.contains("te=trailers"),
        "TE: trailers 必须保留：{body}"
    );

    // HTTP/1.1 只有在响应声明 `Trailer` 时才允许携带尾部首部，因此这里只校验抓包结果。
    let exchange = wait_for_exchange(&store, |exchange| exchange.request.path == "/secret").await;
    assert_eq!(exchange.request.version, "HTTP/1.1");
    let captured = exchange.response.expect("应记录响应");
    assert_eq!(
        captured.version, "HTTP/2.0",
        "响应版本应反映上游实际使用的协议"
    );
    assert!(
        captured.trailers.iter().any(|header| header
            .name
            .eq_ignore_ascii_case("x-upstream-trailer")
            && header.value == "ok"),
        "抓包应记录尾部首部：{:?}",
        captured.trailers
    );

    shutdown.cancel();
    run.await.unwrap().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// 连续请求同一个 h2 上游时应复用连接：上游只应看到一次 TCP 建连。
#[tokio::test]
async fn reuses_http2_upstream_connection() {
    let (upstream, upstream_ca_pem, accepted) = spawn_tls_upstream_counting(&[b"h2"]).await;
    let dir = temp_dir("pool-on");
    let upstream_ca_path = dir.join("upstream-ca.pem");
    std::fs::write(&upstream_ca_path, upstream_ca_pem).unwrap();

    let config = EngineConfig::new()
        .with_port(0)
        .with_ca_dir(dir.join("certs"))
        .with_upstream_extra_root(Some(upstream_ca_path));
    let engine = Arc::new(ProxyEngine::new(config).unwrap());
    let mut events = engine.subscribe();
    let shutdown = engine.shutdown_token();
    let proxy_ca_path = engine.ca_cert_path();

    let run = {
        let engine = engine.clone();
        tokio::spawn(async move { engine.run().await })
    };
    let proxy_addr = wait_for_started(&mut events).await;

    send_sequential_requests(
        proxy_addr,
        &proxy_ca_path,
        &format!("localhost:{}", upstream.port()),
        3,
    )
    .await;

    assert_eq!(
        accepted.load(Ordering::SeqCst),
        1,
        "连续请求应复用同一条 HTTP/2 上游连接"
    );

    shutdown.cancel();
    run.await.unwrap().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// 连接池关闭时，每个请求都会新建上游连接。
#[tokio::test]
async fn opens_a_connection_per_request_without_pool() {
    let (upstream, upstream_ca_pem, accepted) = spawn_tls_upstream_counting(&[b"h2"]).await;
    let dir = temp_dir("pool-off");
    let upstream_ca_path = dir.join("upstream-ca.pem");
    std::fs::write(&upstream_ca_path, upstream_ca_pem).unwrap();

    let config = EngineConfig::new()
        .with_port(0)
        .with_ca_dir(dir.join("certs"))
        .with_upstream_extra_root(Some(upstream_ca_path))
        .with_upstream_pool(Duration::from_secs(90), 0);
    let engine = Arc::new(ProxyEngine::new(config).unwrap());
    let mut events = engine.subscribe();
    let shutdown = engine.shutdown_token();
    let proxy_ca_path = engine.ca_cert_path();

    let run = {
        let engine = engine.clone();
        tokio::spawn(async move { engine.run().await })
    };
    let proxy_addr = wait_for_started(&mut events).await;

    send_sequential_requests(
        proxy_addr,
        &proxy_ca_path,
        &format!("localhost:{}", upstream.port()),
        3,
    )
    .await;

    assert_eq!(
        accepted.load(Ordering::SeqCst),
        3,
        "关闭连接池后每个请求都应新建上游连接"
    );

    shutdown.cancel();
    run.await.unwrap().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// 空闲超过池的回收时间后，下一次请求会重新建连。
#[tokio::test]
async fn expires_idle_http2_upstream_connection() {
    let (upstream, upstream_ca_pem, accepted) = spawn_tls_upstream_counting(&[b"h2"]).await;
    let dir = temp_dir("pool-idle");
    let upstream_ca_path = dir.join("upstream-ca.pem");
    std::fs::write(&upstream_ca_path, upstream_ca_pem).unwrap();

    let config = EngineConfig::new()
        .with_port(0)
        .with_ca_dir(dir.join("certs"))
        .with_upstream_extra_root(Some(upstream_ca_path))
        .with_upstream_pool(Duration::from_millis(50), 4);
    let engine = Arc::new(ProxyEngine::new(config).unwrap());
    let mut events = engine.subscribe();
    let shutdown = engine.shutdown_token();
    let proxy_ca_path = engine.ca_cert_path();

    let run = {
        let engine = engine.clone();
        tokio::spawn(async move { engine.run().await })
    };
    let proxy_addr = wait_for_started(&mut events).await;
    let authority = format!("localhost:{}", upstream.port());

    send_sequential_requests(proxy_addr, &proxy_ca_path, &authority, 1).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    send_sequential_requests(proxy_addr, &proxy_ca_path, &authority, 1).await;

    assert_eq!(
        accepted.load(Ordering::SeqCst),
        2,
        "超过空闲时间后应重新建连"
    );

    shutdown.cancel();
    run.await.unwrap().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// 客户端与上游都协商出 HTTP/2，是最常见的真实场景。
#[tokio::test]
async fn decrypts_http2_on_both_sides() {
    let (upstream, upstream_ca_pem) = spawn_tls_upstream_with_alpn(&[b"h2"]).await;
    let dir = temp_dir("both-h2");
    let upstream_ca_path = dir.join("upstream-ca.pem");
    std::fs::write(&upstream_ca_path, upstream_ca_pem).unwrap();

    let config = EngineConfig::new()
        .with_port(0)
        .with_ca_dir(dir.join("certs"))
        .with_upstream_extra_root(Some(upstream_ca_path));
    let engine = Arc::new(ProxyEngine::new(config).unwrap());
    let mut events = engine.subscribe();
    let store = engine.store();
    let shutdown = engine.shutdown_token();
    let proxy_ca_path = engine.ca_cert_path();

    let run = {
        let engine = engine.clone();
        tokio::spawn(async move { engine.run().await })
    };
    let proxy_addr = wait_for_started(&mut events).await;

    let tls = connect_tunnel_tls(
        proxy_addr,
        &proxy_ca_path,
        &format!("localhost:{}", upstream.port()),
        Some(b"h2"),
    )
    .await;
    assert_eq!(
        tls.get_ref().1.alpn_protocol(),
        Some(&b"h2"[..]),
        "代理应协商出 HTTP/2"
    );

    let (mut sender, connection) =
        hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(tls))
            .await
            .expect("HTTP/2 握手失败");
    tokio::spawn(async move {
        let _ = connection.await;
    });

    let request = Request::builder()
        .uri(format!("https://localhost:{}/secret", upstream.port()))
        .body(Full::new(Bytes::new()))
        .unwrap();
    let response = sender.send_request(request).await.expect("HTTP/2 请求失败");
    assert_eq!(response.status(), 200);
    assert_eq!(response.version(), hyper::Version::HTTP_2);

    let collected = response.into_body().collect().await.unwrap();
    let client_trailers = collected.trailers().cloned();
    let body = String::from_utf8_lossy(&collected.to_bytes()).into_owned();
    assert!(body.contains("HTTP/2.0"), "上游也应是 h2：{body}");
    assert_eq!(
        client_trailers
            .as_ref()
            .and_then(|trailers| trailers.get("x-upstream-trailer"))
            .and_then(|value| value.to_str().ok()),
        Some("ok"),
        "h2 客户端应收到尾部首部"
    );

    let exchange = wait_for_exchange(&store, |exchange| exchange.request.path == "/secret").await;
    assert_eq!(exchange.request.version, "HTTP/2.0");
    let captured = exchange.response.expect("应记录响应");
    assert_eq!(captured.version, "HTTP/2.0");
    assert!(String::from_utf8_lossy(&captured.body).contains("hello from upstream"));

    shutdown.cancel();
    run.await.unwrap().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn proxies_h2c_prior_knowledge_request() {
    let upstream = spawn_plain_upstream().await;
    let dir = temp_dir("h2c");

    let engine = Arc::new(
        ProxyEngine::new(
            EngineConfig::new()
                .with_port(0)
                .with_ca_dir(dir.join("certs")),
        )
        .unwrap(),
    );
    let mut events = engine.subscribe();
    let store = engine.store();
    let shutdown = engine.shutdown_token();

    let run = {
        let engine = engine.clone();
        tokio::spawn(async move { engine.run().await })
    };
    let proxy_addr = wait_for_started(&mut events).await;

    // 明文入口没有 ALPN，靠 HTTP/2 preface 自动识别。
    let stream = TcpStream::connect(proxy_addr).await.unwrap();
    let (mut sender, connection) =
        hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(stream))
            .await
            .expect("h2c 握手失败");
    tokio::spawn(async move {
        let _ = connection.await;
    });

    let request = Request::builder()
        .uri(format!("http://{upstream}/hello"))
        .body(Full::new(Bytes::new()))
        .unwrap();
    let response = sender.send_request(request).await.expect("h2c 请求失败");
    assert_eq!(response.status(), 200);
    assert_eq!(response.version(), hyper::Version::HTTP_2);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8_lossy(&body).into_owned();
    assert!(body.contains("echo:GET /hello"), "响应体异常：{body}");

    let exchange =
        wait_for_exchange(&store, |exchange| exchange.request.scheme == Scheme::Http).await;
    assert_eq!(exchange.request.version, "HTTP/2.0");
    assert_eq!(exchange.request.path, "/hello");
    assert_eq!(exchange.request.url, format!("http://{upstream}/hello"));

    shutdown.cancel();
    run.await.unwrap().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn serves_status_page_instead_of_proxying_itself() {
    let dir = temp_dir("self");

    let engine = Arc::new(
        ProxyEngine::new(
            EngineConfig::new()
                .with_port(0)
                .with_ca_dir(dir.join("certs")),
        )
        .unwrap(),
    );
    let mut events = engine.subscribe();
    let shutdown = engine.shutdown_token();

    let run = {
        let engine = engine.clone();
        tokio::spawn(async move { engine.run().await })
    };
    let proxy_addr = wait_for_started(&mut events).await;

    let mut client = TcpStream::connect(proxy_addr).await.unwrap();
    let request = format!(
        "GET http://{proxy_addr}/ HTTP/1.1\r\nHost: {proxy_addr}\r\nConnection: close\r\n\r\n"
    );
    client.write_all(request.as_bytes()).await.unwrap();
    let response = String::from_utf8_lossy(&read_to_end(client).await).into_owned();
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "应返回状态页：{response}"
    );
    assert!(
        response.contains("packet-sniffer"),
        "状态页内容异常：{response}"
    );

    let mut client = TcpStream::connect(proxy_addr).await.unwrap();
    let request = format!(
        "GET http://{proxy_addr}/favicon.ico HTTP/1.1\r\nHost: {proxy_addr}\r\nConnection: close\r\n\r\n"
    );
    client.write_all(request.as_bytes()).await.unwrap();
    let response = String::from_utf8_lossy(&read_to_end(client).await).into_owned();
    assert!(
        response.starts_with("HTTP/1.1 204"),
        "favicon 应返回 204：{response}"
    );

    shutdown.cancel();
    run.await.unwrap().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}
