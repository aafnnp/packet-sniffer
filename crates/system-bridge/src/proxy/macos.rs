//! macOS 系统代理，通过 `networksetup` 命令实现。

use tracing::info;

use super::ProxySettings;
use crate::{run, Result};

/// 列出所有启用的网络服务（跳过 `*` 开头的停用服务与提示行）。
fn list_services() -> Result<Vec<String>> {
    let output = run("networksetup", &["-listallnetworkservices"])?;
    Ok(parse_services(&output))
}

fn parse_services(output: &str) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with('*'))
        .filter(|line| !line.starts_with("An asterisk"))
        .map(str::to_string)
        .collect()
}

/// 为单个网络服务生成 `networksetup` 参数列表（纯函数，便于测试）。
fn enable_commands(service: &str, settings: &ProxySettings) -> Vec<Vec<String>> {
    let port = settings.port.to_string();
    vec![
        vec![
            "-setwebproxy".into(),
            service.into(),
            settings.host.clone(),
            port.clone(),
        ],
        vec![
            "-setsecurewebproxy".into(),
            service.into(),
            settings.host.clone(),
            port.clone(),
        ],
        vec!["-setwebproxystate".into(), service.into(), "on".into()],
        vec![
            "-setsecurewebproxystate".into(),
            service.into(),
            "on".into(),
        ],
    ]
}

fn disable_commands(service: &str) -> Vec<Vec<String>> {
    vec![
        vec!["-setwebproxystate".into(), service.into(), "off".into()],
        vec![
            "-setsecurewebproxystate".into(),
            service.into(),
            "off".into(),
        ],
    ]
}

fn apply_commands(commands: Vec<Vec<String>>) -> Result<()> {
    for command in commands {
        let args: Vec<&str> = command.iter().map(String::as_str).collect();
        run("networksetup", &args)?;
    }
    Ok(())
}

pub fn apply(settings: &ProxySettings) -> Result<()> {
    let services = list_services()?;
    if services.is_empty() {
        return Err(crate::SystemBridgeError::Command {
            program: "networksetup".into(),
            message: "未找到可用的网络服务".into(),
        });
    }

    for service in &services {
        apply_commands(enable_commands(service, settings))?;
    }
    info!(services = services.len(), %settings.host, settings.port, "已开启系统代理");
    Ok(())
}

pub fn clear() -> Result<()> {
    for service in list_services()? {
        apply_commands(disable_commands(&service))?;
    }
    info!("已关闭系统代理");
    Ok(())
}

pub fn current() -> Result<Option<ProxySettings>> {
    let output = run("networksetup", &["-getwebproxy", "Wi-Fi"]).unwrap_or_default();
    Ok(parse_current(&output))
}

fn parse_current(output: &str) -> Option<ProxySettings> {
    let mut enabled = false;
    let mut host = None;
    let mut port = None;
    for line in output.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        match key.trim() {
            "Enabled" => enabled = value.trim().eq_ignore_ascii_case("yes"),
            "Server" => host = Some(value.trim().to_string()),
            "Port" => port = value.trim().parse::<u16>().ok(),
            _ => {}
        }
    }
    match (enabled, host, port) {
        (true, Some(host), Some(port)) if !host.is_empty() => Some(ProxySettings::new(host, port)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_service_list() {
        let output = "An asterisk (*) denotes that a network service is disabled.\nWi-Fi\n*Thunderbolt Bridge\n\n";
        assert_eq!(parse_services(output), vec!["Wi-Fi".to_string()]);
    }

    #[test]
    fn builds_enable_and_disable_commands() {
        let settings = ProxySettings::new("127.0.0.1", 8899);
        let enable = enable_commands("Wi-Fi", &settings);
        assert_eq!(
            enable[0],
            vec!["-setwebproxy", "Wi-Fi", "127.0.0.1", "8899"]
        );
        assert_eq!(enable[2], vec!["-setwebproxystate", "Wi-Fi", "on"]);

        let disable = disable_commands("Wi-Fi");
        assert_eq!(disable[0], vec!["-setwebproxystate", "Wi-Fi", "off"]);
    }

    #[test]
    fn parses_current_proxy_state() {
        let on = "Enabled: Yes\nServer: 127.0.0.1\nPort: 8899\nAuthenticated Proxy Enabled: 0\n";
        assert_eq!(
            parse_current(on),
            Some(ProxySettings::new("127.0.0.1", 8899))
        );

        let off = "Enabled: No\nServer: \nPort: 0\n";
        assert_eq!(parse_current(off), None);
    }
}
