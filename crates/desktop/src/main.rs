//! 命令行入口。
//!
//! 以 headless 模式运行：启动引擎、把抓到的请求打印到终端。
//! 图形界面在 `crates/desktop-ui`，两者共用 [`desktop::app::App`] 状态。

// headless 模式下 UI 状态层只会用到一部分 API，接入 NSH 界面后这些未使用告警会消失。
#![allow(dead_code)]

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use common::types::{CapturedExchange, EngineEvent};
use proxy_engine::ProxyEngine;
use system_bridge::{cert, proxy as system_proxy};

use desktop::app::App;
use desktop::bridge::EventBridge;
use desktop::state::{default_settings_path, Settings};

const USAGE: &str = "\
packet-sniffer —— 基于 NSH 的抓包工具

用法：
  packet-sniffer run [选项]              启动代理（headless 模式输出到终端）
  packet-sniffer export-ca [--out <文件>] 导出根证书
  packet-sniffer install-ca               安装根证书到系统信任存储
  packet-sniffer uninstall-ca             从系统信任存储移除根证书
  packet-sniffer system-proxy enable|disable
  packet-sniffer --help

通用选项：
  --host <地址>          监听地址（默认 127.0.0.1）
  --port <端口>          监听端口（默认 8899）
  --ca-dir <目录>        根证书目录
  --settings <文件>      设置文件路径
  --no-capture           只转发，不解密 HTTPS
  --max-body <字节>      单个 body 抓取上限
  --store-capacity <条>  内存存储条数上限
  --json                 run 模式下额外输出完整记录的 JSON 行
";

fn main() -> Result<()> {
    init_tracing();

    let raw: Vec<String> = std::env::args().skip(1).collect();
    match Command::parse(&raw)? {
        Command::Help => {
            println!("{USAGE}");
            Ok(())
        }
        Command::Run(options) => run(options),
        Command::ExportCa { options, out } => export_ca(&options, &out),
        Command::InstallCa { options } => install_ca(&options),
        Command::UninstallCa { options } => uninstall_ca(&options),
        Command::SystemProxy { options, enable } => set_system_proxy(&options, enable),
    }
}

#[derive(Debug, Default)]
struct RunOptions {
    host: Option<IpAddr>,
    port: Option<u16>,
    ca_dir: Option<PathBuf>,
    settings: Option<PathBuf>,
    capture: Option<bool>,
    max_body_bytes: Option<usize>,
    store_capacity: Option<usize>,
    json: bool,
}

#[derive(Debug)]
enum Command {
    Run(RunOptions),
    ExportCa { options: RunOptions, out: PathBuf },
    InstallCa { options: RunOptions },
    UninstallCa { options: RunOptions },
    SystemProxy { options: RunOptions, enable: bool },
    Help,
}

impl Command {
    fn parse(raw: &[String]) -> Result<Self> {
        let Some(first) = raw.first() else {
            return Ok(Command::Run(RunOptions::default()));
        };

        let (subcommand, rest): (&str, &[String]) = match first.as_str() {
            "-h" | "--help" | "help" => return Ok(Command::Help),
            "run" => ("run", &raw[1..]),
            "export-ca" => ("export-ca", &raw[1..]),
            "install-ca" => ("install-ca", &raw[1..]),
            "uninstall-ca" => ("uninstall-ca", &raw[1..]),
            "system-proxy" => ("system-proxy", &raw[1..]),
            other if other.starts_with('-') => ("run", raw),
            other => bail!("未知子命令：{other}\n\n{USAGE}"),
        };

        let mut options = RunOptions::default();
        let mut out: Option<PathBuf> = None;
        let mut enable = true;

        let mut index = 0;
        while index < rest.len() {
            match rest[index].as_str() {
                "-h" | "--help" => return Ok(Command::Help),
                "--host" => {
                    let value = take_value(rest, &mut index, "--host")?;
                    options.host = Some(value.parse().context("无效的监听地址")?);
                }
                "--port" => {
                    let value = take_value(rest, &mut index, "--port")?;
                    options.port = Some(value.parse().context("无效的监听端口")?);
                }
                "--ca-dir" => {
                    options.ca_dir = Some(take_value(rest, &mut index, "--ca-dir")?.into());
                }
                "--settings" => {
                    options.settings = Some(take_value(rest, &mut index, "--settings")?.into());
                }
                "--out" => {
                    out = Some(take_value(rest, &mut index, "--out")?.into());
                }
                "--max-body" => {
                    let value = take_value(rest, &mut index, "--max-body")?;
                    options.max_body_bytes = Some(value.parse().context("无效的抓取上限")?);
                }
                "--store-capacity" => {
                    let value = take_value(rest, &mut index, "--store-capacity")?;
                    options.store_capacity = Some(value.parse().context("无效的存储容量")?);
                }
                "--no-capture" => options.capture = Some(false),
                "--capture" => options.capture = Some(true),
                "--json" => options.json = true,
                "enable" => enable = true,
                "disable" => enable = false,
                other => bail!("未知参数：{other}\n\n{USAGE}"),
            }
            index += 1;
        }

        Ok(match subcommand {
            "run" => Command::Run(options),
            "export-ca" => Command::ExportCa {
                out: out.unwrap_or_else(|| PathBuf::from("root-ca.pem")),
                options,
            },
            "install-ca" => Command::InstallCa { options },
            "uninstall-ca" => Command::UninstallCa { options },
            "system-proxy" => Command::SystemProxy { options, enable },
            _ => unreachable!("子命令已在上方收敛"),
        })
    }
}

fn take_value(args: &[String], index: &mut usize, flag: &str) -> Result<String> {
    *index += 1;
    args.get(*index)
        .cloned()
        .ok_or_else(|| anyhow!("参数 {flag} 缺少取值"))
}

/// 合并命令行参数与持久化设置。
fn resolve_settings(options: &RunOptions) -> Settings {
    let path = options
        .settings
        .clone()
        .unwrap_or_else(default_settings_path);
    let mut settings = Settings::load(&path);

    if let Some(host) = options.host {
        settings.listen_addr.set_ip(host);
    }
    if let Some(port) = options.port {
        settings.listen_addr.set_port(port);
    }
    if let Some(ca_dir) = options.ca_dir.clone() {
        settings.ca_dir = ca_dir;
    }
    if let Some(capture) = options.capture {
        settings.capture_enabled = capture;
    }
    if let Some(max_body_bytes) = options.max_body_bytes {
        settings.max_body_bytes = max_body_bytes;
    }
    if let Some(store_capacity) = options.store_capacity {
        settings.store_capacity = store_capacity;
    }
    settings
}

fn run(options: RunOptions) -> Result<()> {
    let settings = resolve_settings(&options);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("创建 tokio 运行时失败")?;
    runtime.block_on(run_engine(settings, options.json))
}

async fn run_engine(settings: Settings, json: bool) -> Result<()> {
    let engine = Arc::new(ProxyEngine::new(settings.engine_config())?);
    let bridge = EventBridge::spawn(engine.subscribe());
    let shutdown = engine.shutdown_token();
    let mut app = App::new(settings);

    let mut engine_task = {
        let engine = engine.clone();
        tokio::spawn(async move { engine.run().await })
    };

    loop {
        tokio::select! {
            signal = tokio::signal::ctrl_c() => {
                signal.context("监听退出信号失败")?;
                println!("\n正在停止抓包引擎…");
                shutdown.cancel();
            }
            event = bridge.next() => {
                let Some(event) = event else { break };
                if let Some(line) = render_event(&mut app, &event) {
                    println!("{line}");
                }
                if json {
                    if let EngineEvent::RequestCompleted { exchange } = &event {
                        println!("{}", serde_json::to_string(exchange)?);
                    }
                }
            }
            result = &mut engine_task => {
                return match result {
                    Ok(Ok(())) => Ok(()),
                    Ok(Err(err)) => Err(err.into()),
                    Err(err) => Err(err.into()),
                };
            }
        }
    }

    Ok(())
}

fn render_event(app: &mut App, event: &EngineEvent) -> Option<String> {
    app.handle_event(event.clone());
    match event {
        EngineEvent::Started {
            listen_addr,
            ca_cert_path,
        } => Some(format!(
            "== 代理已启动：http://{listen_addr}\n   模式：{}\n   根证书：{ca_cert_path}\n   提示：{}",
            if app.settings.capture_enabled {
                "解密 HTTPS"
            } else {
                "仅转发，不解密 HTTPS"
            },
            cert::install_hint(Path::new(ca_cert_path))
        )),
        EngineEvent::Stopped => Some("== 代理已停止".to_string()),
        EngineEvent::RequestStarted { request } => {
            Some(format!("-> {} {}", request.method, request.url))
        }
        EngineEvent::RequestCompleted { exchange } => Some(format_exchange(exchange)),
        EngineEvent::Log { level, message } => Some(format!("[{level:?}] {message}")),
    }
}

fn format_exchange(exchange: &CapturedExchange) -> String {
    let status = exchange
        .response
        .as_ref()
        .map(|response| response.status.to_string())
        .unwrap_or_else(|| "-".to_string());
    let bytes = exchange
        .response
        .as_ref()
        .map(|response| response.body.len())
        .unwrap_or(0);
    let error = exchange
        .error
        .as_deref()
        .map(|error| format!("  错误：{error}"))
        .unwrap_or_default();

    format!(
        "<- {status} {} {}ms {bytes}B{error}",
        exchange.request.method, exchange.duration_ms
    )
}

fn export_ca(options: &RunOptions, out: &Path) -> Result<()> {
    let settings = resolve_settings(options);
    let engine = ProxyEngine::new(settings.engine_config())?;
    let source = engine.ca_cert_path();

    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::copy(&source, out).with_context(|| format!("导出根证书到 {} 失败", out.display()))?;

    println!("根证书已导出：{}", out.display());
    println!("{}", cert::install_hint(out));
    Ok(())
}

fn install_ca(options: &RunOptions) -> Result<()> {
    let settings = resolve_settings(options);
    let engine = ProxyEngine::new(settings.engine_config())?;
    let cert_path = engine.ca_cert_path();

    cert::install(&cert_path, &engine.config().ca_common_name).with_context(|| {
        format!(
            "自动安装失败，请手动安装：{}",
            cert::install_hint(&cert_path)
        )
    })?;

    println!("根证书已安装：{}", cert_path.display());
    Ok(())
}

fn uninstall_ca(options: &RunOptions) -> Result<()> {
    let settings = resolve_settings(options);
    let engine = ProxyEngine::new(settings.engine_config())?;
    let cert_path = engine.ca_cert_path();

    cert::uninstall(&cert_path, &engine.config().ca_common_name)?;
    println!("根证书已移除：{}", cert_path.display());
    Ok(())
}

fn set_system_proxy(options: &RunOptions, enable: bool) -> Result<()> {
    let settings = resolve_settings(options);
    if enable {
        let proxy_settings = system_proxy::ProxySettings::new(
            settings.listen_addr.ip().to_string(),
            settings.listen_addr.port(),
        );
        system_proxy::apply(&proxy_settings)?;
        println!("系统代理已指向 {}", proxy_settings.authority());
    } else {
        system_proxy::clear()?;
        println!("系统代理已关闭");
    }
    Ok(())
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_env("PACKET_SNIFFER_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info,hyper=warn,rustls=warn"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn parses_run_with_options() {
        let command = Command::parse(&args(&[
            "run",
            "--port",
            "9100",
            "--host",
            "0.0.0.0",
            "--no-capture",
            "--json",
        ]))
        .unwrap();

        match command {
            Command::Run(options) => {
                assert_eq!(options.port, Some(9100));
                assert_eq!(options.host, Some("0.0.0.0".parse().unwrap()));
                assert_eq!(options.capture, Some(false));
                assert!(options.json);
            }
            other => panic!("应解析为 Run，实际为 {other:?}"),
        }
    }

    #[test]
    fn defaults_to_run_without_arguments() {
        assert!(matches!(Command::parse(&[]).unwrap(), Command::Run(_)));
        assert!(matches!(
            Command::parse(&args(&["--port", "9"])).unwrap(),
            Command::Run(_)
        ));
    }

    #[test]
    fn parses_subcommands() {
        assert!(matches!(
            Command::parse(&args(&["export-ca", "--out", "ca.pem"])).unwrap(),
            Command::ExportCa { .. }
        ));
        assert!(matches!(
            Command::parse(&args(&["install-ca"])).unwrap(),
            Command::InstallCa { .. }
        ));
        assert!(matches!(
            Command::parse(&args(&["system-proxy", "disable"])).unwrap(),
            Command::SystemProxy { enable: false, .. }
        ));
        assert!(matches!(
            Command::parse(&args(&["--help"])).unwrap(),
            Command::Help
        ));
    }

    #[test]
    fn rejects_unknown_input() {
        assert!(Command::parse(&args(&["nope"])).is_err());
        assert!(Command::parse(&args(&["run", "--port"])).is_err());
        assert!(Command::parse(&args(&["run", "--port", "abc"])).is_err());
    }

    #[test]
    fn settings_merge_prefers_command_line() {
        let options = RunOptions {
            port: Some(8123),
            capture: Some(false),
            store_capacity: Some(7),
            ..Default::default()
        };
        let settings = resolve_settings(&options);
        assert_eq!(settings.listen_addr.port(), 8123);
        assert!(!settings.capture_enabled);
        assert_eq!(settings.store_capacity, 7);
    }

    #[test]
    fn formats_exchange_line() {
        use chrono::Utc;
        use common::types::{CapturedExchange, CapturedRequest, Scheme};

        let exchange = CapturedExchange {
            request: CapturedRequest {
                id: uuid::Uuid::new_v4(),
                scheme: Scheme::Http,
                method: "GET".into(),
                url: "http://example.com/".into(),
                host: "example.com".into(),
                port: 80,
                path: "/".into(),
                version: "HTTP/1.1".into(),
                headers: Vec::new(),
                body: Vec::new(),
                body_truncated: false,
                body_complete: true,
                started_at: Utc::now(),
            },
            response: None,
            error: None,
            duration_ms: 8,
            completed_at: None,
        };
        assert_eq!(format_exchange(&exchange), "<- - GET 8ms 0B");
    }

    #[test]
    fn status_enum_labels_are_stable() {
        assert_eq!(desktop::app::EngineStatus::Running.label(), "运行中");
    }
}
