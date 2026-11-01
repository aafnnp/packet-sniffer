//! 启动 NSH 原生窗口。
//!
//! 与 headless 的 `packet-sniffer` 不同，这里直接挂载 [`desktop_ui::App`]，
//! 由 NSH 在 macOS 的 AppKit 上渲染，不经过 JS 运行时。

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
use nsh_platform_linux as platform;
#[cfg(target_os = "macos")]
use nsh_platform_macos as platform;
#[cfg(target_os = "windows")]
use nsh_platform_windows as platform;

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn main() -> impl std::process::Termination {
    use desktop::state::{Settings, Theme, default_settings_path};

    env_logger::init();
    platform::set_ui_sets(vec![nsh_base::registry()]);
    let settings = Settings::load(&default_settings_path());
    let appearance = match settings.theme {
        Theme::System => platform::Appearance::System,
        Theme::Light => platform::Appearance::Light,
        Theme::Dark => platform::Appearance::Dark,
    };
    platform::run_app(
        platform::AppSource::Rust(
            platform::Root::of::<desktop_ui::App>()
                .app_id("dev.packet-sniffer.app")
                .appearance(appearance),
        ),
        platform::AppOptions {
            title: "Packet Sniffer".to_owned(),
            app_id: "dev.packet-sniffer.app".to_owned(),
            size: (1360, 860),
            appearance,
            ..Default::default()
        },
    )
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
compile_error!("原生界面目前支持 macOS、Windows 与 Linux。");
