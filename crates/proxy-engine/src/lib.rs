//! 抓包引擎对外 API。
//!
//! 引擎不依赖任何 UI 或系统 GUI 库：`ProxyEngine::run` 只在 tokio 运行时里监听端口，
//! 通过事件广播把请求推送给上层，因此可以独立编译、独立测试。

pub mod config;
pub mod mitm;
pub mod protocol;
pub mod proxy;
pub mod rules;
pub mod store;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use bytes::Bytes;
use common::types::{EngineEvent, LogLevel};
use http_body_util::combinators::BoxBody;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

pub use config::EngineConfig;
pub use store::MemoryStore;

/// 事件广播缓冲：UI 消费不及时时只影响积压事件，不阻塞网络线程。
const EVENT_BUFFER: usize = 1024;

/// 上游连接池的回收周期。
const POOL_PRUNE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;
/// 代理返回给客户端的 body 类型。
pub type ProxyBody = BoxBody<Bytes, BoxError>;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),
    #[error("证书错误：{0}")]
    Certificate(String),
    #[error("TLS 错误：{0}")]
    Tls(#[from] rustls::Error),
    #[error("无效的服务器名：{0}")]
    InvalidServerName(String),
    #[error("HTTP 错误：{0}")]
    Http(#[from] hyper::Error),
    #[error("HTTP 构建错误：{0}")]
    HttpBuild(#[from] http::Error),
    #[error("公共层错误：{0}")]
    Common(#[from] common::CommonError),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, EngineError>;

/// 引擎内部共享上下文，各协议处理模块只依赖它。
pub(crate) struct ProxyContext {
    pub config: EngineConfig,
    pub store: Arc<MemoryStore>,
    pub events: broadcast::Sender<EngineEvent>,
    pub ca: Arc<mitm::ca::CertificateAuthority>,
    pub tls_client: Arc<rustls::ClientConfig>,
    /// 上游 HTTP/2 连接池，按目标复用连接。
    pub pool: proxy::pool::UpstreamPool,
    /// 实际绑定的地址（`--port 0` 时由系统分配），用于识别「请求打到代理自己」。
    bound_addr: OnceLock<SocketAddr>,
}

impl ProxyContext {
    /// 实际监听地址；尚未绑定端口时退回配置值。
    pub fn listen_addr(&self) -> SocketAddr {
        self.bound_addr
            .get()
            .copied()
            .unwrap_or(self.config.listen_addr)
    }

    /// 推送一条引擎级日志事件。
    pub fn log(&self, level: LogLevel, message: impl Into<String>) {
        self.emit(EngineEvent::Log {
            level,
            message: message.into(),
        });
    }

    /// 非阻塞推送事件：没有订阅者时直接丢弃。
    pub fn emit(&self, event: EngineEvent) {
        if self.events.receiver_count() == 0 {
            return;
        }
        let _ = self.events.send(event);
    }
}

/// 抓包引擎。创建后调用 [`ProxyEngine::run`] 开始监听。
pub struct ProxyEngine {
    ctx: Arc<ProxyContext>,
    shutdown: CancellationToken,
}

impl ProxyEngine {
    /// 初始化存储、根证书与事件通道。首次运行会在 `ca_dir` 生成根证书。
    pub fn new(config: EngineConfig) -> Result<Self> {
        let ca = Arc::new(mitm::ca::CertificateAuthority::load_or_create(
            &config.ca_dir,
            &config.ca_common_name,
        )?);
        let store = Arc::new(MemoryStore::new(config.store_capacity));
        let (events, _) = broadcast::channel(EVENT_BUFFER);

        let extra_roots = match config.upstream_extra_root.as_deref() {
            Some(path) => mitm::tls::load_certs(path)?,
            None => Vec::new(),
        };
        let tls_client = mitm::tls::client_config(&extra_roots);
        let pool = proxy::pool::UpstreamPool::new(
            config.upstream_pool_idle,
            config.upstream_pool_max_per_host,
        );

        Ok(Self {
            ctx: Arc::new(ProxyContext {
                config,
                store,
                events,
                ca,
                tls_client,
                pool,
                bound_addr: OnceLock::new(),
            }),
            shutdown: CancellationToken::new(),
        })
    }

    pub fn config(&self) -> &EngineConfig {
        &self.ctx.config
    }

    pub fn store(&self) -> Arc<MemoryStore> {
        self.ctx.store.clone()
    }

    /// 根证书路径，供 UI / CLI 引导用户安装。
    pub fn ca_cert_path(&self) -> PathBuf {
        self.ctx.ca.cert_path().to_path_buf()
    }

    /// 订阅引擎事件；可多次调用，每个订阅者都会收到完整事件流。
    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.ctx.events.subscribe()
    }

    pub fn shutdown_token(&self) -> CancellationToken {
        self.shutdown.clone()
    }

    pub fn shutdown(&self) {
        self.shutdown.cancel();
    }

    /// 绑定端口并开始服务，直到收到停止信号。
    pub async fn run(&self) -> Result<()> {
        let listener = TcpListener::bind(self.ctx.config.listen_addr).await?;
        let addr = listener.local_addr()?;
        let _ = self.ctx.bound_addr.set(addr);
        tracing::info!(%addr, "抓包引擎已启动");

        self.ctx.emit(EngineEvent::Started {
            listen_addr: addr.to_string(),
            ca_cert_path: self.ctx.ca.cert_path().display().to_string(),
        });

        // 定期回收空闲或已关闭的上游连接，避免连接池长期占用上游资源。
        let prune_ctx = self.ctx.clone();
        let prune_shutdown = self.shutdown.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(POOL_PRUNE_INTERVAL);
            ticker.tick().await;
            loop {
                tokio::select! {
                    _ = prune_shutdown.cancelled() => break,
                    _ = ticker.tick() => prune_ctx.pool.prune(),
                }
            }
        });

        proxy::tcp::serve(listener, self.ctx.clone(), self.shutdown.clone()).await;

        self.ctx.emit(EngineEvent::Stopped);
        tracing::info!("抓包引擎已停止");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::types::EngineEvent;
    use std::time::Duration;

    fn test_config() -> EngineConfig {
        let dir =
            std::env::temp_dir().join(format!("packet-sniffer-engine-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        EngineConfig::new().with_ca_dir(dir).with_port(0)
    }

    #[tokio::test]
    async fn engine_starts_and_stops() {
        let engine = ProxyEngine::new(test_config()).unwrap();
        let mut events = engine.subscribe();
        let token = engine.shutdown_token();

        let run = tokio::spawn(async move {
            let result = engine.run().await;
            (result, engine)
        });

        let started = tokio::time::timeout(Duration::from_secs(5), events.recv())
            .await
            .expect("应收到启动事件")
            .expect("事件通道可用");
        match started {
            EngineEvent::Started { listen_addr, .. } => {
                assert!(!listen_addr.is_empty());
            }
            other => panic!("首个事件应为 Started，实际为 {other:?}"),
        }

        token.cancel();
        let (result, _engine) = tokio::time::timeout(Duration::from_secs(5), run)
            .await
            .expect("引擎应能及时停止")
            .expect("任务未 panic");
        result.unwrap();

        let stopped = tokio::time::timeout(Duration::from_secs(5), events.recv())
            .await
            .expect("应收到停止事件")
            .expect("事件通道可用");
        assert!(matches!(stopped, EngineEvent::Stopped));
    }
}
