//! 其他平台的占位实现。

use std::path::Path;

use crate::{Result, SystemBridgeError};

pub fn install(_cert_path: &Path, _common_name: &str) -> Result<()> {
    Err(SystemBridgeError::Unsupported(format!(
        "{} 暂不支持自动安装根证书",
        crate::platform_name()
    )))
}

pub fn uninstall(_cert_path: &Path, _common_name: &str) -> Result<()> {
    Err(SystemBridgeError::Unsupported(format!(
        "{} 暂不支持自动移除根证书",
        crate::platform_name()
    )))
}
