//! 上游连接：明文 TCP 与 TLS 统一成一个流类型，便于复用同一套 HTTP/1.1 客户端。

use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use rustls_pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

use crate::config::UpstreamProxy;
use crate::{EngineError, Result};

/// 上游代理 CONNECT 响应头的最大长度。
const MAX_PROXY_RESPONSE: usize = 16 * 1024;

/// 到上游站点的连接，可能是明文也可能是 TLS。
#[derive(Debug)]
pub enum UpstreamStream {
    Plain(TcpStream),
    Tls(Box<tokio_rustls::client::TlsStream<TcpStream>>),
}

impl AsyncRead for UpstreamStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            UpstreamStream::Plain(stream) => Pin::new(stream).poll_read(cx, buf),
            UpstreamStream::Tls(stream) => Pin::new(stream).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for UpstreamStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            UpstreamStream::Plain(stream) => Pin::new(stream).poll_write(cx, buf),
            UpstreamStream::Tls(stream) => Pin::new(stream).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            UpstreamStream::Plain(stream) => Pin::new(stream).poll_flush(cx),
            UpstreamStream::Tls(stream) => Pin::new(stream).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            UpstreamStream::Plain(stream) => Pin::new(stream).poll_shutdown(cx),
            UpstreamStream::Tls(stream) => Pin::new(stream).poll_shutdown(cx),
        }
    }
}

/// 建立到上游的连接；配置了链式代理时，先在该代理上打通到目标的隧道。
pub async fn connect(
    host: &str,
    port: u16,
    use_tls: bool,
    timeout: Duration,
    client_config: std::sync::Arc<rustls::ClientConfig>,
    proxy: Option<&UpstreamProxy>,
) -> Result<UpstreamStream> {
    let (socket_host, socket_port) = match proxy {
        Some(proxy) => (proxy.host.as_str(), proxy.port),
        None => (host, port),
    };

    let mut tcp = tokio::time::timeout(timeout, TcpStream::connect((socket_host, socket_port)))
        .await
        .map_err(|_| EngineError::Other(format!("连接 {socket_host}:{socket_port} 超时")))??;
    tcp.set_nodelay(true).ok();

    if let Some(proxy) = proxy {
        let authority = format!("{host}:{port}");
        tokio::time::timeout(timeout, establish_tunnel(&mut tcp, &authority, proxy))
            .await
            .map_err(|_| {
                EngineError::Other(format!(
                    "经上游代理 {} 建立到 {authority} 的隧道超时",
                    proxy.authority()
                ))
            })??;
    }

    if !use_tls {
        return Ok(UpstreamStream::Plain(tcp));
    }

    let server_name = ServerName::try_from(host.to_string())
        .map_err(|_| EngineError::InvalidServerName(host.to_string()))?;
    let connector = TlsConnector::from(client_config);
    let stream = tokio::time::timeout(timeout, connector.connect(server_name, tcp))
        .await
        .map_err(|_| EngineError::Other(format!("与 {host}:{port} 完成 TLS 握手超时")))?
        .map_err(|err| EngineError::Other(format!("与 {host}:{port} 的 TLS 握手失败：{err}")))?;

    Ok(UpstreamStream::Tls(Box::new(stream)))
}

/// 在上游代理上为 `authority` 打通 CONNECT 隧道。
async fn establish_tunnel(
    stream: &mut TcpStream,
    authority: &str,
    proxy: &UpstreamProxy,
) -> Result<()> {
    let request = tunnel_request(authority, proxy);
    stream.write_all(request.as_bytes()).await?;
    stream.flush().await?;

    let mut response = Vec::with_capacity(512);
    let mut chunk = [0u8; 512];
    loop {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Err(EngineError::Other(format!(
                "上游代理 {} 在 CONNECT 阶段关闭了连接",
                proxy.authority()
            )));
        }
        response.extend_from_slice(&chunk[..read]);
        if response.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if response.len() > MAX_PROXY_RESPONSE {
            return Err(EngineError::Other(format!(
                "上游代理 {} 的 CONNECT 响应过大",
                proxy.authority()
            )));
        }
    }

    let head = String::from_utf8_lossy(&response);
    let status_line = head.lines().next().unwrap_or_default().trim().to_string();
    match parse_tunnel_status(&status_line) {
        Some(200..=299) => Ok(()),
        Some(code) => Err(EngineError::Other(format!(
            "上游代理 {} 拒绝 CONNECT（{code}）：{status_line}",
            proxy.authority()
        ))),
        None => Err(EngineError::Other(format!(
            "无法解析上游代理响应：{status_line}"
        ))),
    }
}

fn tunnel_request(authority: &str, proxy: &UpstreamProxy) -> String {
    let mut request = format!(
        "CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\nProxy-Connection: Keep-Alive\r\n"
    );
    if let Some(authorization) = proxy.authorization_header() {
        request.push_str(&format!("Proxy-Authorization: {authorization}\r\n"));
    }
    request.push_str("\r\n");
    request
}

/// 从 `HTTP/1.1 200 Connection established` 里取出状态码。
fn parse_tunnel_status(status_line: &str) -> Option<u16> {
    status_line.split_whitespace().nth(1)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_connect_request() {
        let proxy = UpstreamProxy::new("127.0.0.1", 8118);
        let request = tunnel_request("example.com:443", &proxy);
        assert!(request.starts_with("CONNECT example.com:443 HTTP/1.1\r\n"));
        assert!(request.contains("Host: example.com:443\r\n"));
        assert!(request.ends_with("\r\n\r\n"));
        assert!(!request.contains("Proxy-Authorization"));
    }

    #[test]
    fn connect_request_carries_basic_auth() {
        let proxy = UpstreamProxy::parse("http://user:pass@10.0.0.2:3128").unwrap();
        let request = tunnel_request("example.com:443", &proxy);
        assert!(request.contains("Proxy-Authorization: Basic dXNlcjpwYXNz\r\n"));
    }

    #[test]
    fn parses_tunnel_status_codes() {
        assert_eq!(
            parse_tunnel_status("HTTP/1.1 200 Connection established"),
            Some(200)
        );
        assert_eq!(
            parse_tunnel_status("HTTP/1.0 407 Proxy Authentication Required"),
            Some(407)
        );
        assert_eq!(parse_tunnel_status("garbage"), None);
    }
}
