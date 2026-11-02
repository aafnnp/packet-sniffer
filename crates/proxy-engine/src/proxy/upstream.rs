//! 上游连接：明文 TCP 与 TLS 统一成一个流类型，并按 ALPN 协商结果提供 HTTP/1.1 或 HTTP/2 客户端。

use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use hyper::body::Incoming;
use hyper::client::conn::http1 as client_http1;
use hyper::client::conn::http2 as client_http2;
use hyper_util::rt::{TokioExecutor, TokioIo};
use rustls_pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tracing::debug;

use crate::mitm::tls;
use crate::protocol::body::RecordingBody;
use crate::{EngineError, Result};

/// 发给上游的请求体：边转发边录制，HTTP/1.1 与 HTTP/2 共用同一种类型。
pub(crate) type UpstreamRequestBody = RecordingBody<Incoming>;

/// 与上游实际使用的 HTTP 协议版本。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UpstreamProtocol {
    Http1,
    Http2,
}

impl UpstreamProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Http1 => "HTTP/1.1",
            Self::Http2 => "HTTP/2.0",
        }
    }
}

/// 上游请求发送端：把 h1 / h2 两种 `SendRequest` 包成同一个接口。
pub(crate) enum UpstreamSender {
    Http1(client_http1::SendRequest<UpstreamRequestBody>),
    Http2(client_http2::SendRequest<UpstreamRequestBody>),
}

impl UpstreamSender {
    pub async fn send_request(
        &mut self,
        request: http::Request<UpstreamRequestBody>,
    ) -> Result<http::Response<Incoming>> {
        match self {
            Self::Http1(sender) => sender.send_request(request).await,
            Self::Http2(sender) => sender.send_request(request).await,
        }
        .map_err(EngineError::Http)
    }

    /// 是否还能再开一条流：只有 HTTP/2 连接支持多路复用。
    pub fn is_reusable(&self) -> bool {
        match self {
            Self::Http1(_) => false,
            Self::Http2(sender) => !sender.is_closed() && sender.is_ready(),
        }
    }

    /// 复制一个发送端，用于在同一条 HTTP/2 连接上再开一条流。
    pub fn reuse(&self) -> Option<Self> {
        match self {
            Self::Http1(_) => None,
            Self::Http2(sender) if !sender.is_closed() && sender.is_ready() => {
                Some(Self::Http2(sender.clone()))
            }
            Self::Http2(_) => None,
        }
    }
}

/// 到上游站点的连接，可能是明文也可能是 TLS。
#[derive(Debug)]
pub enum UpstreamStream {
    Plain(TcpStream),
    Tls(Box<tokio_rustls::client::TlsStream<TcpStream>>),
}

impl UpstreamStream {
    /// TLS 协商出的 ALPN；明文连接恒为 `None`。
    fn alpn_protocol(&self) -> Option<&[u8]> {
        match self {
            UpstreamStream::Plain(_) => None,
            UpstreamStream::Tls(stream) => stream.get_ref().1.alpn_protocol(),
        }
    }
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

/// 建立到上游站点的连接。
pub async fn connect(
    host: &str,
    port: u16,
    use_tls: bool,
    timeout: Duration,
    client_config: std::sync::Arc<rustls::ClientConfig>,
) -> Result<UpstreamStream> {
    let tcp = tokio::time::timeout(timeout, TcpStream::connect((host, port)))
        .await
        .map_err(|_| EngineError::Other(format!("连接 {host}:{port} 超时")))??;
    tcp.set_nodelay(true).ok();

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

/// 依据 ALPN 协商结果建立上游 HTTP 客户端，并在后台驱动连接任务。
///
/// 返回实际协议版本：调用方据此决定请求用 origin-form 还是完整 URI、版本号写 h1 还是 h2。
pub(crate) async fn handshake(
    stream: UpstreamStream,
) -> Result<(UpstreamSender, UpstreamProtocol)> {
    let protocol = if tls::is_http2(stream.alpn_protocol()) {
        UpstreamProtocol::Http2
    } else {
        UpstreamProtocol::Http1
    };

    let io = TokioIo::new(stream);
    match protocol {
        UpstreamProtocol::Http2 => {
            let (sender, connection) = client_http2::handshake(TokioExecutor::new(), io)
                .await
                .map_err(EngineError::Http)?;
            tokio::spawn(async move {
                if let Err(err) = connection.await {
                    debug!(error = %err, "上游 HTTP/2 连接关闭");
                }
            });
            Ok((UpstreamSender::Http2(sender), protocol))
        }
        UpstreamProtocol::Http1 => {
            let (sender, connection) = client_http1::handshake(io)
                .await
                .map_err(EngineError::Http)?;
            tokio::spawn(async move {
                if let Err(err) = connection.await {
                    debug!(error = %err, "上游连接关闭");
                }
            });
            Ok((UpstreamSender::Http1(sender), protocol))
        }
    }
}
