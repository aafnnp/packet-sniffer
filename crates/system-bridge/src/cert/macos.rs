//! macOS 证书管理，通过 `security` 命令实现。
//!
//! 注意：单纯执行 `security add-trusted-cert` 在证书尚未导入钥匙串时会「成功」但什么也不做，
//! 因此这里显式分两步：先 `import` 导入当前用户钥匙串，再写入 SSL 策略的信任设置，
//! 最后用 `dump-trust-settings` 回读校验。

use std::path::Path;

use tracing::{info, warn};

use crate::{run, Result, SystemBridgeError};

fn login_keychain() -> String {
    match std::env::var_os("HOME") {
        Some(home) => format!(
            "{}/Library/Keychains/login.keychain-db",
            Path::new(&home).display()
        ),
        None => "login.keychain-db".to_string(),
    }
}

fn is_imported(common_name: &str, keychain: &str) -> bool {
    run(
        "security",
        &["find-certificate", "-c", common_name, keychain],
    )
    .is_ok()
}

fn is_trusted(common_name: &str) -> bool {
    run("security", &["dump-trust-settings"])
        .map(|output| output.contains(common_name))
        .unwrap_or(false)
}

pub fn install(cert_path: &Path, common_name: &str) -> Result<()> {
    let path = cert_path.display().to_string();
    let keychain = login_keychain();

    if is_imported(common_name, &keychain) {
        info!("根证书已存在于当前用户钥匙串");
    } else {
        run(
            "security",
            &["import", &path, "-k", &keychain, "-T", "/usr/bin/security"],
        )?;
        info!("根证书已导入当前用户钥匙串");
    }

    // 必须指定 -p ssl：只导入证书不会让浏览器接受动态签发的站点证书。
    run(
        "security",
        &[
            "add-trusted-cert",
            "-r",
            "trustRoot",
            "-p",
            "ssl",
            "-k",
            &keychain,
            &path,
        ],
    )?;

    if !is_trusted(common_name) {
        return Err(SystemBridgeError::Command {
            program: "security".into(),
            message: format!("未能写入 «{common_name}» 的信任设置，请改用手动信任"),
        });
    }

    info!("根证书已设为 SSL 受信任");
    Ok(())
}

pub fn uninstall(cert_path: &Path, common_name: &str) -> Result<()> {
    let path = cert_path.display().to_string();
    let keychain = login_keychain();

    match run("security", &["remove-trusted-cert", &path]) {
        Ok(_) => info!("已移除根证书的信任设置"),
        Err(err) => warn!(error = %err, "移除信任设置失败（可能本就没有设置）"),
    }

    if is_imported(common_name, &keychain) {
        run(
            "security",
            &["delete-certificate", "-c", common_name, &keychain],
        )?;
        info!("根证书已从当前用户钥匙串删除");
    }

    Ok(())
}
