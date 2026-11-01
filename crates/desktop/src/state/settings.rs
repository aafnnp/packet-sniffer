//! 应用设置：持久化到 JSON，并负责转换成引擎配置。

use std::fs;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use proxy_engine::config::UpstreamProxy;
use proxy_engine::rules::Rule;
use proxy_engine::EngineConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// 结构级 `serde(default)`：新增字段不会让已有配置文件解析失败。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub listen_addr: SocketAddr,
    pub capture_enabled: bool,
    pub max_body_bytes: usize,
    pub store_capacity: usize,
    pub ca_dir: PathBuf,
    /// 是否在引擎启动后自动接管系统代理。
    pub system_proxy: bool,
    pub theme: Theme,
    /// 链式代理（例如本机已有的 127.0.0.1:8118），为空表示直连。
    pub upstream_proxy: Option<UpstreamProxy>,
    /// 请求 / 响应规则。
    #[serde(default)]
    pub rules: Vec<Rule>,
}

impl Default for Settings {
    fn default() -> Self {
        let engine = EngineConfig::default();
        Self {
            listen_addr: engine.listen_addr,
            capture_enabled: engine.capture_enabled,
            max_body_bytes: engine.max_body_bytes,
            store_capacity: engine.store_capacity,
            ca_dir: engine.ca_dir,
            system_proxy: false,
            theme: Theme::System,
            upstream_proxy: None,
            rules: Vec::new(),
        }
    }
}

impl Settings {
    /// 读取设置；文件不存在或损坏时回落到默认值，不阻断启动。
    pub fn load(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str(&text) {
                Ok(settings) => settings,
                Err(err) => {
                    tracing::warn!(error = %err, path = %path.display(), "设置文件解析失败，使用默认设置");
                    Self::default()
                }
            },
            Err(err) if err.kind() == io::ErrorKind::NotFound => Self::default(),
            Err(err) => {
                tracing::warn!(error = %err, path = %path.display(), "设置文件读取失败，使用默认设置");
                Self::default()
            }
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        fs::write(path, text)
    }

    pub fn engine_config(&self) -> EngineConfig {
        EngineConfig::new()
            .with_listen_addr(self.listen_addr)
            .with_capture(self.capture_enabled)
            .with_max_body_bytes(self.max_body_bytes)
            .with_store_capacity(self.store_capacity)
            .with_ca_dir(self.ca_dir.clone())
            .with_upstream_proxy(self.upstream_proxy.clone())
            .with_rules(self.rules.clone())
    }

    pub fn proxy_authority(&self) -> String {
        format!("{}:{}", self.listen_addr.ip(), self.listen_addr.port())
    }
}

/// `~/.packet-sniffer/settings.json`
pub fn default_settings_path() -> PathBuf {
    match std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        Some(home) => PathBuf::from(home)
            .join(".packet-sniffer")
            .join("settings.json"),
        None => PathBuf::from("settings.json"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "packet-sniffer-settings-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir.join("settings.json")
    }

    #[test]
    fn save_then_load_roundtrip() {
        let path = temp_path("roundtrip");
        let mut settings = Settings::default();
        settings.listen_addr.set_port(9001);
        settings.capture_enabled = false;
        settings.theme = Theme::Dark;

        settings.save(&path).unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded.listen_addr.port(), 9001);
        assert!(!loaded.capture_enabled);
        assert_eq!(loaded.theme, Theme::Dark);

        fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn missing_file_falls_back_to_defaults() {
        let path = temp_path("missing");
        assert_eq!(Settings::load(&path), Settings::default());
    }

    #[test]
    fn corrupt_file_falls_back_to_defaults() {
        let path = temp_path("corrupt");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{not json").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn legacy_settings_without_upstream_proxy_still_parse() {
        let path = temp_path("legacy");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{
                "listen_addr": "127.0.0.1:8899",
                "capture_enabled": true,
                "max_body_bytes": 1024,
                "store_capacity": 100,
                "ca_dir": "/tmp/certs",
                "system_proxy": false,
                "theme": "dark"
            }"#,
        )
        .unwrap();

        let loaded = Settings::load(&path);
        assert_eq!(loaded.theme, Theme::Dark);
        assert!(loaded.upstream_proxy.is_none());
        fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn rules_roundtrip_and_reach_engine_config() {
        use proxy_engine::rules::{Rule, RuleAction};

        let path = temp_path("rules");
        let mut settings = Settings::default();
        settings.rules.push(
            Rule::new(
                "mock",
                RuleAction::Mock {
                    status: 201,
                    content_type: "application/json".into(),
                    body: "{}".into(),
                },
            )
            .with_url("/api"),
        );
        settings.save(&path).unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded.rules.len(), 1);
        assert_eq!(loaded.rules[0].name, "mock");
        assert_eq!(loaded.engine_config().rules.len(), 1);
        fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn engine_config_carries_upstream_proxy() {
        let settings = Settings {
            upstream_proxy: Some(UpstreamProxy::new("127.0.0.1", 8118)),
            ..Settings::default()
        };
        let config = settings.engine_config();
        assert_eq!(
            config
                .upstream_proxy
                .as_ref()
                .map(|proxy| proxy.authority()),
            Some("127.0.0.1:8118".to_string())
        );
    }

    #[test]
    fn engine_config_mirrors_settings() {
        let mut settings = Settings::default();
        settings.listen_addr.set_port(8898);
        settings.store_capacity = 42;
        settings.max_body_bytes = 128;

        let config = settings.engine_config();
        assert_eq!(config.listen_addr.port(), 8898);
        assert_eq!(config.store_capacity, 42);
        assert_eq!(config.max_body_bytes, 128);
        assert_eq!(settings.proxy_authority(), "127.0.0.1:8898");
    }
}
