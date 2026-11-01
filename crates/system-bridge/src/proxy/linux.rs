//! Linux 系统代理，通过 GNOME `gsettings` 实现。

use tracing::info;

use super::ProxySettings;
use crate::{run, Result};

fn apply_gsettings(host: &str, port: u16) -> Result<()> {
    let port = port.to_string();
    for (schema, key, value) in [
        ("org.gnome.system.proxy", "mode", "manual".to_string()),
        ("org.gnome.system.proxy.http", "host", host.to_string()),
        ("org.gnome.system.proxy.http", "port", port.clone()),
        ("org.gnome.system.proxy.https", "host", host.to_string()),
        ("org.gnome.system.proxy.https", "port", port),
    ] {
        run("gsettings", &["set", schema, key, &value])?;
    }
    Ok(())
}

pub fn apply(settings: &ProxySettings) -> Result<()> {
    apply_gsettings(&settings.host, settings.port)?;
    info!(%settings.host, settings.port, "已开启系统代理（GNOME）");
    Ok(())
}

pub fn clear() -> Result<()> {
    run(
        "gsettings",
        &["set", "org.gnome.system.proxy", "mode", "none"],
    )?;
    info!("已关闭系统代理（GNOME）");
    Ok(())
}

pub fn current() -> Result<Option<ProxySettings>> {
    let mode = run("gsettings", &["get", "org.gnome.system.proxy", "mode"])?;
    if !mode.contains("manual") {
        return Ok(None);
    }
    let host = run("gsettings", &["get", "org.gnome.system.proxy.http", "host"])?;
    let port = run("gsettings", &["get", "org.gnome.system.proxy.http", "port"])?;
    Ok(parse_gsettings(&host, &port))
}

fn parse_gsettings(host: &str, port: &str) -> Option<ProxySettings> {
    let host = host.trim().trim_matches('\'').to_string();
    let port = port.trim().parse::<u16>().ok()?;
    if host.is_empty() {
        return None;
    }
    Some(ProxySettings::new(host, port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gsettings_values() {
        assert_eq!(
            parse_gsettings("'127.0.0.1'\n", "8899\n"),
            Some(ProxySettings::new("127.0.0.1", 8899))
        );
        assert_eq!(parse_gsettings("''\n", "0\n"), None);
        assert_eq!(parse_gsettings("'127.0.0.1'\n", "not-a-port"), None);
    }
}
