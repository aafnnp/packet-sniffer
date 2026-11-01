//! 窗口外壳：顶部工具栏、左侧导航与底部状态栏。

use std::rc::Rc;

use nsh_core::EllipsizeMode;
use nsh_core::{
    element::{AccessibilityRole, AdaptiveColor, input},
    geometry::px,
    prelude::*,
};

use crate::model::{Facet, KindFilter, Section, View};
use crate::theme::{colors, radius, size};
use crate::ui::icons::{IconName, icon, icon_reactive};
use crate::ui::widgets::{Ctx, icon_button, section_title};

pub fn toolbar(ctx: Ctx) -> impl IntoElement {
    let ctx_icon = ctx;
    let ctx_bg = ctx;
    let ctx_press = ctx;
    let ctx_filter = ctx;
    let ctx_trash = ctx;
    let ctx_sort = ctx;
    let ctx_export = ctx;
    let ctx_gear = ctx;
    view()
        .h(px(size::TOOLBAR_HEIGHT))
        .w_full()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .px(px(12.0))
        .bg(colors::CHROME)
        .border_b_1()
        .border_b_color(colors::BORDER)
        .child(
            view()
                .size(px(30.0))
                .items_center()
                .justify_center()
                .rounded(px(15.0))
                .bg(move || {
                    if ctx_bg.read(|m| m.status.running) {
                        colors::ACCENT
                    } else {
                        colors::RAISED
                    }
                })
                .border_1()
                .border_color(colors::BORDER)
                .cursor_pointer()
                .role(AccessibilityRole::Button)
                .label("启动或停止代理")
                .hover(|s| s.opacity(0.85))
                .active(|s| s.opacity(0.6))
                .on_press(move || ctx_press.act(|m| m.toggle_engine()))
                .child(
                    when(
                        move || ctx_icon.read(|m| m.status.running),
                        || icon(IconName::Pause, 13.0, colors::ON_ACCENT),
                    )
                    .otherwise(|| {
                        view()
                            .child(icon(IconName::Play, 13.0, colors::TEXT))
                            .into_element()
                    }),
                )
                .into_element(),
        )
        .child(icon_button(
            IconName::Filter,
            "筛选",
            colors::MUTED,
            move || {
                ctx_filter.filter_ref.focus();
            },
        ))
        .child(icon_button(
            IconName::Trash,
            "清空",
            colors::MUTED,
            move || ctx_trash.act(|m| m.clear()),
        ))
        .child(view().flex_1())
        .child(status_pill(ctx))
        .child(view().flex_1())
        .child(icon_button(
            IconName::Sort,
            "排序",
            colors::MUTED,
            move || ctx_sort.act(|m| m.toggle_sort()),
        ))
        .child(icon_button(
            IconName::Export,
            "导出请求 JSON",
            colors::MUTED,
            move || ctx_export.act(|m| m.export_requests()),
        ))
        .child(icon_button(
            IconName::Gear,
            "设置",
            colors::MUTED,
            move || ctx_gear.act(|m| m.set_view(View::Settings)),
        ))
}

fn status_pill(ctx: Ctx) -> impl IntoElement {
    let ctx_dot = ctx;
    let ctx_text = ctx;
    let ctx_press = ctx;
    view()
        .h(px(28.0))
        .px(px(12.0))
        .flex_row()
        .items_center()
        .gap(px(7.0))
        .rounded(px(14.0))
        .bg(colors::RAISED)
        .border_1()
        .border_color(colors::BORDER)
        .cursor_pointer()
        .role(AccessibilityRole::Button)
        .label("代理状态")
        .hover(|s| s.bg(colors::ALT))
        .on_press(move || ctx_press.act(|m| m.toggle_engine()))
        .child(view().size(px(7.0)).rounded(px(3.5)).bg(move || {
            ctx_dot.read(|m| {
                if m.status.failed {
                    colors::RED
                } else if m.status.running {
                    colors::GREEN
                } else {
                    colors::FAINT
                }
            })
        }))
        .child(
            text(move || {
                ctx_text.read(|m| {
                    let label = if m.status.running {
                        "Packet Sniffer"
                    } else if m.status.failed {
                        "Packet Sniffer · 异常"
                    } else {
                        "Packet Sniffer · 已停止"
                    };
                    match m.status.listen.as_deref() {
                        Some(addr) if m.status.running => {
                            format!("{label} | 监听 {addr}")
                        }
                        _ => label.to_string(),
                    }
                })
            })
            .text_size(px(12.0))
            .text_color(colors::TEXT),
        )
}

pub fn notice_banner(ctx: Ctx) -> impl IntoElement {
    view().child(
        when(
            move || ctx.read(|m| m.notice.is_some()),
            move || {
                view()
                    .h(px(30.0))
                    .w_full()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(12.0))
                    .bg(colors::GREEN.opacity(0.12))
                    .border_b_1()
                    .border_b_color(colors::GREEN.opacity(0.35))
                    .child(icon(IconName::Check, 14.0, colors::GREEN))
                    .child(
                        text(move || ctx.read(|m| m.notice.clone().unwrap_or_default()))
                            .flex_1()
                            .number_of_lines(1)
                            .ellipsize_mode(EllipsizeMode::Middle)
                            .text_size(px(11.5))
                            .text_color(colors::GREEN),
                    )
                    .child(icon_button(
                        IconName::Close,
                        "关闭提示",
                        colors::GREEN,
                        move || ctx.act(|m| m.clear_notice()),
                    ))
                    .into_element()
            },
        )
        .otherwise(view),
    )
}

pub fn error_banner(ctx: Ctx) -> impl IntoElement {
    view().child(
        when(
            move || ctx.read(|m| m.status.error.is_some()),
            move || {
                view()
                    .h(px(30.0))
                    .w_full()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(12.0))
                    .bg(colors::RED.opacity(0.12))
                    .border_b_1()
                    .border_b_color(colors::RED.opacity(0.35))
                    .child(icon(IconName::Error, 14.0, colors::RED))
                    .child(
                        text(move || ctx.read(|m| m.status.error.clone().unwrap_or_default()))
                            .flex_1()
                            .number_of_lines(1)
                            .ellipsize_mode(EllipsizeMode::Tail)
                            .text_size(px(11.5))
                            .text_color(colors::RED),
                    )
                    .child(icon_button(
                        IconName::Close,
                        "关闭错误提示",
                        colors::RED,
                        move || ctx.act(|m| m.clear_error()),
                    ))
                    .into_element()
            },
        )
        .otherwise(view),
    )
}

pub fn sidebar(ctx: Ctx) -> impl IntoElement {
    view()
        .w(px(size::SIDEBAR_WIDTH))
        .flex_shrink_0()
        .h_full()
        .flex_col()
        .bg(colors::SIDEBAR)
        .border_r_1()
        .border_r_color(colors::BORDER)
        .child(
            view().flex_1().overflow_hidden().child(
                scroll().h_full().child(
                    view()
                        .w_full()
                        .py(px(8.0))
                        .child(favorites_section(ctx))
                        .child(all_section(ctx))
                        .child(tools_section(ctx))
                        .child(apps_section(ctx))
                        .child(domains_section(ctx)),
                ),
            ),
        )
        .child(
            view()
                .p(px(8.0))
                .border_t_1()
                .border_t_color(colors::BORDER)
                .child(sidebar_filter(ctx)),
        )
}

fn favorites_section(ctx: Ctx) -> impl IntoElement {
    view()
        .w_full()
        .child(section_title(
            "收藏",
            None,
            move || ctx.read(|m| m.expanded_sections.favorites),
            move || ctx.act(|m| m.toggle_section(Section::Favorites)),
        ))
        .child(
            when(move || ctx.read(|m| m.expanded_sections.favorites), {
                move || {
                    view()
                        .child(nav_row(
                            ctx,
                            IconName::Pin,
                            "固定请求".to_string(),
                            move || ctx.read(|m| m.pinned.len()),
                            move || ctx.read(|m| m.pinned_only),
                            move || ctx.act(|m| m.set_pinned_only(true)),
                        ))
                        .child(nav_row(
                            ctx,
                            IconName::Error,
                            "错误请求".to_string(),
                            move || {
                                ctx.read(|m| {
                                    m.app
                                        .requests
                                        .iter()
                                        .filter(|summary| {
                                            summary.error.is_some()
                                                || summary.status.is_some_and(|s| s >= 400)
                                        })
                                        .count()
                                })
                            },
                            move || ctx.read(|m| m.kind == KindFilter::Errors),
                            move || ctx.act(|m| m.set_kind(KindFilter::Errors)),
                        ))
                }
            })
            .otherwise(view),
        )
}

fn all_section(ctx: Ctx) -> impl IntoElement {
    let ctx_for_count = ctx;
    let ctx_for_active = ctx;
    let ctx_for_press = ctx;
    view().w_full().child(nav_row(
        ctx,
        IconName::Sidebar,
        "全部请求".to_string(),
        move || ctx_for_count.read(|m| m.app.requests.len()),
        move || {
            ctx_for_active.read(|m| {
                m.kind == KindFilter::All
                    && !m.pinned_only
                    && m.host.is_none()
                    && m.client.is_none()
                    && m.query.trim().is_empty()
            })
        },
        move || {
            ctx_for_press.act(|m| {
                m.set_kind(KindFilter::All);
                m.set_pinned_only(false);
                m.set_host(None);
                m.set_client(None);
            })
        },
    ))
}

fn tools_section(ctx: Ctx) -> impl IntoElement {
    view().w_full().mt(px(6.0)).child(nav_row(
        ctx,
        IconName::Rule,
        "规则引擎".to_string(),
        move || ctx.read(|m| m.settings.rules.len()),
        move || ctx.read(|m| m.view == View::Rules),
        move || ctx.act(|m| m.set_view(View::Rules)),
    ))
}

fn apps_section(ctx: Ctx) -> impl IntoElement {
    view()
        .w_full()
        .child(section_title(
            "应用",
            None,
            move || ctx.read(|m| m.expanded_sections.apps),
            move || ctx.act(|m| m.toggle_section(Section::Apps)),
        ))
        .child(
            when(move || ctx.read(|m| m.expanded_sections.apps), {
                move || {
                    each(
                        move || ctx.read(|m| m.clients.clone()),
                        |facet: &Facet| facet.name.clone(),
                        move |facet, _| facet_row(ctx, facet),
                    )
                }
            })
            .otherwise(view),
        )
}

fn domains_section(ctx: Ctx) -> impl IntoElement {
    view()
        .w_full()
        .child(section_title(
            "域名",
            None,
            move || ctx.read(|m| m.expanded_sections.domains),
            move || ctx.act(|m| m.toggle_section(Section::Domains)),
        ))
        .child(
            when(move || ctx.read(|m| m.expanded_sections.domains), {
                move || {
                    each(
                        move || ctx.read(|m| m.hosts.clone()),
                        |facet: &Facet| facet.name.clone(),
                        move |facet, _| facet_row(ctx, facet),
                    )
                }
            })
            .otherwise(view),
        )
}

fn facet_row(ctx: Ctx, facet: ReadSignal<Facet>) -> impl IntoElement {
    let name = facet.get_untracked().name;
    let icon = facet.get_untracked().icon;
    let active_name = name.clone();
    let ctx_for_active = ctx;
    let ctx_for_press = ctx;
    nav_row(
        ctx,
        icon,
        name.clone(),
        move || facet.get().count,
        move || {
            ctx_for_active.read(|m| {
                m.host.as_deref() == Some(active_name.as_str())
                    || m.client.as_deref() == Some(active_name.as_str())
            })
        },
        move || {
            ctx_for_press.act(|m| {
                if m.host.as_deref() == Some(name.as_str()) {
                    m.set_host(None);
                } else if m.client.as_deref() == Some(name.as_str()) {
                    m.set_client(None);
                } else if icon == IconName::Globe {
                    m.set_client(None);
                    m.set_host(Some(name.clone()));
                } else {
                    m.set_host(None);
                    m.set_client(Some(name.clone()));
                }
            })
        },
    )
}

fn nav_row(
    _ctx: Ctx,
    icon_name: IconName,
    label: String,
    count: impl Fn() -> usize + 'static,
    active: impl Fn() -> bool + 'static,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    let active: Rc<dyn Fn() -> bool> = Rc::new(active);
    let bg_active = active.clone();
    let icon_active = active.clone();
    let text_active = active.clone();
    view()
        .h(px(24.0))
        .mx(px(6.0))
        .px(px(7.0))
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .rounded(px(radius::CONTROL))
        .bg(move || {
            if bg_active() {
                colors::RAISED
            } else {
                AdaptiveColor::TRANSPARENT
            }
        })
        .hover(|s| s.bg(colors::RAISED))
        .cursor_pointer()
        .role(AccessibilityRole::Button)
        .label(label.clone())
        .on_press(on_press)
        .child(icon_reactive(icon_name, 13.0, move || {
            if icon_active() {
                colors::ACCENT
            } else {
                colors::MUTED
            }
        }))
        .child(
            text(label)
                .flex_1()
                .number_of_lines(1)
                .ellipsize_mode(EllipsizeMode::Middle)
                .text_size(px(12.0))
                .text_color(move || {
                    if text_active() {
                        colors::ACCENT
                    } else {
                        colors::TEXT
                    }
                }),
        )
        .child(
            text(move || count().to_string())
                .text_size(px(10.5))
                .text_color(colors::FAINT),
        )
}

fn sidebar_filter(ctx: Ctx) -> impl IntoElement {
    let ctx_value = ctx;
    let ctx_change = ctx;
    let ctx_clear = ctx;
    view()
        .h(px(28.0))
        .w_full()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .px(px(8.0))
        .rounded(px(radius::CONTROL))
        .bg(colors::SURFACE)
        .border_1()
        .border_color(colors::BORDER)
        .child(icon(IconName::Search, 13.0, colors::FAINT))
        .child(
            input()
                .element_ref(&ctx.filter_ref)
                .flex_1()
                .h_full()
                .value(move || ctx_value.read(|m| m.query.clone()))
                .placeholder("筛选 URL、域名或方法")
                .text_size(px(12.0))
                .text_color(colors::TEXT)
                .placeholder_color(color(colors::FAINT))
                .on_change(move |value: &str| {
                    let value = value.to_string();
                    ctx_change.act(|m| m.set_query(value));
                }),
        )
        .child(icon_button(
            IconName::Close,
            "清除筛选",
            colors::FAINT,
            move || ctx_clear.act(|m| m.set_query(String::new())),
        ))
}

pub fn status_bar(ctx: Ctx) -> impl IntoElement {
    let ctx_dot = ctx;
    let ctx_left = ctx;
    let ctx_center = ctx;
    let ctx_right = ctx;
    let clear = { move || ctx.act(|m| m.clear()) };
    let filter = { move || ctx.filter_ref.focus() };
    let auto_active: Rc<dyn Fn() -> bool> = Rc::new(move || ctx.read(|m| m.auto_select));
    let auto_bg = auto_active.clone();
    let auto_press = {
        move || {
            ctx.act(|m| {
                let next = m.auto_select;
                m.set_auto_select(!next);
            })
        }
    };
    view()
        .h(px(size::STATUS_BAR))
        .w_full()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .px(px(10.0))
        .bg(colors::SURFACE)
        .border_t_1()
        .border_t_color(colors::BORDER)
        .child(bar_pill("清空", None, clear))
        .child(bar_pill("筛选", None, filter))
        .child(bar_pill("自动选中", Some(auto_bg), auto_press))
        .child(
            text(move || {
                ctx_center.read(|m| {
                    let selected = if m.selected.is_some() { 1 } else { 0 };
                    format!(
                        "{selected} / {} 条已选中 · 显示 {} / {} 条",
                        m.rows.len(),
                        m.rows.len(),
                        m.app.requests.len()
                    )
                })
            })
            .text_size(px(11.5))
            .text_color(colors::FAINT),
        )
        .child(view().flex_1())
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .child(view().size(px(6.0)).rounded(px(3.0)).bg(move || {
                    ctx_dot.read(|m| {
                        if m.status.failed {
                            colors::RED
                        } else if m.status.running {
                            colors::GREEN
                        } else {
                            colors::FAINT
                        }
                    })
                }))
                .child(
                    text(move || {
                        ctx_left.read(|m| {
                            let state = if m.status.failed {
                                "异常"
                            } else if m.status.running {
                                "运行中"
                            } else {
                                "已停止"
                            };
                            match m.status.listen.as_deref() {
                                Some(addr) if m.status.running => {
                                    format!("代理{state} · {addr} · {} 条", m.status.captured)
                                }
                                _ => format!("代理{state} · {} 条", m.status.captured),
                            }
                        })
                    })
                    .text_size(px(11.0))
                    .text_color(colors::MUTED),
                ),
        )
        .child(
            text(move || {
                ctx_right.read(|m| {
                    if m.status.system_proxy {
                        "系统代理 开"
                    } else {
                        "系统代理 关"
                    }
                })
            })
            .text_size(px(11.0))
            .text_color(colors::FAINT),
        )
}

fn bar_pill(
    label: &'static str,
    active: Option<Rc<dyn Fn() -> bool>>,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    let bg_active = active.clone();
    let text_active = active;
    view()
        .h(px(21.0))
        .px(px(10.0))
        .items_center()
        .justify_center()
        .rounded(px(11.0))
        .bg(move || match &bg_active {
            Some(active) if active() => colors::ACCENT,
            _ => colors::RAISED,
        })
        .cursor_pointer()
        .role(AccessibilityRole::Button)
        .label(label)
        .hover(|s| s.opacity(0.85))
        .on_press(on_press)
        .child(
            text(label)
                .text_size(px(11.0))
                .text_color(move || match &text_active {
                    Some(active) if active() => colors::ON_ACCENT,
                    _ => colors::TEXT,
                }),
        )
}
