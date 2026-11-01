//! Windows 系统代理，通过注册表（`reg`）实现。

use tracing::info;

use super::ProxySettings;
use crate::{run, Result};

const INTERNET_SETTINGS: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings";

pub fn apply(settings: &ProxySettings) -> Result<()> {
    run(
        "reg",
        &[
            "add",
            INTERNET_SETTINGS,
            "/v",
            "ProxyServer",
            "/t",
            "REG_SZ",
            "/d",
            &settings.authority(),
            "/f",
        ],
    )?;
    run(
        "reg",
        &[
            "add",
            INTERNET_SETTINGS,
            "/v",
            "ProxyEnable",
            "/t",
            "REG_DWORD",
            "/d",
            "1",
            "/f",
        ],
    )?;
    info!(%settings.host, settings.port, "已开启系统代理");
    Ok(())
}

pub fn clear() -> Result<()> {
    run(
        "reg",
        &[
            "add",
            INTERNET_SETTINGS,
            "/v",
            "ProxyEnable",
            "/t",
            "REG_DWORD",
            "/d",
            "0",
            "/f",
        ],
    )?;
    info!("已关闭系统代理");
    Ok(())
}

pub fn current() -> Result<Option<ProxySettings>> {
    let output = run("reg", &["query", INTERNET_SETTINGS, "/v", "ProxyServer"])?;
    Ok(parse_proxy_server(&output))
}

fn parse_proxy_server(output: &str) -> Option<ProxySettings> {
    let value = output
        .lines()
        .find_map(|line| line.split_once("REG_SZ").map(|(_, value)| value.trim()))?;
    let (host, port) = value.rsplit_once(':')?;
    Some(ProxySettings::new(host, port.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_registry_output() {
        let output = "\nHKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings\n    ProxyServer    REG_SZ    127.0.0.1:8899\n\n";
        assert_eq!(
            parse_proxy_server(output),
            Some(ProxySettings::new("127.0.0.1", 8899))
        );
    }

    #[test]
    fn ignores_unrelated_output() {
        assert_eq!(parse_proxy_server("no proxy here"), None);
    }
}
