//! 端到端测试：真实走一遍「客户端 → 代理 → 上游」的全链路。
//!
//! 覆盖两条路径：
//! 1. 明文 HTTP 代理（absolute-form 请求）；
//! 2. CONNECT 隧道 + MITM 解密（客户端信任代理根证书）。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use common::types::{CapturedExchange, EngineEvent, Scheme};
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use proxy_engine::config::UpstreamProxy;
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

/// 启动一个使用自签 CA 的 HTTPS 上游，返回地址与该 CA 的 PEM。
async fn spawn_tls_upstream() -> (SocketAddr, String) {
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

    let server_config = rustls::ServerConfig::builder_with_provider(crypto_provider())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![leaf.der().clone()],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf_key.serialize_der())),
        )
        .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(server_config));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(stream).await else {
                    return;
                };
                let service = service_fn(|_req: Request<Incoming>| async {
                    Ok::<_, std::convert::Infallible>(Response::new(Full::new(Bytes::from_static(
                        b"hello from upstream",
                    ))))
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(tls), service)
                    .await;
            });
        }
    });

    (addr, ca_cert.pem())
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

/// 最小可用的上游 HTTP 代理：只处理 CONNECT，收到后双向中继。
async fn spawn_connect_proxy() -> (SocketAddr, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let tunnels = Arc::new(AtomicUsize::new(0));
    let counter = tunnels.clone();

    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            let counter = counter.clone();
            tokio::spawn(async move {
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                loop {
                    match stream.read(&mut byte).await {
                        Ok(1) => {
                            head.push(byte[0]);
                            if head.ends_with(b"\r\n\r\n") {
                                break;
                            }
                        }
                        _ => return,
                    }
                }

                let head = String::from_utf8_lossy(&head).into_owned();
                let mut parts = head.lines().next().unwrap_or_default().split_whitespace();
                if parts.next() != Some("CONNECT") {
                    return;
                }
                let Some(authority) = parts.next() else {
                    return;
                };
                let Ok(mut upstream) = TcpStream::connect(authority).await else {
                    return;
                };

                if stream
                    .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                    .await
                    .is_err()
                {
                    return;
                }
                counter.fetch_add(1, Ordering::SeqCst);
                let _ = tokio::io::copy_bidirectional(&mut stream, &mut upstream).await;
            });
        }
    });

    (addr, tunnels)
}

#[tokio::test]
async fn chains_through_upstream_proxy() {
    let (upstream, upstream_ca_pem) = spawn_tls_upstream().await;
    let (chain_proxy, tunnels) = spawn_connect_proxy().await;

    let dir = temp_dir("chain");
    let upstream_ca_path = dir.join("upstream-ca.pem");
    std::fs::write(&upstream_ca_path, upstream_ca_pem).unwrap();

    let config = EngineConfig::new()
        .with_port(0)
        .with_ca_dir(dir.join("certs"))
        .with_upstream_extra_root(Some(upstream_ca_path))
        .with_upstream_proxy(Some(UpstreamProxy::new("127.0.0.1", chain_proxy.port())));
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

    // 客户端 → 我们的代理（CONNECT + TLS）
    let mut raw = TcpStream::connect(proxy_addr).await.unwrap();
    let authority = format!("localhost:{}", upstream.port());
    raw.write_all(format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n").as_bytes())
        .await
        .unwrap();
    assert!(read_headers(&mut raw).await.starts_with("HTTP/1.1 200"));

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
    let mut tls = TlsConnector::from(Arc::new(client_config))
        .connect(ServerName::try_from("localhost").unwrap(), raw)
        .await
        .expect("MITM 握手失败");

    tls.write_all(b"GET /through-proxy HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let response = String::from_utf8_lossy(&read_to_end(tls).await).into_owned();
    assert!(response.starts_with("HTTP/1.1 200"), "响应异常：{response}");
    assert!(response.contains("hello from upstream"));

    assert!(
        tunnels.load(Ordering::SeqCst) >= 1,
        "流量没有经过配置的上游代理"
    );

    let exchange =
        wait_for_exchange(&store, |exchange| exchange.request.scheme == Scheme::Https).await;
    assert_eq!(exchange.request.path, "/through-proxy");

    shutdown.cancel();
    run.await.unwrap().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}
