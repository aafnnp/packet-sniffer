//! 其他平台的占位实现。

use super::ProxySettings;
use crate::{Result, SystemBridgeError};

pub fn apply(_settings: &ProxySettings) -> Result<()> {
    Err(SystemBridgeError::Unsupported(format!(
        "{} 暂不支持自动设置系统代理",
        crate::platform_name()
    )))
}

pub fn clear() -> Result<()> {
    Err(SystemBridgeError::Unsupported(format!(
        "{} 暂不支持自动设置系统代理",
        crate::platform_name()
    )))
}

pub fn current() -> Result<Option<ProxySettings>> {
    Ok(None)
}
