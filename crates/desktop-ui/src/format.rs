//! 列表与详情面板的格式化工具。

use chrono::{DateTime, Local, Utc};
use common::types::{CapturedRequest, CapturedResponse, Header};
use nsh_core::element::AdaptiveColor;

use crate::theme::colors;

pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    let value = bytes as f64;
    if value >= MB {
        format!("{:.1} MB", value / MB)
    } else if value >= KB {
        format!("{:.1} KB", value / KB)
    } else {
        format!("{bytes} B")
    }
}

pub fn format_duration(ms: u64) -> String {
    if ms >= 1000 {
        format!("{:.2} s", ms as f64 / 1000.0)
    } else {
        format!("{ms} ms")
    }
}

pub fn format_time(time: DateTime<Utc>) -> String {
    time.with_timezone(&Local)
        .format("%H:%M:%S%.3f")
        .to_string()
}

pub fn status_text(status: Option<u16>, completed: bool, error: Option<&str>) -> String {
    match status {
        Some(status) => status.to_string(),
        None if error.is_some() => "错误".to_string(),
        None if completed => "-".to_string(),
        None => "…".to_string(),
    }
}

pub fn status_color(status: Option<u16>, error: Option<&str>) -> AdaptiveColor {
    if error.is_some() {
        return colors::RED;
    }
    match status {
        Some(200..=299) => colors::GREEN,
        Some(300..=399) => colors::BLUE,
        Some(400..=499) => colors::ORANGE,
        Some(500..=599) => colors::RED,
        Some(_) => colors::GRAY,
        None => colors::FAINT,
    }
}

pub fn method_color(method: &str) -> AdaptiveColor {
    match method {
        "GET" => colors::GREEN,
        "POST" => colors::BLUE,
        "PUT" => colors::ORANGE,
        "PATCH" => colors::PURPLE,
        "DELETE" => colors::RED,
        "HEAD" | "OPTIONS" => colors::TEAL,
        "CONNECT" => colors::GRAY,
        _ => colors::MUTED,
    }
}

pub fn content_type(headers: &[Header]) -> Option<&str> {
    headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-type"))
        .map(|header| header.value.as_str())
}

pub fn is_json(content_type: Option<&str>) -> bool {
    content_type.is_some_and(|value| {
        let value = value.to_ascii_lowercase();
        value.contains("json") || value.contains("+json")
    })
}

pub fn is_textual(content_type: Option<&str>) -> bool {
    let Some(value) = content_type else {
        return false;
    };
    let value = value.to_ascii_lowercase();
    value.starts_with("text/")
        || value.contains("json")
        || value.contains("xml")
        || value.contains("javascript")
        || value.contains("x-www-form-urlencoded")
        || value.contains("graphql")
        || value.contains("html")
        || value.contains("css")
}

fn pretty_json(bytes: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    serde_json::to_string_pretty(&value).ok()
}

/// body 的可读预览：JSON 美化、文本原样、二进制十六进制。
pub fn body_preview(bytes: &[u8], content_type: Option<&str>) -> String {
    if bytes.is_empty() {
        return "（空）".to_string();
    }
    let looks_json = bytes
        .iter()
        .find(|byte| !byte.is_ascii_whitespace())
        .is_some_and(|byte| *byte == b'{' || *byte == b'[');
    if (is_json(content_type) || looks_json)
        && let Some(pretty) = pretty_json(bytes)
    {
        return pretty;
    }
    if is_textual(content_type) || std::str::from_utf8(bytes).is_ok() {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    hex_dump(bytes)
}

fn hex_dump(bytes: &[u8]) -> String {
    const LIMIT: usize = 4096;
    let shown = &bytes[..bytes.len().min(LIMIT)];
    let mut out = String::with_capacity(shown.len() * 4 + 64);
    for (index, chunk) in shown.chunks(16).enumerate() {
        out.push_str(&format!("{:08x}  ", index * 16));
        for byte in chunk {
            out.push_str(&format!("{byte:02x} "));
        }
        if chunk.len() < 16 {
            for _ in chunk.len()..16 {
                out.push_str("   ");
            }
        }
        out.push_str(" |");
        for byte in chunk {
            let c = if byte.is_ascii_graphic() || *byte == b' ' {
                *byte as char
            } else {
                '.'
            };
            out.push(c);
        }
        out.push_str("|\n");
    }
    if bytes.len() > LIMIT {
        out.push_str(&format!(
            "\n… 已省略 {}（共 {}）",
            format_bytes((bytes.len() - LIMIT) as u64),
            format_bytes(bytes.len() as u64)
        ));
    }
    out
}

/// body 的原始文本（不美化 JSON），用于「原始」标签页。
pub fn body_raw(bytes: &[u8], content_type: Option<&str>) -> String {
    if bytes.is_empty() {
        return "（空）".to_string();
    }
    if is_textual(content_type) || std::str::from_utf8(bytes).is_ok() {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    hex_dump(bytes)
}

pub fn headers_rows(headers: &[Header]) -> Vec<(String, String)> {
    headers
        .iter()
        .map(|header| (header.name.clone(), header.value.clone()))
        .collect()
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// 把请求复制成可再次执行的 cURL 命令。
pub fn curl(request: &CapturedRequest) -> String {
    let mut out = format!(
        "curl -X {} --url {}",
        request.method,
        shell_quote(&request.url)
    );
    for header in &request.headers {
        if header.name.eq_ignore_ascii_case("proxy-connection")
            || header.name.eq_ignore_ascii_case("content-length")
        {
            continue;
        }
        out.push_str(" \\\n  -H ");
        out.push_str(&shell_quote(&format!("{}: {}", header.name, header.value)));
    }
    if !request.body.is_empty() {
        out.push_str(" \\\n  --data-raw ");
        out.push_str(&shell_quote(&body_raw(
            &request.body,
            content_type(&request.headers),
        )));
    }
    out
}

pub fn raw_request(request: &CapturedRequest) -> String {
    let mut out = format!(
        "{} {} {}\r\n",
        request.method, request.path, request.version
    );
    for header in &request.headers {
        out.push_str(&format!("{}: {}\r\n", header.name, header.value));
    }
    if !request.body.is_empty() {
        out.push_str("\r\n");
        out.push_str(&body_raw(&request.body, content_type(&request.headers)));
    }
    out
}

pub fn raw_response(response: &CapturedResponse) -> String {
    let mut out = format!(
        "{} {} {}\r\n",
        response.version, response.status, response.status_text
    );
    for header in &response.headers {
        out.push_str(&format!("{}: {}\r\n", header.name, header.value));
    }
    if !response.body.is_empty() {
        out.push_str("\r\n");
        out.push_str(&body_preview(
            &response.body,
            content_type(&response.headers),
        ));
    }
    out
}

pub fn query_pairs(url: &str) -> Vec<(String, String)> {
    let Ok(parsed) = url::Url::parse(url) else {
        return Vec::new();
    };
    parsed
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect()
}

/// 从 `User-Agent` 推断客户端名称，用于侧边栏的「应用」分组。
pub fn client_name(request: &CapturedRequest) -> String {
    let Some(agent) = request.header("user-agent") else {
        return "未知".to_string();
    };
    let agent = agent.to_ascii_lowercase();
    if agent.contains("edg/") {
        "Microsoft Edge"
    } else if agent.contains("chrome/") {
        "Google Chrome"
    } else if agent.contains("safari/") {
        "Safari"
    } else if agent.contains("firefox/") {
        "Firefox"
    } else if agent.contains("curl/") {
        "curl"
    } else if agent.contains("python") {
        "Python"
    } else if agent.contains("node") {
        "Node.js"
    } else if agent.contains("postman") {
        "Postman"
    } else if agent.contains("proxyman") {
        "Proxyman"
    } else if agent.contains("electron") {
        "Electron"
    } else if agent.contains("okhttp") {
        "OkHttp"
    } else {
        "其他"
    }
    .to_string()
}

/// `content-type` 的可读种类。
pub fn preview_kind(content_type: Option<&str>) -> &'static str {
    let Some(value) = content_type else {
        return "内容";
    };
    let value = value.to_ascii_lowercase();
    if value.contains("json") {
        "JSON"
    } else if value.contains("html") {
        "HTML"
    } else if value.contains("xml") {
        "XML"
    } else if value.contains("css") {
        "CSS"
    } else if value.contains("javascript") {
        "JavaScript"
    } else if value.starts_with("text/") {
        "文本"
    } else if value.contains("image/") {
        "图片"
    } else if value.contains("font") {
        "字体"
    } else {
        "二进制"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::types::Header;

    #[test]
    fn bytes_and_durations_are_human_readable() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(format_duration(42), "42 ms");
        assert_eq!(format_duration(1500), "1.50 s");
    }

    #[test]
    fn json_bodies_are_pretty_printed() {
        let body = br#"{"hello":"world","items":[1,2]}"#;
        let text = body_preview(body, Some("application/json; charset=utf-8"));
        assert!(text.contains("\n  \"hello\": \"world\""));
    }

    #[test]
    fn non_json_text_is_kept_and_binary_is_hex_dumped() {
        assert_eq!(body_preview(b"plain", Some("text/plain")), "plain");
        let dump = body_preview(&[0x00, 0xff, b'A'], Some("application/octet-stream"));
        assert!(dump.contains("00 ff 41"));
    }

    #[test]
    fn query_pairs_come_from_the_url() {
        let pairs = query_pairs("https://example.com/search?q=nsh&page=2");
        assert_eq!(
            pairs,
            vec![
                ("q".to_string(), "nsh".to_string()),
                ("page".to_string(), "2".to_string())
            ]
        );
    }

    #[test]
    fn curl_command_quotes_headers_and_body() {
        let request = CapturedRequest {
            id: uuid::Uuid::new_v4(),
            scheme: common::types::Scheme::Https,
            method: "POST".into(),
            url: "https://example.com/api?q=1".into(),
            host: "example.com".into(),
            port: 443,
            path: "/api?q=1".into(),
            version: "HTTP/1.1".into(),
            headers: vec![
                Header::new("Content-Type", "application/json"),
                Header::new("Proxy-Connection", "keep-alive"),
            ],
            body: br#"{"hello":"it's"}"#.to_vec(),
            body_truncated: false,
            body_complete: true,
            started_at: Utc::now(),
        };
        let command = curl(&request);
        assert!(command.starts_with("curl -X POST --url 'https://example.com/api?q=1'"));
        assert!(command.contains("-H 'Content-Type: application/json'"));
        assert!(!command.contains("Proxy-Connection"));
        assert!(command.contains("--data-raw"));
    }

    #[test]
    fn clients_are_named_from_the_user_agent() {
        let request = |agent: &str| CapturedRequest {
            id: uuid::Uuid::new_v4(),
            scheme: common::types::Scheme::Http,
            method: "GET".into(),
            url: "http://example.com/".into(),
            host: "example.com".into(),
            port: 80,
            path: "/".into(),
            version: "HTTP/1.1".into(),
            headers: vec![Header::new("User-Agent", agent)],
            body: Vec::new(),
            body_truncated: false,
            body_complete: true,
            started_at: Utc::now(),
        };
        assert_eq!(client_name(&request("curl/8.7.1")), "curl");
        assert_eq!(
            client_name(&request(
                "Mozilla/5.0 (Macintosh) Chrome/120.0 Safari/537.36"
            )),
            "Google Chrome"
        );
        assert_eq!(client_name(&request("unknown/1.0")), "其他");
    }
}
