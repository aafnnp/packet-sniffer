//! TCP 监听与请求分发。

use std::sync::Arc;
use std::time::Duration;

use ::http::{Request, Response};
use common::types::Scheme;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::proxy::{connect, http};
use crate::{ProxyBody, ProxyContext, Result};

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

    http1::Builder::new()
        .keep_alive(true)
        .serve_connection(TokioIo::new(stream), service)
        .with_upgrades()
        .await?;

    Ok(())
}
