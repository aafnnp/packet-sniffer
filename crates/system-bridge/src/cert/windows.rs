//! Windows 证书管理，通过 `certutil` 实现当前用户根证书存储。

use std::path::Path;

use tracing::info;

use crate::{run, Result};

/// 根证书是否已在当前用户的「受信任的根证书颁发机构」中。
pub fn is_installed(common_name: &str) -> bool {
    let name = common_name.to_ascii_lowercase();
    run("certutil", &["-store", "-user", "Root", common_name])
        .map(|output| output.to_ascii_lowercase().contains(&name))
        .unwrap_or(false)
}

pub fn install(cert_path: &Path, _common_name: &str) -> Result<()> {
    let path = cert_path.display().to_string();
    run("certutil", &["-addstore", "-user", "Root", &path])?;
    info!("根证书已加入当前用户「受信任的根证书颁发机构」");
    Ok(())
}

pub fn uninstall(_cert_path: &Path, common_name: &str) -> Result<()> {
    run("certutil", &["-delstore", "-user", "Root", common_name])?;
    info!("根证书已移除");
    Ok(())
}
