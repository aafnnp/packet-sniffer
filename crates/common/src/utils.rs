//! 通用工具函数。

use crate::error::{CommonError, Result};

/// 把 `host:port` / `host` / `[::1]:port` 形式的 authority 拆成主机与端口。
pub fn split_authority(authority: &str, default_port: u16) -> Result<(String, u16)> {
    let authority = authority.trim();
    if authority.is_empty() {
        return Err(CommonError::InvalidAuthority(authority.to_string()));
    }

    if let Some(rest) = authority.strip_prefix('[') {
        let (host, tail) = rest
            .split_once(']')
            .ok_or_else(|| CommonError::InvalidAuthority(authority.to_string()))?;
        let port = match tail.strip_prefix(':') {
            Some(port) if !port.is_empty() => port
                .parse()
                .map_err(|_| CommonError::InvalidPort(port.to_string()))?,
            _ => default_port,
        };
        return Ok((host.to_string(), port));
    }

    match authority.rsplit_once(':') {
        Some((host, port))
            if !host.is_empty() && !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) =>
        {
            let port = port
                .parse()
                .map_err(|_| CommonError::InvalidPort(port.to_string()))?;
            Ok((host.to_string(), port))
        }
        _ => Ok((authority.to_string(), default_port)),
    }
}

/// 只截取前缀，返回 `(前缀, 是否被截断)`。
pub fn truncate_bytes(data: &[u8], limit: usize) -> (Vec<u8>, bool) {
    if data.len() <= limit {
        (data.to_vec(), false)
    } else {
        (data[..limit].to_vec(), true)
    }
}

/// 人类可读的字节数。
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// 把 body 转成用于详情面板展示的文本预览。
pub fn preview_text(body: &[u8], limit: usize) -> String {
    let (slice, truncated) = truncate_bytes(body, limit);
    let mut text = String::from_utf8_lossy(&slice).into_owned();
    if truncated {
        text.push_str("\n…（内容已截断）");
    }
    text
}

/// 判断 body 是否大概率是可读文本（用于 UI 选择文本 / 十六进制视图）。
pub fn looks_like_text(body: &[u8]) -> bool {
    let sample = &body[..body.len().min(512)];
    !sample.contains(&0) && std::str::from_utf8(sample).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_authority_handles_common_forms() {
        assert_eq!(
            split_authority("example.com", 443).unwrap(),
            ("example.com".into(), 443)
        );
        assert_eq!(
            split_authority("example.com:8443", 443).unwrap(),
            ("example.com".into(), 8443)
        );
        assert_eq!(
            split_authority("[::1]:8080", 443).unwrap(),
            ("::1".into(), 8080)
        );
        assert_eq!(split_authority("[::1]", 443).unwrap(), ("::1".into(), 443));
        assert!(split_authority("  ", 80).is_err());
    }

    #[test]
    fn split_authority_keeps_non_numeric_suffix_as_host() {
        assert_eq!(
            split_authority("host:abc", 80).unwrap(),
            ("host:abc".into(), 80)
        );
    }

    #[test]
    fn truncate_bytes_reports_flag() {
        assert_eq!(truncate_bytes(b"abc", 10), (b"abc".to_vec(), false));
        assert_eq!(truncate_bytes(b"abcdef", 3), (b"abc".to_vec(), true));
    }

    #[test]
    fn format_bytes_picks_unit() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn looks_like_text_detects_binary() {
        assert!(looks_like_text(b"hello"));
        assert!(!looks_like_text(&[0u8, 1, 2, 3]));
    }
}
