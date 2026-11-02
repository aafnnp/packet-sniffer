//! TCP 监听与请求分发。

use std::sync::Arc;
use std::time::Duration;

use ::http::{Request, Response};
use common::types::Scheme;
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::proxy::{connect, http};
use crate::{EngineError, ProxyBody, ProxyContext, Result};

/// 监听循环，直到收到停止信号。
pub(crate) async fn serve(
    listener: TcpListener,
    ctx: Arc<ProxyContext>,
    shutdown: CancellationToken,
) {
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => {
                debug!("收到停止信号，监听循环退出");
                break;
            }
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, peer)) => {
                        let ctx = ctx.clone();
                        tokio::spawn(async move {
                            if let Err(err) = serve_connection(stream, ctx).await {
                                debug!(%peer, error = %err, "连接处理结束");
                            }
                        });
                    }
                    Err(err) => {
                        warn!(error = %err, "接受连接失败");
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                }
            }
        }
    }
}

async fn serve_connection(stream: TcpStream, ctx: Arc<ProxyContext>) -> Result<()> {
    stream.set_nodelay(true).ok();

    let service = service_fn(move |req: Request<Incoming>| {
        let ctx = ctx.clone();
        async move {
            let response: Response<ProxyBody> = if req.method() == hyper::Method::CONNECT {
                connect::handle(ctx, req).await
            } else {
                http::handle(ctx, Scheme::Http, None, req).await
            };
            Ok::<_, std::convert::Infallible>(response)
        }
    });

    // 明文入口没有 TLS 可供协商，交给自动检测：客户端可以直接发 HTTP/1.1，
    // 也可以用 h2c prior knowledge（HTTP/2 preface）。Upgrade 只在 h1 分支生效。
    auto::Builder::new(TokioExecutor::new())
        .serve_connection_with_upgrades(TokioIo::new(stream), service)
        .await
        .map_err(|err| EngineError::Other(format!("连接处理失败：{err}")))?;

    Ok(())
}
