//! Windows 证书管理，通过 `certutil` 实现当前用户根证书存储。

use std::path::Path;

use tracing::info;

use crate::{run, Result};

pub fn install(cert_path: &Path) -> Result<()> {
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
