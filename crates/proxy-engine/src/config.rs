//! 引擎配置。

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::rules::Rule;

/// 默认监听端口。
pub const DEFAULT_PORT: u16 = 8899;

/// 抓包引擎的运行配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineConfig {
    /// 代理监听地址，默认 `127.0.0.1:8899`。
    pub listen_addr: SocketAddr,
    /// 根证书存放目录（证书与私钥由引擎首次启动时生成）。
    pub ca_dir: PathBuf,
    /// 根证书 Common Name。
    pub ca_common_name: String,
    /// 是否解密 HTTPS（关闭后 CONNECT 仅做透明转发）。
    pub capture_enabled: bool,
    /// 单个 body 的抓取上限，超出部分不进入内存存储。
    pub max_body_bytes: usize,
    /// 内存存储的最大请求条数。
    pub store_capacity: usize,
    /// 连接上游的超时时间。
    pub connect_timeout: Duration,
    /// 等待响应体读完的最长时间，超时按已抓取部分入库。
    pub body_idle_timeout: Duration,
    /// 额外的上游信任根证书（PEM）。用于上游已被企业代理二次拦截的场景，
    /// 未配置时只信任系统内置的 Mozilla 根证书集合。
    pub upstream_extra_root: Option<PathBuf>,
    /// 上游 HTTP/2 连接的空闲回收时间；为 0 时关闭连接复用。
    #[serde(default = "default_pool_idle")]
    pub upstream_pool_idle: Duration,
    /// 每个上游目标最多保留的 HTTP/2 连接数；为 0 时关闭连接复用。
    #[serde(default = "default_pool_max_per_host")]
    pub upstream_pool_max_per_host: usize,
    /// 请求 / 响应规则，按顺序匹配。
    #[serde(default)]
    pub rules: Vec<Rule>,
}

fn default_pool_idle() -> Duration {
    Duration::from_secs(90)
}

fn default_pool_max_per_host() -> usize {
    4
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            listen_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), DEFAULT_PORT),
            ca_dir: default_ca_dir(),
            ca_common_name: "Packet Sniffer Local CA".to_string(),
            capture_enabled: true,
            max_body_bytes: 2 * 1024 * 1024,
            store_capacity: 2_000,
            connect_timeout: Duration::from_secs(10),
            body_idle_timeout: Duration::from_secs(120),
            upstream_extra_root: None,
            upstream_pool_idle: default_pool_idle(),
            upstream_pool_max_per_host: default_pool_max_per_host(),
            rules: Vec::new(),
        }
    }
}

impl EngineConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_listen_addr(mut self, listen_addr: SocketAddr) -> Self {
        self.listen_addr = listen_addr;
        self
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.listen_addr.set_port(port);
        self
    }

    pub fn with_ca_dir(mut self, ca_dir: impl Into<PathBuf>) -> Self {
        self.ca_dir = ca_dir.into();
        self
    }

    pub fn with_capture(mut self, capture_enabled: bool) -> Self {
        self.capture_enabled = capture_enabled;
        self
    }

    pub fn with_max_body_bytes(mut self, max_body_bytes: usize) -> Self {
        self.max_body_bytes = max_body_bytes;
        self
    }

    pub fn with_store_capacity(mut self, store_capacity: usize) -> Self {
        self.store_capacity = store_capacity;
        self
    }

    pub fn with_upstream_extra_root(mut self, path: Option<PathBuf>) -> Self {
        self.upstream_extra_root = path;
        self
    }

    pub fn with_rules(mut self, rules: Vec<Rule>) -> Self {
        self.rules = rules;
        self
    }

    /// 调整上游连接池；`max_per_host` 为 0 或 `idle` 为 0 时关闭复用。
    pub fn with_upstream_pool(mut self, idle: Duration, max_per_host: usize) -> Self {
        self.upstream_pool_idle = idle;
        self.upstream_pool_max_per_host = max_per_host;
        self
    }

    /// 根证书路径。
    pub fn ca_cert_path(&self) -> PathBuf {
        self.ca_dir.join("root-ca.pem")
    }

    /// 根证书私钥路径。
    pub fn ca_key_path(&self) -> PathBuf {
        self.ca_dir.join("root-ca.key")
    }
}

/// 默认证书目录：`~/.packet-sniffer/certs`，取不到家目录时退回当前目录。
pub fn default_ca_dir() -> PathBuf {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    match home {
        Some(home) => PathBuf::from(home).join(".packet-sniffer").join("certs"),
        None => PathBuf::from("certs"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_overrides_fields() {
        let config = EngineConfig::new()
            .with_port(9100)
            .with_capture(false)
            .with_store_capacity(10)
            .with_max_body_bytes(1024);

        assert_eq!(config.listen_addr.port(), 9100);
        assert!(!config.capture_enabled);
        assert_eq!(config.store_capacity, 10);
        assert_eq!(config.max_body_bytes, 1024);
    }

    #[test]
    fn upstream_pool_is_configurable() {
        let config = EngineConfig::new().with_upstream_pool(Duration::from_secs(5), 2);
        assert_eq!(config.upstream_pool_idle, Duration::from_secs(5));
        assert_eq!(config.upstream_pool_max_per_host, 2);

        let disabled = EngineConfig::new().with_upstream_pool(Duration::from_secs(5), 0);
        assert_eq!(disabled.upstream_pool_max_per_host, 0);
    }

    #[test]
    fn ca_paths_live_in_ca_dir() {
        let config = EngineConfig::new().with_ca_dir("/tmp/pk");
        assert_eq!(config.ca_cert_path(), PathBuf::from("/tmp/pk/root-ca.pem"));
        assert_eq!(config.ca_key_path(), PathBuf::from("/tmp/pk/root-ca.key"));
    }

    #[test]
    fn defaults_bind_loopback() {
        let config = EngineConfig::default();
        assert!(config.listen_addr.ip().is_loopback());
        assert_eq!(config.listen_addr.port(), DEFAULT_PORT);
    }
}
