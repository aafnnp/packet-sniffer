//! CONNECT 隧道处理：HTTPS 走 MITM 解密，未开启解密时退化为透明中继。

use std::sync::Arc;

use ::http::{Request, Response, StatusCode};
use common::types::Scheme;
use hyper::body::Incoming;
use hyper::server::conn::http1 as server_http1;
use hyper::server::conn::http2 as server_http2;
use hyper::service::service_fn;
use hyper::upgrade::Upgraded;
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, BufReader};
use tokio::net::TcpStream;
use tokio_rustls::TlsAcceptor;
use tracing::{debug, info, warn};

use crate::mitm::tls;
use crate::proxy::http;
use crate::{ProxyBody, ProxyContext, Result};

/// TLS ClientHello 的 record 类型。
const TLS_HANDSHAKE: u8 = 0x16;

/// 处理 CONNECT 请求。
pub(crate) async fn handle(ctx: Arc<ProxyContext>, req: Request<Incoming>) -> Response<ProxyBody> {
    let authority = req
        .uri()
        .authority()
        .map(|authority| authority.as_str().to_string())
        .or_else(|| {
            req.headers()
                .get("host")
                .and_then(|value| value.to_str().ok())
                .map(|value| value.to_string())
        });

    let authority = match authority {
        Some(authority) => authority,
        None => {
            return http::error_response(StatusCode::BAD_REQUEST, "CONNECT 缺少目标地址");
        }
    };

    let (host, port) = match common::utils::split_authority(&authority, 443) {
        Ok(target) => target,
        Err(err) => {
            return http::error_response(
                StatusCode::BAD_REQUEST,
                &format!("CONNECT 目标无效：{err}"),
            );
        }
    };

    // 必须在返回响应前取到升级句柄，之后连接会交给隧道任务接管。
    let on_upgrade = hyper::upgrade::on(req);
    let tunnel_ctx = ctx.clone();
    tokio::spawn(async move {
        match on_upgrade.await {
            Ok(upgraded) => {
                if let Err(err) = handle_tunnel(tunnel_ctx, host, port, upgraded).await {
                    debug!(error = %err, "CONNECT 隧道结束");
                }
            }
            Err(err) => warn!(error = %err, "CONNECT 升级失败"),
        }
    });

    Response::builder()
        .status(StatusCode::OK)
        .body(http::empty_body())
        .expect("固定响应一定合法")
}

async fn handle_tunnel(
    ctx: Arc<ProxyContext>,
    host: String,
    port: u16,
    upgraded: Upgraded,
) -> Result<()> {
    let mut client = BufReader::new(TokioIo::new(upgraded));
    let first_byte = {
        let buffered = client.fill_buf().await?;
        buffered.first().copied()
    };

    if !ctx.config.capture_enabled {
        return relay(host, port, client).await;
    }

    let authority = format!("{host}:{port}");

    if first_byte == Some(TLS_HANDSHAKE) {
        info!(host, port, "开始解密 HTTPS 会话");
        let acceptor = TlsAcceptor::from(tls::server_config(ctx.ca.clone()));
        match acceptor.accept(client).await {
            Ok(stream) => {
                let http2 = tls::is_http2(stream.get_ref().1.alpn_protocol());
                if http2 {
                    info!(host, port, "已与客户端协商 HTTP/2");
                }
                serve(ctx, Scheme::Https, Some(authority), stream, http2).await
            }
            Err(err) => {
                info!(host, error = %err, "TLS 握手失败，客户端可能尚未信任本地根证书");
                Ok(())
            }
        }
    } else {
        // 明文隧道（例如 CONNECT 到 80 端口），按 HTTP/1.1 直接解析。
        serve(ctx, Scheme::Http, Some(authority), client, false).await
    }
}

/// 在已建立的流上提供 HTTP 服务：`http2` 为真时用 HTTP/2，否则用带 Upgrade 支持的 HTTP/1.1。
async fn serve<S>(
    ctx: Arc<ProxyContext>,
    scheme: Scheme,
    tunnel_authority: Option<String>,
    stream: S,
    http2: bool,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    // 两条分支都要各自构造服务闭包，因此用工厂函数避免 `service_fn` 无法克隆的问题。
    let service = || {
        let ctx = ctx.clone();
        let tunnel_authority = tunnel_authority.clone();
        service_fn(move |req: Request<Incoming>| {
            let ctx = ctx.clone();
            let tunnel_authority = tunnel_authority.clone();
            async move {
                Ok::<_, std::convert::Infallible>(
                    http::handle(ctx, scheme, tunnel_authority, req).await,
                )
            }
        })
    };

    if http2 {
        // HTTP/2 没有 HTTP/1.1 的 Upgrade 语义，WebSocket 仍走 h1 通道。
        server_http2::Builder::new(TokioExecutor::new())
            .serve_connection(TokioIo::new(stream), service())
            .await?;
    } else {
        server_http1::Builder::new()
            .serve_connection(TokioIo::new(stream), service())
            .with_upgrades()
            .await?;
    }

    Ok(())
}

/// 不解析内容，纯粹双向中继（关闭抓取时使用）。
async fn relay(host: String, port: u16, client: impl AsyncRead + AsyncWrite + Unpin) -> Result<()> {
    let mut client = client;
    let mut upstream = TcpStream::connect((host.as_str(), port)).await?;
    let (to_upstream, to_client) =
        tokio::io::copy_bidirectional(&mut client, &mut upstream).await?;
    debug!(to_upstream, to_client, "透明隧道结束");
    Ok(())
}
