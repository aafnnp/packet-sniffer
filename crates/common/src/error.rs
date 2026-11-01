//! 统一错误类型。

use std::io;

/// 跨层通用错误。
#[derive(Debug, thiserror::Error)]
pub enum CommonError {
    #[error("无效的 URL：{0}")]
    InvalidUrl(String),
    #[error("无效的 authority：{0}")]
    InvalidAuthority(String),
    #[error("无效的端口：{0}")]
    InvalidPort(String),
    #[error("IO 错误：{0}")]
    Io(#[from] io::Error),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, CommonError>;
