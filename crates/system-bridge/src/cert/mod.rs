//! 系统证书管理：根证书的安装与移除。

use std::path::Path;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod unsupported;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
pub use macos::{install, uninstall};
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub use unsupported::{install, uninstall};
#[cfg(target_os = "windows")]
pub use windows::{install, uninstall};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{install, uninstall};

/// 手动安装根证书的提示文本（自动安装失败时展示给用户）。
pub fn install_hint(cert_path: &Path) -> String {
    let path = cert_path.display();
    if cfg!(target_os = "macos") {
        format!("打开「钥匙串访问」→ 将 {path} 拖入「系统」钥匙串 → 双击并设为「始终信任」")
    } else if cfg!(target_os = "windows") {
        format!("双击 {path} → 安装证书 → 选择「受信任的根证书颁发机构」")
    } else {
        format!(
            "将 {path} 复制到 /usr/local/share/ca-certificates/ 后执行 sudo update-ca-certificates"
        )
    }
}
