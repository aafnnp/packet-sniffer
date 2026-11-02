//! 请求 / 响应结构体与引擎事件枚举。
//!
//! 所有跨层数据结构都定义在此，避免引擎、系统适配层与 UI 之间重复定义与转换。

use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 单次请求的全局唯一标识，同时用于列表索引与详情查询。
pub type RequestId = Uuid;

/// 请求使用的传输协议。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scheme {
    Http,
    Https,
}

impl Scheme {
    pub const fn as_str(self) -> &'static str {
        match self {
            Scheme::Http => "http",
            Scheme::Https => "https",
        }
    }

    pub const fn default_port(self) -> u16 {
        match self {
            Scheme::Http => 80,
            Scheme::Https => 443,
        }
    }
}

impl fmt::Display for Scheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 首部字段的保序表示，序列化友好，可直接透传给 UI。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub name: String,
    pub value: String,
}

impl Header {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

impl fmt::Display for Header {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.name, self.value)
    }
}

/// 抓取到的请求。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedRequest {
    pub id: RequestId,
    pub scheme: Scheme,
    pub method: String,
    /// 完整 URL（已补全 scheme 与端口）。
    pub url: String,
    pub host: String,
    pub port: u16,
    /// 路径 + 查询串，不含 scheme 与 authority。
    pub path: String,
    pub version: String,
    pub headers: Vec<Header>,
    pub body: Vec<u8>,
    /// 体积超过抓取上限，body 只是前缀。
    pub body_truncated: bool,
    /// body 是否已经读完（流式请求体可能未读完就收到响应）。
    pub body_complete: bool,
    pub started_at: DateTime<Utc>,
}

impl CapturedRequest {
    /// 大小写不敏感地查询首部字段。
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.as_str())
    }

    pub fn body_len(&self) -> usize {
        self.body.len()
    }

    /// 生成列表页使用的轻量摘要（用于请求刚到达、响应尚未返回时）。
    pub fn summary(&self) -> RequestSummary {
        RequestSummary {
            id: self.id,
            scheme: self.scheme,
            method: self.method.clone(),
            host: self.host.clone(),
            path: self.path.clone(),
            url: self.url.clone(),
            status: None,
            duration_ms: 0,
            request_bytes: self.body.len() as u64,
            response_bytes: 0,
            started_at: self.started_at,
            completed: false,
            error: None,
        }
    }
}

/// 抓取到的响应。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedResponse {
    pub status: u16,
    pub status_text: String,
    pub version: String,
    pub headers: Vec<Header>,
    pub body: Vec<u8>,
    pub body_truncated: bool,
    pub body_complete: bool,
    /// 响应尾部首部（HTTP/2 与 gRPC 用它承载 `grpc-status` 等元数据）。
    #[serde(default)]
    pub trailers: Vec<Header>,
    pub received_at: DateTime<Utc>,
}

impl CapturedResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.as_str())
    }

    pub fn body_len(&self) -> usize {
        self.body.len()
    }

    pub fn is_success(&self) -> bool {
        (200..400).contains(&self.status)
    }
}

/// 一次完整的请求-响应往返。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedExchange {
    pub request: CapturedRequest,
    /// 尚未收到响应时为 `None`（例如连接失败或请求仍在进行中）。
    pub response: Option<CapturedResponse>,
    pub error: Option<String>,
    pub duration_ms: u64,
    pub completed_at: Option<DateTime<Utc>>,
}

impl CapturedExchange {
    pub fn id(&self) -> RequestId {
        self.request.id
    }

    pub fn is_completed(&self) -> bool {
        self.response.is_some() || self.error.is_some()
    }

    pub fn status(&self) -> Option<u16> {
        self.response.as_ref().map(|r| r.status)
    }

    /// 生成列表页使用的轻量摘要，避免 UI 列表持有完整 body。
    pub fn summary(&self) -> RequestSummary {
        RequestSummary {
            status: self.status(),
            duration_ms: self.duration_ms,
            response_bytes: self.response.as_ref().map_or(0, |r| r.body.len() as u64),
            completed: self.is_completed(),
            error: self.error.clone(),
            ..self.request.summary()
        }
    }
}

/// 列表页展示用的请求摘要。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestSummary {
    pub id: RequestId,
    pub scheme: Scheme,
    pub method: String,
    pub host: String,
    pub path: String,
    pub url: String,
    pub status: Option<u16>,
    pub duration_ms: u64,
    pub request_bytes: u64,
    pub response_bytes: u64,
    pub started_at: DateTime<Utc>,
    pub completed: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

/// 引擎向 UI 推送的事件，是引擎与 UI 之间唯一的耦合面。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EngineEvent {
    /// 引擎开始监听。
    Started {
        listen_addr: String,
        ca_cert_path: String,
    },
    /// 引擎已停止。
    Stopped,
    /// 收到新请求。
    RequestStarted { request: CapturedRequest },
    /// 请求完成（可能带错误），包含更新后的完整交换记录。
    RequestCompleted { exchange: CapturedExchange },
    /// 引擎级日志 / 错误。
    Log { level: LogLevel, message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_request() -> CapturedRequest {
        CapturedRequest {
            id: Uuid::new_v4(),
            scheme: Scheme::Https,
            method: "GET".into(),
            url: "https://example.com/a?b=1".into(),
            host: "example.com".into(),
            port: 443,
            path: "/a?b=1".into(),
            version: "HTTP/1.1".into(),
            headers: vec![Header::new("Host", "example.com")],
            body: Vec::new(),
            body_truncated: false,
            body_complete: true,
            started_at: Utc::now(),
        }
    }

    #[test]
    fn header_lookup_is_case_insensitive() {
        let req = sample_request();
        assert_eq!(req.header("host"), Some("example.com"));
        assert_eq!(req.header("HOST"), Some("example.com"));
        assert_eq!(req.header("missing"), None);
    }

    #[test]
    fn summary_reflects_completion_state() {
        let exchange = CapturedExchange {
            request: sample_request(),
            response: None,
            error: None,
            duration_ms: 12,
            completed_at: None,
        };
        let summary = exchange.summary();
        assert!(!summary.completed);
        assert_eq!(summary.status, None);
        assert_eq!(summary.duration_ms, 12);
    }

    #[test]
    fn scheme_defaults_match_well_known_ports() {
        assert_eq!(Scheme::Http.default_port(), 80);
        assert_eq!(Scheme::Https.default_port(), 443);
        assert_eq!(Scheme::Https.to_string(), "https");
    }
}
