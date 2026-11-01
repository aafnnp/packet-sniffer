//! 响应体解压：gzip / deflate / brotli。
//!
//! 存储层保留原始字节（保证与线上一致），展示时再按需解压。

use std::io::Read;

use common::types::Header;

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("不支持的 Content-Encoding：{0}")]
    Unsupported(String),
    #[error("解压失败：{0}")]
    Io(#[from] std::io::Error),
}

/// 读取 `Content-Encoding`（可能形如 `gzip, br`，取最后一个生效的编码）。
pub fn content_encoding(headers: &[Header]) -> Option<String> {
    headers
        .iter()
        .filter(|h| h.name.eq_ignore_ascii_case("content-encoding"))
        .flat_map(|h| h.value.split(','))
        .map(|part| part.trim().to_ascii_lowercase())
        .rfind(|part| !part.is_empty() && part != "identity")
}

/// 按首部自动选择解压方式。
pub fn decode_body(headers: &[Header], body: &[u8], limit: usize) -> Result<Vec<u8>, DecodeError> {
    match content_encoding(headers) {
        Some(encoding) => decode_with(&encoding, body, limit),
        None => Ok(body.to_vec()),
    }
}

/// 按指定编码解压，解压结果最多保留 `limit` 字节。
pub fn decode_with(encoding: &str, body: &[u8], limit: usize) -> Result<Vec<u8>, DecodeError> {
    let encoding = encoding.trim().to_ascii_lowercase();
    match encoding.as_str() {
        "" | "identity" => Ok(body.to_vec()),
        "gzip" | "x-gzip" => read_limited(flate2::read::GzDecoder::new(body), limit),
        "br" => read_limited(brotli::Decompressor::new(body, 8192), limit),
        "deflate" => {
            // 部分服务端发送 raw deflate，zlib 解不出来时再退回 raw 解码。
            match read_limited(flate2::read::ZlibDecoder::new(body), limit) {
                Ok(out) => Ok(out),
                Err(_) => read_limited(flate2::read::DeflateDecoder::new(body), limit),
            }
        }
        other => Err(DecodeError::Unsupported(other.to_string())),
    }
}

fn read_limited(mut reader: impl Read, limit: usize) -> Result<Vec<u8>, DecodeError> {
    let mut out = Vec::new();
    let mut limited = (&mut reader).take(limit as u64);
    limited.read_to_end(&mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn gzip(data: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn decodes_gzip() {
        let raw = b"packet sniffer";
        let compressed = gzip(raw);
        assert_eq!(decode_with("gzip", &compressed, 1024).unwrap(), raw);
    }

    #[test]
    fn decodes_brotli() {
        let raw = b"packet sniffer brotli";
        let mut compressed = Vec::new();
        {
            let mut writer = brotli::CompressorWriter::new(&mut compressed, 4096, 5, 22);
            writer.write_all(raw).unwrap();
        }
        assert_eq!(decode_with("br", &compressed, 1024).unwrap(), raw);
    }

    #[test]
    fn respects_limit() {
        let compressed = gzip(b"0123456789");
        assert_eq!(decode_with("gzip", &compressed, 4).unwrap().len(), 4);
    }

    #[test]
    fn identity_passthrough() {
        assert_eq!(decode_with("identity", b"abc", 8).unwrap(), b"abc");
        let headers = vec![Header::new("Content-Encoding", "identity")];
        assert!(content_encoding(&headers).is_none());
        assert_eq!(decode_body(&headers, b"abc", 8).unwrap(), b"abc");
    }

    #[test]
    fn unknown_encoding_errors() {
        assert!(matches!(
            decode_with("zstd", b"abc", 8),
            Err(DecodeError::Unsupported(_))
        ));
    }

    #[test]
    fn picks_last_non_identity_encoding() {
        let headers = vec![Header::new("Content-Encoding", "identity, gzip")];
        assert_eq!(content_encoding(&headers).as_deref(), Some("gzip"));
    }
}
