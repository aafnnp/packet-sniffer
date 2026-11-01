//! 请求列表：类型标签、表头与虚拟滚动的请求行。

use std::rc::Rc;

use nsh_core::EllipsizeMode;
use nsh_core::{
    element::{AccessibilityRole, AdaptiveColor, FontWeight},
    geometry::px,
    prelude::*,
};

use crate::format;
use crate::model::{KindFilter, SortKey};
use crate::theme::{colors, highlight_color, radius, size};
use desktop::storage::Highlight;
use nsh_base::{context_menu, menu_item, menu_separator, submenu};

use crate::ui::icons::{IconName, icon};
use crate::ui::widgets::{Ctx, accent_button, icon_button, subtle_button, tab_button};

enum Width {
    Fixed(f32),
    Flex,
}

pub fn request_table(ctx: Ctx) -> impl IntoElement {
    view()
        .flex_1()
        .min_w(px(0.0))
        .h_full()
        .flex_col()
        .overflow_hidden()
        .bg(colors::SURFACE)
        .child(kind_tabs(ctx))
        .child(header_row(ctx))
        .child(rows(ctx))
}

fn kind_tabs(ctx: Ctx) -> impl IntoElement {
    let mut tabs = view()
        .h(px(size::TAB_HEIGHT))
        .w_full()
        .flex_row()
        .items_center()
        .px(px(8.0))
        .gap(px(1.0))
        .bg(colors::CHROME)
        .border_b_1()
        .border_b_color(colors::BORDER);
    for kind in KindFilter::ALL {
        tabs = tabs.child(tab_button(
            kind.label(),
            move || ctx.read(|m| m.kind == kind),
            move || ctx.act(|m| m.set_kind(kind)),
        ));
    }
    tabs.child(view().w(px(1.0)).h(px(16.0)).mx(px(4.0)).bg(colors::BORDER))
        .child(when(|| window().width.0 >= 1150.0, move || method_pills(ctx)).otherwise(view))
        .child(view().flex_1())
        .child(
            text(move || {
                ctx.read(|m| {
                    if m.rows.len() == m.app.requests.len() {
                        format!("{} 条", m.rows.len())
                    } else {
                        format!("{} / {} 条", m.rows.len(), m.app.requests.len())
                    }
                })
            })
            .text_size(px(10.5))
            .text_color(colors::FAINT),
        )
        .child(icon_button(IconName::Gear, "设置", colors::MUTED, {
            move || ctx.act(|m| m.set_view(crate::model::View::Settings))
        }))
}

fn method_pills(ctx: Ctx) -> impl IntoElement {
    let mut row = view().flex_row().items_center().gap(px(2.0));
    for method in [
        None,
        Some("GET"),
        Some("POST"),
        Some("PUT"),
        Some("PATCH"),
        Some("DELETE"),
    ] {
        let label = method.unwrap_or("方法");
        let active: Rc<dyn Fn() -> bool> =
            Rc::new(move || ctx.read(|m| m.method.as_deref() == method));
        let ctx_press = ctx;
        let active_bg = active.clone();
        let active_text = active.clone();
        row = row.child(
            view()
                .h(px(22.0))
                .px(px(7.0))
                .items_center()
                .justify_center()
                .rounded(px(radius::CHIP))
                .bg(move || {
                    if active_bg() {
                        colors::ACCENT_SOFT
                    } else {
                        AdaptiveColor::TRANSPARENT
                    }
                })
                .hover(|s| s.bg(colors::RAISED))
                .cursor_pointer()
                .role(AccessibilityRole::Button)
                .label(label)
                .on_press(move || ctx_press.act(|m| m.set_method(method.map(str::to_string))))
                .child(text(label).text_size(px(10.5)).text_color(move || {
                    if active_text() {
                        colors::ACCENT
                    } else {
                        colors::FAINT
                    }
                })),
        );
    }
    row
}

fn header_row(ctx: Ctx) -> impl IntoElement {
    view()
        .h(px(size::TABLE_HEADER))
        .w_full()
        .flex_row()
        .items_center()
        .px(px(6.0))
        .gap(px(2.0))
        .bg(colors::RAISED)
        .border_b_1()
        .border_b_color(colors::BORDER)
        .child(header_button(ctx, SortKey::Index, Width::Fixed(38.0)))
        .child(header_button(ctx, SortKey::Url, Width::Flex))
        .child(header_button(ctx, SortKey::Client, Width::Fixed(104.0)))
        .child(header_button(ctx, SortKey::Method, Width::Fixed(58.0)))
        .child(header_button(ctx, SortKey::Status, Width::Fixed(52.0)))
        .child(header_button(ctx, SortKey::Time, Width::Fixed(92.0)))
        .child(header_button(ctx, SortKey::Duration, Width::Fixed(66.0)))
        .child(header_button(ctx, SortKey::RequestSize, Width::Fixed(60.0)))
        .child(header_button(
            ctx,
            SortKey::ResponseSize,
            Width::Fixed(66.0),
        ))
        .child(header_cell("SSL", Width::Fixed(32.0)))
}

fn header_button(ctx: Ctx, key: SortKey, width: Width) -> View {
    let mut cell = view()
        .h_full()
        .flex_row()
        .items_center()
        .gap(px(2.0))
        .px(px(4.0))
        .overflow_hidden()
        .rounded(px(3.0))
        .cursor_pointer()
        .role(AccessibilityRole::Button)
        .label(key.label())
        .hover(|s| s.bg(colors::LINE))
        .active(|s| s.opacity(0.6))
        .on_press(move || ctx.act(|m| m.set_sort(key)));
    cell = match width {
        Width::Fixed(width) => cell.w(px(width)).flex_shrink_0(),
        Width::Flex => cell.flex_1().min_w(px(100.0)),
    };
    cell.child(
        text(key.label())
            .number_of_lines(1)
            .text_size(px(10.0))
            .font_weight(FontWeight::MEDIUM)
            .text_color(move || {
                if ctx.read(|m| m.sort == key) {
                    colors::ACCENT
                } else {
                    colors::FAINT
                }
            }),
    )
    .child(
        when(
            move || ctx.read(|m| m.sort == key),
            move || {
                text(move || {
                    if ctx.read(|m| m.ascending) {
                        "↑"
                    } else {
                        "↓"
                    }
                })
                .text_size(px(9.0))
                .text_color(colors::ACCENT)
                .into_element()
            },
        )
        .otherwise(view),
    )
}

fn header_cell(label: &'static str, width: Width) -> View {
    let mut cell = view()
        .h_full()
        .flex_row()
        .items_center()
        .px(px(4.0))
        .overflow_hidden();
    cell = match width {
        Width::Fixed(width) => cell.w(px(width)).flex_shrink_0(),
        Width::Flex => cell.flex_1().min_w(px(100.0)),
    };
    cell.child(
        text(label)
            .number_of_lines(1)
            .text_size(px(10.0))
            .font_weight(FontWeight::MEDIUM)
            .text_color(colors::FAINT),
    )
}

fn rows(ctx: Ctx) -> impl IntoElement {
    let ctx_empty = ctx;
    view().flex_1().min_h(px(0.0)).child(
        when(
            move || ctx.read(|m| m.rows.is_empty()),
            move || empty_state(ctx_empty),
        )
        .otherwise({
            move || {
                virtual_list(move || ctx.read(|m| m.rows.len()), {
                    move |at| row(ctx, at)
                })
                .estimated_item_size(px(size::ROW))
                .follow_end(px(120.0))
                .flex_1()
            }
        })
        .into_element(),
    )
}

fn row(ctx: Ctx, at: Position) -> impl IntoElement {
    let sel: Rc<dyn Fn() -> bool> = {
        Rc::new(move || {
            ctx.read(|m| {
                m.rows
                    .get(at.index())
                    .is_some_and(|row| m.selected == Some(row.summary.id))
            })
        })
    };
    let tint = {
        let sel = sel.clone();
        move |base: AdaptiveColor| {
            let sel = sel.clone();
            move || {
                if sel() { colors::ON_ACCENT } else { base }
            }
        }
    };
    let client_tint = tint(colors::MUTED);

    let index = {
        move || {
            ctx.read(|m| {
                m.rows
                    .get(at.index())
                    .map_or_else(String::new, |row| row.index.to_string())
            })
        }
    };
    let url = {
        move || {
            ctx.read(|m| {
                m.rows
                    .get(at.index())
                    .map_or_else(String::new, |row| row.summary.url.clone())
            })
        }
    };
    let client = {
        move || {
            ctx.read(|m| {
                m.rows
                    .get(at.index())
                    .map_or_else(String::new, |row| row.client.clone())
            })
        }
    };
    let comment = {
        move || {
            ctx.read(|m| {
                m.rows
                    .get(at.index())
                    .map_or_else(String::new, |row| row.comment.clone())
            })
        }
    };
    let method = {
        move || {
            ctx.read(|m| {
                m.rows
                    .get(at.index())
                    .map_or_else(String::new, |row| row.summary.method.clone())
            })
        }
    };
    let status = {
        move || {
            ctx.read(|m| {
                m.rows.get(at.index()).map_or_else(String::new, |row| {
                    format::status_text(
                        row.summary.status,
                        row.summary.completed,
                        row.summary.error.as_deref(),
                    )
                })
            })
        }
    };
    let time = {
        move || {
            ctx.read(|m| {
                m.rows.get(at.index()).map_or_else(String::new, |row| {
                    format::format_time(row.summary.started_at)
                })
            })
        }
    };
    let duration = {
        move || {
            ctx.read(|m| {
                m.rows.get(at.index()).map_or_else(String::new, |row| {
                    format::format_duration(row.summary.duration_ms)
                })
            })
        }
    };
    let request_size = {
        move || {
            ctx.read(|m| {
                m.rows.get(at.index()).map_or_else(String::new, |row| {
                    format::format_bytes(row.summary.request_bytes)
                })
            })
        }
    };
    let response_size = {
        move || {
            ctx.read(|m| {
                m.rows.get(at.index()).map_or_else(String::new, |row| {
                    format::format_bytes(row.summary.response_bytes)
                })
            })
        }
    };

    let method_tint = {
        let sel = sel.clone();
        move || {
            if sel() {
                return colors::ON_ACCENT;
            }
            let method = ctx.read(|m| {
                m.rows
                    .get(at.index())
                    .map_or_else(String::new, |row| row.summary.method.clone())
            });
            format::method_color(&method)
        }
    };
    let status_tint = {
        let sel = sel.clone();
        move || {
            if sel() {
                return colors::ON_ACCENT;
            }
            ctx.read(|m| {
                m.rows.get(at.index()).map_or(colors::FAINT, |row| {
                    format::status_color(row.summary.status, row.summary.error.as_deref())
                })
            })
        }
    };
    let dot_tint = {
        let sel = sel.clone();
        move || {
            if sel() {
                return colors::ON_ACCENT;
            }
            ctx.read(|m| {
                m.rows.get(at.index()).map_or(colors::FAINT, |row| {
                    format::status_color(row.summary.status, row.summary.error.as_deref())
                })
            })
        }
    };

    let pressed = {
        move || {
            let id = ctx.read(|m| m.rows.get(at.index()).map(|row| row.summary.id));
            if let Some(id) = id {
                ctx.act(|m| m.select(id));
            }
        }
    };
    let row_bg = move || {
        let (selected, highlight) = ctx.read(|m| {
            let row = m.rows.get(at.index());
            (
                row.is_some_and(|row| m.selected == Some(row.summary.id)),
                row.map_or(Highlight::None, |row| row.highlight),
            )
        });
        if selected {
            colors::SELECTION
        } else if let Some(color) = highlight_color(highlight) {
            color.opacity(0.16)
        } else if at.index().is_multiple_of(2) {
            colors::SURFACE
        } else {
            colors::ALT
        }
    };

    let row_view = view()
        .h(px(size::ROW))
        .w_full()
        .flex_row()
        .items_center()
        .px(px(6.0))
        .gap(px(2.0))
        .bg(row_bg)
        .hover(|s| s.bg(colors::RAISED))
        .cursor_pointer()
        .role(AccessibilityRole::Button)
        .label("请求")
        .on_press(pressed)
        .child(
            view()
                .w(px(6.0))
                .h(px(6.0))
                .mx(px(6.0))
                .rounded(px(3.0))
                .bg(dot_tint),
        )
        .child(cell(index, tint(colors::FAINT), Width::Fixed(30.0)))
        .child(cell(url, tint(colors::TEXT), Width::Flex))
        .child(cell(client, client_tint, Width::Fixed(104.0)))
        .child(cell(method, method_tint, Width::Fixed(58.0)))
        .child(cell(status, status_tint, Width::Fixed(52.0)))
        .child(cell(time, tint(colors::MUTED), Width::Fixed(92.0)))
        .child(cell(duration, tint(colors::MUTED), Width::Fixed(66.0)))
        .child(cell(request_size, tint(colors::FAINT), Width::Fixed(60.0)))
        .child(cell(response_size, tint(colors::FAINT), Width::Fixed(66.0)))
        .child(cell(comment, tint(colors::MUTED), Width::Fixed(120.0)))
        .child(
            view()
                .w(px(32.0))
                .flex_row()
                .items_center()
                .justify_center()
                .gap(px(2.0))
                .child(
                    when(
                        {
                            move || {
                                ctx.read(|m| {
                                    m.rows.get(at.index()).is_some_and(|row| {
                                        row.summary.scheme == common::types::Scheme::Https
                                    })
                                })
                            }
                        },
                        || icon(IconName::Shield, 11.0, colors::GREEN),
                    )
                    .otherwise(view),
                )
                .child(
                    when(
                        {
                            move || {
                                ctx.read(|m| m.rows.get(at.index()).is_some_and(|row| row.pinned))
                            }
                        },
                        || icon(IconName::Pin, 10.0, colors::ACCENT),
                    )
                    .otherwise(view),
                ),
        );
    row_menu(ctx, at, row_view)
}

fn row_menu(ctx: Ctx, at: Position, trigger: impl IntoElement) -> impl IntoElement {
    context_menu()
        .w_full()
        .trigger(trigger)
        .popup_class("min-w-[190px] rounded-md border border-border bg-popover p-1 shadow-lg")
        .child(menu_entry("复制 URL", move || {
            let id = ctx.read(|m| m.rows.get(at.index()).map(|row| row.summary.id));
            if let Some(id) = id
                && let Some(url) = ctx.read(|m| m.url_of(id))
            {
                set_clipboard(url);
            }
        }))
        .child(menu_entry("复制为 cURL", move || {
            let id = ctx.read(|m| m.rows.get(at.index()).map(|row| row.summary.id));
            if let Some(id) = id
                && let Some(command) = ctx.read(|m| m.curl_of(id))
            {
                set_clipboard(command);
            }
        }))
        .child(menu_entry("复制请求原文", move || {
            let id = ctx.read(|m| m.rows.get(at.index()).map(|row| row.summary.id));
            if let Some(id) = id
                && let Some(raw) = ctx.read(|m| m.request_raw_of(id))
            {
                set_clipboard(raw);
            }
        }))
        .child(menu_entry("复制响应原文", move || {
            let id = ctx.read(|m| m.rows.get(at.index()).map(|row| row.summary.id));
            if let Some(id) = id
                && let Some(raw) = ctx.read(|m| m.response_raw_of(id))
            {
                set_clipboard(raw);
            }
        }))
        .child(menu_separator().h(px(1.0)).my(px(4.0)).bg(colors::BORDER))
        .child(menu_entry("固定 / 取消固定", move || {
            let id = ctx.read(|m| m.rows.get(at.index()).map(|row| row.summary.id));
            if let Some(id) = id {
                ctx.act(|m| m.toggle_pin_id(id));
            }
        }))
        .child(
            submenu()
                .trigger(menu_label("高亮标记"))
                .child(highlight_menu_entry(ctx, at, "红色", Highlight::Red))
                .child(highlight_menu_entry(ctx, at, "橙色", Highlight::Orange))
                .child(highlight_menu_entry(ctx, at, "黄色", Highlight::Yellow))
                .child(highlight_menu_entry(ctx, at, "绿色", Highlight::Green))
                .child(highlight_menu_entry(ctx, at, "蓝色", Highlight::Blue))
                .child(highlight_menu_entry(ctx, at, "紫色", Highlight::Purple))
                .child(highlight_menu_entry(ctx, at, "灰色", Highlight::Gray))
                .child(menu_entry("清除高亮", move || {
                    let id = ctx.read(|m| m.rows.get(at.index()).map(|row| row.summary.id));
                    if let Some(id) = id {
                        ctx.act(|m| m.set_highlight(id, Highlight::None));
                    }
                })),
        )
        .child(menu_entry("导出筛选结果 JSON", move || {
            ctx.act(|m| m.export_requests());
        }))
        .child(menu_entry("删除此请求", move || {
            let id = ctx.read(|m| m.rows.get(at.index()).map(|row| row.summary.id));
            if let Some(id) = id {
                ctx.act(|m| m.remove(id));
            }
        }))
        .child(menu_separator().h(px(1.0)).my(px(4.0)).bg(colors::BORDER))
        .child(menu_entry("清空全部", move || ctx.act(|m| m.clear())))
}

fn menu_label(label: &'static str) -> impl IntoElement {
    view()
        .px(px(8.0))
        .py(px(5.0))
        .child(text(label).text_size(px(12.0)).text_color(colors::TEXT))
}

fn highlight_menu_entry(
    ctx: Ctx,
    at: Position,
    label: &'static str,
    highlight: Highlight,
) -> impl IntoElement {
    menu_entry(label, move || {
        let id = ctx.read(|m| m.rows.get(at.index()).map(|row| row.summary.id));
        if let Some(id) = id {
            ctx.act(|m| m.set_highlight(id, highlight));
        }
    })
}

fn menu_entry(label: &'static str, on_select: impl Fn() + 'static) -> impl IntoElement {
    menu_item()
        .px(px(8.0))
        .py(px(5.0))
        .rounded(px(4.0))
        .hover(|s| s.bg(colors::RAISED))
        .active(|s| s.opacity(0.6))
        .on_select(on_select)
        .child(text(label).text_size(px(12.0)).text_color(colors::TEXT))
}

fn cell(
    value: impl Fn() -> String + 'static,
    color: impl Fn() -> AdaptiveColor + 'static,
    width: Width,
) -> View {
    let mut cell = view()
        .h_full()
        .flex_row()
        .items_center()
        .px(px(4.0))
        .overflow_hidden();
    cell = match width {
        Width::Fixed(width) => cell.w(px(width)).flex_shrink_0(),
        Width::Flex => cell.flex_1().min_w(px(100.0)),
    };
    cell.child(
        text(value)
            .number_of_lines(1)
            .ellipsize_mode(EllipsizeMode::Tail)
            .text_size(px(11.5))
            .font_family(crate::theme::MONO)
            .text_color(color),
    )
}

fn empty_state(ctx: Ctx) -> impl IntoElement {
    let total = ctx.read(|m| m.app.requests.len());
    if total > 0 {
        let reset = ctx;
        return view()
            .w_full()
            .h_full()
            .items_center()
            .justify_center()
            .gap(px(10.0))
            .bg(colors::SURFACE)
            .child(icon(IconName::Search, 34.0, colors::BORDER))
            .child(
                text("没有匹配的请求")
                    .text_size(px(16.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(colors::TEXT),
            )
            .child(
                text("调整筛选条件，或重置后查看全部请求。")
                    .text_size(px(12.0))
                    .text_color(colors::MUTED),
            )
            .child(accent_button("重置筛选", move || {
                reset.act(|m| m.reset_filters())
            }))
            .into_element();
    }
    let start = ctx;
    let install = ctx;
    view()
        .w_full()
        .h_full()
        .items_center()
        .justify_center()
        .gap(px(10.0))
        .bg(colors::SURFACE)
        .child(icon(IconName::Globe, 44.0, colors::BORDER))
        .child(
            text("还没有抓到请求")
                .text_size(px(17.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors::TEXT),
        )
        .child(
            text("启动代理、安装根证书，再把系统代理指向本机端口即可开始抓包。")
                .text_size(px(12.0))
                .text_color(colors::MUTED),
        )
        .child(
            when(
                move || ctx.read(|m| m.status.running),
                || {
                    text("代理已运行，等待请求…")
                        .text_size(px(11.5))
                        .text_color(colors::GREEN)
                        .into_element()
                },
            )
            .otherwise(move || {
                accent_button("启动代理", move || start.act(|m| m.start_engine())).into_element()
            }),
        )
        .child(subtle_button("安装根证书", move || {
            install.act(|m| m.install_ca())
        }))
        .into_element()
}
