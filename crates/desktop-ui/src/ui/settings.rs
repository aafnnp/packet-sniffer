//! 设置页：代理、外观与根证书。

use nsh_core::EllipsizeMode;
use nsh_core::{
    element::{AccessibilityRole, FontWeight, input},
    geometry::px,
    prelude::*,
};

use desktop::state::Theme as AppearanceSetting;
use desktop::storage::default_history_path;

use crate::model::View as AppView;
use crate::theme::{colors, radius};
use crate::ui::icons::{IconName, icon};
use crate::ui::widgets::{Ctx, accent_button, icon_button, subtle_button, toggle};
use nsh_core::element::View as UiView;

pub fn settings_page(ctx: Ctx) -> impl IntoElement {
    let back = { move || ctx.act(|m| m.set_view(AppView::Traffic)) };
    let save = { move || ctx.act(|m| m.apply_draft()) };
    view()
        .flex_1()
        .w_full()
        .h_full()
        .flex_col()
        .bg(colors::WINDOW)
        .child(
            view()
                .h(px(44.0))
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .px(px(14.0))
                .bg(colors::CHROME)
                .border_b_1()
                .border_b_color(colors::BORDER)
                .child(icon_button(
                    IconName::ChevronLeft,
                    "返回",
                    colors::MUTED,
                    back,
                ))
                .child(
                    text("设置")
                        .text_size(px(16.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors::TEXT),
                )
                .child(view().flex_1())
                .child(accent_button("保存并应用", save)),
        )
        .child(
            scroll().flex_1().child(
                view().w_full().items_center().py(px(18.0)).child(
                    view()
                        .w(px(620.0))
                        .gap(px(14.0))
                        .child(proxy_section(ctx))
                        .child(appearance_section(ctx))
                        .child(cert_section(ctx))
                        .child(data_section(ctx))
                        .child(about_section(ctx))
                        .child(message(ctx)),
                ),
            ),
        )
}

fn card() -> UiView {
    view()
        .w_full()
        .p(px(14.0))
        .gap(px(10.0))
        .rounded(px(radius::PANEL))
        .bg(colors::SURFACE)
        .border_1()
        .border_color(colors::BORDER)
}

fn section_heading(title: &'static str, note: &'static str) -> impl IntoElement {
    view()
        .gap(px(2.0))
        .child(
            text(title)
                .text_size(px(13.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors::TEXT),
        )
        .child(text(note).text_size(px(11.0)).text_color(colors::MUTED))
}

fn proxy_section(ctx: Ctx) -> impl IntoElement {
    let ctx_port_value = ctx;
    let ctx_port_change = ctx;
    let ctx_max_value = ctx;
    let ctx_max_change = ctx;
    let ctx_capture = ctx;
    let ctx_system = ctx;
    card()
        .child(section_heading("代理", "更改监听端口后会自动重启引擎。"))
        .child(field_row(
            "监听端口",
            "本机代理端口，系统代理会指向它",
            input()
                .w(px(120.0))
                .h(px(28.0))
                .px(px(8.0))
                .rounded(px(radius::CONTROL))
                .bg(colors::RAISED)
                .border_1()
                .border_color(colors::BORDER)
                .text_size(px(12.0))
                .text_color(colors::TEXT)
                .value(move || ctx_port_value.read(|m| m.draft.port.clone()))
                .on_change(move |value: &str| {
                    let value = value.to_string();
                    ctx_port_change.act(|m| m.set_draft_port(value));
                }),
        ))
        .child(field_row(
            "正文上限（MB）",
            "单个请求或响应 body 的抓取上限",
            input()
                .w(px(120.0))
                .h(px(28.0))
                .px(px(8.0))
                .rounded(px(radius::CONTROL))
                .bg(colors::RAISED)
                .border_1()
                .border_color(colors::BORDER)
                .text_size(px(12.0))
                .text_color(colors::TEXT)
                .value(move || ctx_max_value.read(|m| m.draft.max_body_mb.clone()))
                .on_change(move |value: &str| {
                    let value = value.to_string();
                    ctx_max_change.act(|m| m.set_draft_max_body(value));
                }),
        ))
        .child(toggle_row(
            "解密 HTTPS",
            "关闭后只转发 CONNECT 隧道，不解析 TLS 内容",
            move || ctx_capture.read(|m| m.draft.capture),
            move || {
                ctx_capture.act(|m| {
                    let next = m.draft.capture;
                    m.set_draft_capture(!next);
                })
            },
        ))
        .child(toggle_row(
            "接管系统代理",
            "启动代理时同时把系统 HTTP/HTTPS 代理指向本机",
            move || ctx_system.read(|m| m.draft.system_proxy),
            move || {
                ctx_system.act(|m| {
                    let next = m.draft.system_proxy;
                    m.set_draft_system_proxy(!next);
                })
            },
        ))
}

fn appearance_section(ctx: Ctx) -> impl IntoElement {
    card()
        .child(section_heading("外观", "跟随系统，或固定为浅色 / 深色。"))
        .child(
            view()
                .flex_row()
                .gap(px(6.0))
                .child(appearance_button(
                    ctx,
                    AppearanceSetting::System,
                    "跟随系统",
                ))
                .child(appearance_button(ctx, AppearanceSetting::Light, "浅色"))
                .child(appearance_button(ctx, AppearanceSetting::Dark, "深色")),
        )
}

fn appearance_button(ctx: Ctx, value: AppearanceSetting, label: &'static str) -> impl IntoElement {
    let ctx_active_bg = ctx;
    let ctx_active_border = ctx;
    let ctx_press = ctx;
    view()
        .h(px(28.0))
        .px(px(12.0))
        .items_center()
        .justify_center()
        .rounded(px(radius::CONTROL))
        .bg(move || {
            if ctx_active_bg.read(|m| m.draft.appearance == value) {
                colors::ACCENT_SOFT
            } else {
                colors::RAISED
            }
        })
        .border_1()
        .border_color(move || {
            if ctx_active_border.read(|m| m.draft.appearance == value) {
                colors::ACCENT
            } else {
                colors::BORDER
            }
        })
        .cursor_pointer()
        .role(AccessibilityRole::Button)
        .label(label)
        .on_press(move || ctx_press.act(|m| m.set_draft_appearance(value)))
        .child(text(label).text_size(px(11.5)).text_color(colors::TEXT))
}

fn cert_section(ctx: Ctx) -> impl IntoElement {
    let install = { move || ctx.act(|m| m.install_ca()) };
    let reinstall = { move || ctx.act(|m| m.install_ca()) };
    let export = { move || ctx.act(|m| m.export_ca()) };
    card()
        .child(section_heading(
            "根证书",
            "解密 HTTPS 前，需要让系统信任本机生成的根证书。",
        ))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .child(icon(IconName::Shield, 15.0, colors::GREEN))
                .child(
                    text(move || {
                        ctx.read(|m| {
                            m.status
                                .ca_path
                                .clone()
                                .unwrap_or_else(|| m.settings.engine_config().ca_cert_path())
                                .display()
                                .to_string()
                        })
                    })
                    .flex_1()
                    .number_of_lines(1)
                    .ellipsize_mode(EllipsizeMode::Middle)
                    .font_family(crate::theme::MONO)
                    .text_size(px(10.5))
                    .text_color(colors::MUTED),
                ),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    when(
                        move || ctx.read(|m| m.status.ca_installed),
                        move || {
                            view()
                                .flex_row()
                                .items_center()
                                .gap(px(5.0))
                                .child(icon(IconName::Check, 14.0, colors::GREEN))
                                .child(
                                    text("已安装到系统信任存储")
                                        .text_size(px(11.5))
                                        .text_color(colors::GREEN),
                                )
                                .into_element()
                        },
                    )
                    .otherwise(move || accent_button("安装到系统", install).into_element()),
                )
                .child(view().flex_1())
                .child(
                    when(
                        move || ctx.read(|m| m.status.ca_installed),
                        move || subtle_button("重新安装", reinstall).into_element(),
                    )
                    .otherwise(view),
                )
                .child(subtle_button("导出到下载目录", export)),
        )
        .child(
            when(
                move || ctx.read(|m| m.status.ca_installed),
                || {
                    text("系统已信任该证书，抓取 HTTPS 流量时不会再提示安装。")
                        .text_size(px(10.5))
                        .text_color(colors::FAINT)
                        .into_element()
                },
            )
            .otherwise(|| {
                text("macOS 会提示输入管理员密码；Windows 安装到「受信任的根证书颁发机构」。")
                    .text_size(px(10.5))
                    .text_color(colors::FAINT)
                    .into_element()
            }),
        )
}

fn data_section(ctx: Ctx) -> impl IntoElement {
    let clear = move || ctx.act(|m| m.clear_history());
    card()
        .child(section_heading(
            "数据",
            "请求历史与高亮 / 备注保存在本地 SQLite 数据库中。",
        ))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .child(icon(IconName::Sidebar, 15.0, colors::MUTED))
                .child(
                    text(move || default_history_path().display().to_string())
                        .flex_1()
                        .number_of_lines(1)
                        .ellipsize_mode(EllipsizeMode::Middle)
                        .font_family(crate::theme::MONO)
                        .text_size(px(10.5))
                        .text_color(colors::MUTED),
                ),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .child(
                    text(move || format!("已保存 {} 条历史记录", ctx.read(|m| m.history_count)))
                        .flex_1()
                        .text_size(px(11.5))
                        .text_color(colors::TEXT),
                )
                .child(subtle_button("清空历史数据库", clear)),
        )
}

fn about_section(ctx: Ctx) -> impl IntoElement {
    card()
        .child(section_heading(
            "关于",
            "Packet Sniffer · 本地 HTTPS 抓包与调试工具",
        ))
        .child(
            view()
                .flex_row()
                .gap(px(18.0))
                .child(stat("已抓取", move || {
                    ctx.read(|m| m.status.captured.to_string())
                }))
                .child(stat("域名", move || {
                    ctx.read(|m| m.hosts.len().to_string())
                }))
                .child(stat("客户端", move || {
                    ctx.read(|m| m.clients.len().to_string())
                })),
        )
        .child(
            text("引擎：proxy-engine · 界面：NSH（Rust 原生组件）· 证书：rcgen / rustls")
                .text_size(px(10.5))
                .text_color(colors::FAINT),
        )
}

fn stat(label: &'static str, value: impl Fn() -> String + 'static) -> impl IntoElement {
    view()
        .gap(px(2.0))
        .child(
            text(value)
                .text_size(px(18.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors::TEXT),
        )
        .child(text(label).text_size(px(10.5)).text_color(colors::FAINT))
}

fn field_row(label: &'static str, note: &'static str, input: impl IntoElement) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .child(
            view()
                .flex_1()
                .gap(px(1.0))
                .child(text(label).text_size(px(12.0)).text_color(colors::TEXT))
                .child(text(note).text_size(px(10.5)).text_color(colors::FAINT)),
        )
        .child(input)
}

fn toggle_row(
    label: &'static str,
    note: &'static str,
    value: impl Fn() -> bool + 'static,
    on_toggle: impl Fn() + 'static,
) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .child(
            view()
                .flex_1()
                .gap(px(1.0))
                .child(text(label).text_size(px(12.0)).text_color(colors::TEXT))
                .child(text(note).text_size(px(10.5)).text_color(colors::FAINT)),
        )
        .child(toggle(value, on_toggle))
}

fn message(ctx: Ctx) -> impl IntoElement {
    view().child(
        when(
            move || ctx.read(|m| m.settings_message.is_some()),
            move || {
                ctx.read(|m| {
                    let (ok, message) = m.settings_message.clone().unwrap_or_default();
                    view()
                        .w_full()
                        .p(px(8.0))
                        .rounded(px(radius::CONTROL))
                        .bg(if ok {
                            colors::GREEN.opacity(0.12)
                        } else {
                            colors::RED.opacity(0.12)
                        })
                        .child(text(message).text_size(px(11.0)).text_color(if ok {
                            colors::GREEN
                        } else {
                            colors::RED
                        }))
                        .into_element()
                })
            },
        )
        .otherwise(view),
    )
}
