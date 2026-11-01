//! 轻量规则引擎：按方法 / URL / 域名匹配，拦截、模拟响应或改写首部。

use common::types::CapturedRequest;
use serde::{Deserialize, Serialize};

/// 一条规则。空字符串表示该条件不限制。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    /// 只匹配该方法（大小写不敏感）；为空匹配全部。
    pub method: Option<String>,
    /// URL 包含该子串时匹配。
    pub url_contains: String,
    /// Host 以该后缀结尾时匹配。
    pub host_suffix: String,
    pub action: RuleAction,
}

impl Default for Rule {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: "新规则".to_string(),
            enabled: true,
            method: None,
            url_contains: String::new(),
            host_suffix: String::new(),
            action: RuleAction::Block { status: 403 },
        }
    }
}

impl Rule {
    pub fn new(name: impl Into<String>, action: RuleAction) -> Self {
        Self {
            name: name.into(),
            action,
            ..Self::default()
        }
    }

    pub fn with_method(mut self, method: impl Into<String>) -> Self {
        self.method = Some(method.into());
        self
    }

    pub fn with_url(mut self, contains: impl Into<String>) -> Self {
        self.url_contains = contains.into();
        self
    }

    pub fn with_host(mut self, suffix: impl Into<String>) -> Self {
        self.host_suffix = suffix.into();
        self
    }

    pub fn matches(&self, request: &CapturedRequest) -> bool {
        if !self.enabled {
            return false;
        }
        if let Some(method) = &self.method {
            if !method.is_empty() && !request.method.eq_ignore_ascii_case(method.trim()) {
                return false;
            }
        }
        if !self.url_contains.trim().is_empty()
            && !request
                .url
                .to_lowercase()
                .contains(&self.url_contains.trim().to_lowercase())
        {
            return false;
        }
        if !self.host_suffix.trim().is_empty()
            && !request
                .host
                .to_lowercase()
                .ends_with(&self.host_suffix.trim().to_lowercase())
        {
            return false;
        }
        true
    }

    /// 是否结束转发（阻止或直接返回模拟响应）。
    pub fn terminal(&self) -> bool {
        matches!(
            self.action,
            RuleAction::Block { .. } | RuleAction::Mock { .. }
        )
    }

    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if let Some(method) = &self.method {
            if !method.is_empty() {
                parts.push(method.to_uppercase());
            }
        }
        if !self.url_contains.is_empty() {
            parts.push(format!("URL 含 {}", self.url_contains));
        }
        if !self.host_suffix.is_empty() {
            parts.push(format!("域名 *{}", self.host_suffix));
        }
        if parts.is_empty() {
            "全部请求".to_string()
        } else {
            parts.join(" · ")
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuleAction {
    /// 直接返回状态码，不连接上游。
    Block { status: u16 },
    /// 直接返回一段模拟响应。
    Mock {
        status: u16,
        content_type: String,
        body: String,
    },
    /// 在请求发往上游前设置 / 覆盖首部。
    AddRequestHeader { name: String, value: String },
    /// 在响应返回客户端前设置 / 覆盖首部。
    AddResponseHeader { name: String, value: String },
}

impl Default for RuleAction {
    fn default() -> Self {
        RuleAction::Block { status: 403 }
    }
}

impl RuleAction {
    pub fn label(&self) -> &'static str {
        match self {
            RuleAction::Block { .. } => "阻止请求",
            RuleAction::Mock { .. } => "模拟响应",
            RuleAction::AddRequestHeader { .. } => "添加请求头",
            RuleAction::AddResponseHeader { .. } => "添加响应头",
        }
    }

    pub fn description(&self) -> String {
        match self {
            RuleAction::Block { status } => format!("返回 {status}"),
            RuleAction::Mock {
                status,
                content_type,
                body,
            } => format!("{status} · {content_type} · {} 字节", body.len()),
            RuleAction::AddRequestHeader { name, value }
            | RuleAction::AddResponseHeader { name, value } => {
                format!("{name}: {value}")
            }
        }
    }
}

/// 所有匹配的规则（按配置顺序）。
pub fn matching<'a>(rules: &'a [Rule], request: &CapturedRequest) -> Vec<&'a Rule> {
    rules.iter().filter(|rule| rule.matches(request)).collect()
}

/// 第一条会终止转发的规则。
pub fn terminal<'a>(matched: &[&'a Rule]) -> Option<&'a Rule> {
    matched.iter().copied().find(|rule| rule.terminal())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use common::types::Scheme;

    fn request(method: &str, url: &str, host: &str) -> CapturedRequest {
        CapturedRequest {
            id: uuid::Uuid::new_v4(),
            scheme: Scheme::Https,
            method: method.into(),
            url: url.into(),
            host: host.into(),
            port: 443,
            path: "/".into(),
            version: "HTTP/1.1".into(),
            headers: Vec::new(),
            body: Vec::new(),
            body_truncated: false,
            body_complete: true,
            started_at: Utc::now(),
        }
    }

    #[test]
    fn matches_method_url_and_host() {
        let rule = Rule::new(
            "mock",
            RuleAction::Mock {
                status: 200,
                content_type: "application/json".into(),
                body: "{}".into(),
            },
        )
        .with_method("post")
        .with_url("/api/")
        .with_host("example.com");

        assert!(rule.matches(&request(
            "POST",
            "https://api.example.com/api/users",
            "api.example.com"
        )));
        assert!(!rule.matches(&request(
            "GET",
            "https://api.example.com/api/users",
            "api.example.com"
        )));
        assert!(!rule.matches(&request(
            "POST",
            "https://api.example.com/other",
            "api.example.com"
        )));
        assert!(!rule.matches(&request("POST", "https://other.test/api/x", "other.test")));
    }

    #[test]
    fn disabled_rules_never_match() {
        let mut rule = Rule::new("block", RuleAction::Block { status: 403 });
        assert!(rule.matches(&request("GET", "https://example.com/", "example.com")));
        rule.enabled = false;
        assert!(!rule.matches(&request("GET", "https://example.com/", "example.com")));
    }

    #[test]
    fn terminal_picks_the_first_block_or_mock() {
        let header = Rule::new(
            "header",
            RuleAction::AddRequestHeader {
                name: "x-test".into(),
                value: "1".into(),
            },
        );
        let mock = Rule::new(
            "mock",
            RuleAction::Mock {
                status: 200,
                content_type: "text/plain".into(),
                body: "ok".into(),
            },
        );
        let rules = vec![header, mock];
        let req = request("GET", "https://example.com/", "example.com");
        let matched = matching(&rules, &req);
        assert_eq!(matched.len(), 2);
        assert_eq!(
            terminal(&matched).map(|rule| rule.name.as_str()),
            Some("mock")
        );
    }
}
