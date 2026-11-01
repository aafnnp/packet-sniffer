//! 系统代理设置。

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod unsupported;

#[cfg(target_os = "linux")]
pub use linux::{apply, clear, current};
#[cfg(target_os = "macos")]
pub use macos::{apply, clear, current};
#[cfg(target_os = "windows")]
pub use windows::{apply, clear, current};

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub use unsupported::{apply, clear, current};

/// 系统代理配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxySettings {
    pub host: String,
    pub port: u16,
}

impl ProxySettings {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
        }
    }

    /// `host:port` 形式，部分平台（Windows 注册表）需要。
    pub fn authority(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

impl Default for ProxySettings {
    fn default() -> Self {
        Self::new("127.0.0.1", 8899)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authority_joins_host_and_port() {
        assert_eq!(
            ProxySettings::new("127.0.0.1", 8899).authority(),
            "127.0.0.1:8899"
        );
        assert_eq!(ProxySettings::default().port, 8899);
    }
}
