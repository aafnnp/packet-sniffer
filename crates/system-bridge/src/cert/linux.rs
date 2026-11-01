//! Linux 证书管理，通过 NSS 数据库（`certutil`）为浏览器安装根证书。

use std::path::Path;

use tracing::info;

use crate::{run, Result};

fn nss_db() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    format!("sql:{home}/.pki/nssdb")
}

/// 根证书是否已在当前用户的 NSS 数据库中。
pub fn is_installed(common_name: &str) -> bool {
    run("certutil", &["-d", &nss_db(), "-L", "-n", common_name]).is_ok()
}

pub fn install(cert_path: &Path, common_name: &str) -> Result<()> {
    let db = nss_db();
    let path = cert_path.display().to_string();
    run(
        "certutil",
        &["-d", &db, "-A", "-t", "C,,", "-n", common_name, "-i", &path],
    )?;
    info!("根证书已加入 NSS 数据库（浏览器需要重启生效）");
    Ok(())
}

pub fn uninstall(_cert_path: &Path, common_name: &str) -> Result<()> {
    let db = nss_db();
    run("certutil", &["-d", &db, "-D", "-n", common_name])?;
    info!("根证书已从 NSS 数据库移除");
    Ok(())
}
