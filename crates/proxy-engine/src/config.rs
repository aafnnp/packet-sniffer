//! 引擎配置。

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use common::utils::base64_encode;
use common::CommonError;
use serde::{Deserialize, Serialize};

use crate::rules::Rule;

/// 默认监听端口。
pub const DEFAULT_PORT: u16 = 8899;

/// 链式代理目标：把请求再交给另一个 HTTP 代理（企业代理、本地代理客户端等）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpstreamProxy {
    pub host: String,
    pub port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

impl UpstreamProxy {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            username: None,
            password: None,
        }
    }

    /// 解析 `host:port` 或 `http://user:password@host:port`。
    pub fn parse(value: &str) -> Result<Self, CommonError> {
        let value = value.trim();
        if value.is_empty() {
            return Err(CommonError::InvalidAuthority(value.to_string()));
        }

        let normalized = if value.contains("://") {
            value.to_string()
        } else {
            format!("http://{value}")
        };
        let url = url::Url::parse(&normalized)
            .map_err(|err| CommonError::InvalidUrl(format!("{value}：{err}")))?;

        if !matches!(url.scheme(), "http" | "https") {
            return Err(CommonError::InvalidUrl(format!(
                "{value}：只支持 http/https 代理"
            )));
        }

        let host = url
            .host_str()
            .ok_or_else(|| CommonError::InvalidAuthority(value.to_string()))?
            .to_string();
        let port = url
            .port()
            .ok_or_else(|| CommonError::InvalidPort(value.to_string()))?;

        Ok(Self {
            host,
            port,
            username: (!url.username().is_empty()).then(|| url.username().to_string()),
            password: url.password().map(str::to_string),
        })
    }

    pub fn authority(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// `Proxy-Authorization` 首部值（Basic 认证）。
    pub fn authorization_header(&self) -> Option<String> {
        let username = self.username.as_deref()?;
        let raw = format!(
            "{username}:{}",
            self.password.as_deref().unwrap_or_default()
        );
        Some(format!("Basic {}", base64_encode(raw.as_bytes())))
    }
}

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
    /// 链式代理：为空表示直连；配置后所有上游连接先经该代理建立隧道。
    pub upstream_proxy: Option<UpstreamProxy>,
    /// 请求 / 响应规则，按顺序匹配。
    #[serde(default)]
    pub rules: Vec<Rule>,
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
            upstream_proxy: None,
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

    pub fn with_upstream_proxy(mut self, proxy: Option<UpstreamProxy>) -> Self {
        self.upstream_proxy = proxy;
        self
    }

    pub fn with_rules(mut self, rules: Vec<Rule>) -> Self {
        self.rules = rules;
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
    fn ca_paths_live_in_ca_dir() {
        let config = EngineConfig::new().with_ca_dir("/tmp/pk");
        assert_eq!(config.ca_cert_path(), PathBuf::from("/tmp/pk/root-ca.pem"));
        assert_eq!(config.ca_key_path(), PathBuf::from("/tmp/pk/root-ca.key"));
    }

    #[test]
    fn parses_upstream_proxy_forms() {
        assert_eq!(
            UpstreamProxy::parse("127.0.0.1:8118").unwrap(),
            UpstreamProxy::new("127.0.0.1", 8118)
        );
        assert_eq!(
            UpstreamProxy::parse("http://10.0.0.2:3128").unwrap(),
            UpstreamProxy::new("10.0.0.2", 3128)
        );

        let with_auth = UpstreamProxy::parse("http://user:pass@127.0.0.1:8118").unwrap();
        assert_eq!(with_auth.username.as_deref(), Some("user"));
        assert_eq!(with_auth.password.as_deref(), Some("pass"));
        assert_eq!(
            with_auth.authorization_header().as_deref(),
            Some("Basic dXNlcjpwYXNz")
        );
    }

    #[test]
    fn rejects_invalid_upstream_proxy() {
        assert!(UpstreamProxy::parse("").is_err());
        assert!(UpstreamProxy::parse("socks5://127.0.0.1:1080").is_err());
        assert!(UpstreamProxy::parse("127.0.0.1").is_err());
    }

    #[test]
    fn builder_sets_upstream_proxy() {
        let proxy = UpstreamProxy::new("127.0.0.1", 8118);
        let config = EngineConfig::new().with_upstream_proxy(Some(proxy.clone()));
        assert_eq!(config.upstream_proxy, Some(proxy));
    }

    #[test]
    fn defaults_bind_loopback() {
        let config = EngineConfig::default();
        assert!(config.listen_addr.ip().is_loopback());
        assert_eq!(config.listen_addr.port(), DEFAULT_PORT);
    }
}
