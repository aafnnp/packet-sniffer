//! 公共类型与工具，被引擎、系统适配层与 UI 层共同依赖。
//!
//! 该包不依赖任何网络、TLS 或 UI 库，保证可以被任意层安全引用。

pub mod error;
pub mod types;
pub mod utils;

pub use error::{CommonError, Result};
pub use types::{
    CapturedExchange, CapturedRequest, CapturedResponse, EngineEvent, Header, LogLevel, RequestId,
    RequestSummary, Scheme,
};
