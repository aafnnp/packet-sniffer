//! 系统能力适配层。
//!
//! 上层只调用本包提供的统一接口，平台差异全部收敛在条件编译之后的实现里。
//! MVP 阶段通过系统自带命令行工具完成操作（macOS `networksetup` / `security`，
//! Windows `reg` / `certutil`，Linux `gsettings` / `certutil`），避免引入额外的本地依赖。

pub mod cert;
pub mod proxy;

use std::io;
use std::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum SystemBridgeError {
    #[error("当前平台不支持该操作：{0}")]
    Unsupported(String),
    #[error("命令 `{program}` 执行失败：{message}")]
    Command { program: String, message: String },
    #[error("IO 错误：{0}")]
    Io(#[from] io::Error),
}

pub type Result<T> = std::result::Result<T, SystemBridgeError>;

/// 执行系统命令并返回标准输出；非零退出码视为失败。
pub(crate) fn run(program: &str, args: &[&str]) -> Result<String> {
    tracing::debug!(program, ?args, "执行系统命令");

    let output =
        Command::new(program)
            .args(args)
            .output()
            .map_err(|err| SystemBridgeError::Command {
                program: program.to_string(),
                message: err.to_string(),
            })?;

    if !output.status.success() {
        return Err(SystemBridgeError::Command {
            program: program.to_string(),
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// 当前编译平台名称，用于 UI 提示。
pub const fn platform_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(target_os = "windows") {
        "Windows"
    } else if cfg!(target_os = "linux") {
        "Linux"
    } else {
        "Unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_reports_missing_program() {
        let err = run("definitely-not-a-real-command-xyz", &[]).unwrap_err();
        assert!(matches!(err, SystemBridgeError::Command { .. }));
    }

    #[test]
    fn platform_name_is_not_empty() {
        assert!(!platform_name().is_empty());
    }
}
