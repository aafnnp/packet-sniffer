//! HTTP/1.1 报文的辅助处理：目标地址解析、首部转换与逐跳首部过滤。

use common::types::{Header, Scheme};
use common::utils::split_authority;
use common::CommonError;
use http::header::HeaderMap;
use http::{StatusCode, Uri, Version};

/// 请求的实际转发目标。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestTarget {
    pub scheme: Scheme,
    pub host: String,
    pub port: u16,
    /// 路径 + 查询串（origin-form），用于向上游发起请求。
    pub path: String,
    /// 完整 URL，用于列表展示。
    pub url: String,
}

impl RequestTarget {
    /// 不带默认端口的 authority，用于拼接展示用 URL。
    pub fn authority(&self) -> String {
        if self.port == self.scheme.default_port() {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

/// 逐跳首部，转发时必须按方向重新生成，不能原样透传。
const HOP_BY_HOP: &[&str] = &[
    "connection",
    "proxy-connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// 从请求行与首部解析转发目标。
///
/// 代理请求使用 absolute-form（`GET http://host/path`），MITM 内部请求使用
/// origin-form（`GET /path` + `Host`）。隧道内的请求还带有 CONNECT 阶段确定的
/// `authority`，它是上游连接的真实目标，优先于 `Host` 首部。
pub fn resolve_target(
    uri: &Uri,
    headers: &HeaderMap,
    fallback_scheme: Scheme,
    fallback_authority: Option<&str>,
) -> Result<RequestTarget, CommonError> {
    let scheme = match uri.scheme_str() {
        Some("http") => Scheme::Http,
        Some("https") => Scheme::Https,
        Some(other) => {
            return Err(CommonError::InvalidUrl(format!("不支持的协议：{other}")));
        }
        None => fallback_scheme,
    };

    let authority = match uri.authority() {
        Some(authority) => authority.as_str().to_string(),
        None => match fallback_authority {
            Some(authority) => authority.to_string(),
            None => header_str(headers, "host")
                .ok_or_else(|| CommonError::InvalidAuthority(uri.to_string()))?,
        },
    };

    let (host, port) = split_authority(&authority, scheme.default_port())?;
    let path = match uri.path_and_query() {
        Some(path_and_query) if !path_and_query.as_str().is_empty() => {
            path_and_query.as_str().to_string()
        }
        _ => "/".to_string(),
    };

    let target = RequestTarget {
        scheme,
        host,
        port,
        path,
        url: String::new(),
    };

    Ok(RequestTarget {
        url: format!("{}://{}{}", target.scheme, target.authority(), target.path),
        ..target
    })
}

fn header_str(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string())
}

/// `http::HeaderMap` → 共享类型的保序首部列表。
pub fn header_map_to_headers(headers: &HeaderMap) -> Vec<Header> {
    headers
        .iter()
        .map(|(name, value)| {
            Header::new(
                name.as_str(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
        .collect()
}

pub fn version_to_string(version: Version) -> String {
    match version {
        Version::HTTP_09 => "HTTP/0.9",
        Version::HTTP_10 => "HTTP/1.0",
        Version::HTTP_11 => "HTTP/1.1",
        Version::HTTP_2 => "HTTP/2.0",
        Version::HTTP_3 => "HTTP/3.0",
        other => return format!("{other:?}"),
    }
    .to_string()
}

pub fn status_text(status: StatusCode) -> String {
    status.canonical_reason().unwrap_or_default().to_string()
}

/// 过滤逐跳首部（含 `Connection` 中声明的扩展逐跳首部）。
pub fn filter_headers(headers: &HeaderMap) -> HeaderMap {
    let mut announced: Vec<String> = Vec::new();
    for value in headers.get_all("connection").iter() {
        if let Ok(value) = value.to_str() {
            announced.extend(
                value
                    .split(',')
                    .map(|token| token.trim().to_ascii_lowercase())
                    .filter(|token| !token.is_empty()),
            );
        }
    }

    let mut filtered = HeaderMap::new();
    for (name, value) in headers.iter() {
        let lower = name.as_str().to_ascii_lowercase();
        if HOP_BY_HOP.contains(&lower.as_str()) || announced.contains(&lower) {
            continue;
        }
        filtered.append(name.clone(), value.clone());
    }
    filtered
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::HeaderValue;

    #[test]
    fn resolves_absolute_form_proxy_request() {
        let uri: Uri = "http://example.com:8080/a?b=1".parse().unwrap();
        let target = resolve_target(&uri, &HeaderMap::new(), Scheme::Http, None).unwrap();
        assert_eq!(target.scheme, Scheme::Http);
        assert_eq!(target.host, "example.com");
        assert_eq!(target.port, 8080);
        assert_eq!(target.path, "/a?b=1");
        assert_eq!(target.url, "http://example.com:8080/a?b=1");
    }

    #[test]
    fn resolves_origin_form_with_host_header() {
        let uri: Uri = "/login".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("host", HeaderValue::from_static("api.example.com"));
        let target = resolve_target(&uri, &headers, Scheme::Https, None).unwrap();
        assert_eq!(target.scheme, Scheme::Https);
        assert_eq!(target.host, "api.example.com");
        assert_eq!(target.port, 443);
        assert_eq!(target.url, "https://api.example.com/login");
    }

    #[test]
    fn tunnel_authority_wins_over_host_header() {
        let uri: Uri = "/secret".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("host", HeaderValue::from_static("localhost"));
        let target = resolve_target(&uri, &headers, Scheme::Https, Some("127.0.0.1:8443")).unwrap();
        assert_eq!(target.host, "127.0.0.1");
        assert_eq!(target.port, 8443);
        assert_eq!(target.url, "https://127.0.0.1:8443/secret");
    }

    #[test]
    fn rejects_missing_authority() {
        let uri: Uri = "/no-host".parse().unwrap();
        assert!(resolve_target(&uri, &HeaderMap::new(), Scheme::Http, None).is_err());
    }

    #[test]
    fn url_keeps_non_default_port_only() {
        let uri: Uri = "https://example.com/".parse().unwrap();
        let target = resolve_target(&uri, &HeaderMap::new(), Scheme::Https, None).unwrap();
        assert_eq!(target.url, "https://example.com/");
    }

    #[test]
    fn filters_hop_by_hop_and_connection_tokens() {
        let mut headers = HeaderMap::new();
        headers.insert("connection", HeaderValue::from_static("x-custom, close"));
        headers.insert("x-custom", HeaderValue::from_static("1"));
        headers.insert("proxy-connection", HeaderValue::from_static("keep-alive"));
        headers.insert("content-type", HeaderValue::from_static("text/plain"));

        let filtered = filter_headers(&headers);
        assert!(filtered.get("connection").is_none());
        assert!(filtered.get("x-custom").is_none());
        assert!(filtered.get("proxy-connection").is_none());
        assert_eq!(filtered.get("content-type").unwrap(), "text/plain");
    }

    #[test]
    fn converts_headers_to_shared_types() {
        let mut headers = HeaderMap::new();
        headers.insert("x-a", HeaderValue::from_static("b"));
        let converted = header_map_to_headers(&headers);
        assert_eq!(converted, vec![Header::new("x-a", "b")]);
    }

    #[test]
    fn status_and_version_have_readable_text() {
        assert_eq!(status_text(StatusCode::NOT_FOUND), "Not Found");
        assert_eq!(version_to_string(Version::HTTP_11), "HTTP/1.1");
    }
}
