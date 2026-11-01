//! 详情检查器：请求 / 响应并排，按首部、查询、正文与原始数据分页。

use desktop::storage::Highlight;
use nsh_core::EllipsizeMode;
use nsh_core::{
    element::{AccessibilityRole, FontWeight, input},
    geometry::px,
    prelude::*,
};

use crate::format;
use crate::model::{RequestTab, ResponseTab};
use crate::theme::{colors, highlight_color, radius, size};
use crate::ui::icons::{IconName, icon, icon_reactive};
use crate::ui::widgets::{Ctx, icon_button, tab_button};
use nsh_base::editor::editor;

pub fn inspector(ctx: Ctx) -> impl IntoElement {
    view()
        .h(move || px((window().height.0 * 0.47).max(250.0)))
        .w_full()
        .flex_shrink_0()
        .flex_col()
        .bg(colors::SURFACE)
        .border_t_1()
        .border_t_color(colors::BORDER)
        .child(detail_bar(ctx))
        .child(
            when(
                move || ctx.read(|m| m.detail.is_some()),
                move || annotation_bar(ctx),
            )
            .otherwise(view),
        )
        .child(
            view()
                .flex_1()
                .min_h(px(0.0))
                .flex_row()
                .child(request_pane(ctx))
                .child(response_pane(ctx)),
        )
}

fn detail_bar(ctx: Ctx) -> impl IntoElement {
    let copy = {
        move || {
            let url = ctx.read(|m| {
                m.detail
                    .as_ref()
                    .map(|detail| detail.summary.url.clone())
                    .unwrap_or_default()
            });
            set_clipboard(url);
        }
    };
    view()
        .h(px(38.0))
        .w_full()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .px(px(10.0))
        .bg(colors::CHROME)
        .border_b_1()
        .border_b_color(colors::BORDER)
        .child(
            when(move || ctx.read(|m| m.detail.is_some()), {
                move || {
                    let method = text(move || {
                        ctx.read(|m| {
                            m.detail
                                .as_ref()
                                .map(|detail| detail.summary.method.clone())
                                .unwrap_or_default()
                        })
                    });
                    let status = text(move || {
                        ctx.read(|m| {
                            m.detail.as_ref().map_or_else(String::new, |detail| {
                                format::status_text(detail.status, true, detail.error.as_deref())
                            })
                        })
                    });
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            view()
                                .px(px(7.0))
                                .py(px(2.0))
                                .rounded(px(9.0))
                                .border_1()
                                .border_color(move || {
                                    ctx.read(|m| {
                                        m.detail.as_ref().map_or(colors::MUTED, |detail| {
                                            format::method_color(&detail.summary.method)
                                        })
                                    })
                                })
                                .bg(move || {
                                    ctx.read(|m| {
                                        m.detail.as_ref().map_or(colors::MUTED, |detail| {
                                            format::method_color(&detail.summary.method)
                                                .opacity(0.16)
                                        })
                                    })
                                })
                                .child(
                                    method
                                        .text_size(px(11.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(move || {
                                            ctx.read(|m| {
                                                m.detail.as_ref().map_or(colors::MUTED, |detail| {
                                                    format::method_color(&detail.summary.method)
                                                })
                                            })
                                        }),
                                ),
                        )
                        .child(
                            view()
                                .px(px(7.0))
                                .py(px(2.0))
                                .rounded(px(9.0))
                                .border_1()
                                .border_color(move || {
                                    ctx.read(|m| {
                                        m.detail.as_ref().map_or(colors::FAINT, |detail| {
                                            format::status_color(
                                                detail.status,
                                                detail.error.as_deref(),
                                            )
                                        })
                                    })
                                })
                                .bg(move || {
                                    ctx.read(|m| {
                                        m.detail.as_ref().map_or(colors::FAINT, |detail| {
                                            format::status_color(
                                                detail.status,
                                                detail.error.as_deref(),
                                            )
                                            .opacity(0.14)
                                        })
                                    })
                                })
                                .child(
                                    status
                                        .text_size(px(11.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(move || {
                                            ctx.read(|m| {
                                                m.detail.as_ref().map_or(colors::FAINT, |detail| {
                                                    format::status_color(
                                                        detail.status,
                                                        detail.error.as_deref(),
                                                    )
                                                })
                                            })
                                        }),
                                ),
                        )
                        .into_element()
                }
            })
            .otherwise(view),
        )
        .child(
            when(
                move || {
                    ctx.read(|m| {
                        m.detail
                            .as_ref()
                            .is_some_and(|detail| detail.highlight != Highlight::None)
                    })
                },
                move || {
                    view()
                        .size(px(8.0))
                        .rounded(px(4.0))
                        .bg(move || {
                            ctx.read(|m| {
                                m.detail
                                    .as_ref()
                                    .and_then(|detail| highlight_color(detail.highlight))
                                    .unwrap_or(AdaptiveColor::TRANSPARENT)
                            })
                        })
                        .into_element()
                },
            )
            .otherwise(view),
        )
        .child(
            text(move || {
                ctx.read(|m| {
                    m.detail.as_ref().map_or_else(
                        || "选择一条请求查看详情".to_string(),
                        |d| d.summary.url.clone(),
                    )
                })
            })
            .flex_1()
            .number_of_lines(1)
            .ellipsize_mode(EllipsizeMode::Middle)
            .font_family(crate::theme::MONO)
            .text_size(px(11.5))
            .text_color(move || {
                if ctx.read(|m| m.detail.is_some()) {
                    colors::BLUE
                } else {
                    colors::MUTED
                }
            }),
        )
        .child(icon_button(IconName::Copy, "复制 URL", colors::MUTED, copy))
        .child(
            view()
                .size(px(28.0))
                .items_center()
                .justify_center()
                .rounded(px(6.0))
                .cursor_pointer()
                .role(AccessibilityRole::Button)
                .label("固定请求")
                .hover(|s| s.bg(colors::RAISED))
                .active(|s| s.opacity(0.55))
                .on_press(move || ctx.act(|m| m.toggle_pin()))
                .child(icon_reactive(IconName::Pin, 16.0, move || {
                    if ctx.read(|m| m.detail.as_ref().is_some_and(|detail| detail.pinned)) {
                        colors::ACCENT
                    } else {
                        colors::MUTED
                    }
                })),
        )
}

fn annotation_bar(ctx: Ctx) -> impl IntoElement {
    let current = move || {
        ctx.read(|m| {
            m.detail
                .as_ref()
                .map_or(Highlight::None, |detail| detail.highlight)
        })
    };
    let mut bar = view()
        .h(px(34.0))
        .w_full()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .px(px(10.0))
        .bg(colors::CHROME)
        .border_b_1()
        .border_b_color(colors::BORDER)
        .child(text("高亮").text_size(px(11.0)).text_color(colors::MUTED));
    for highlight in Highlight::ALL {
        let active = move || current() == highlight;
        let mut dot = view()
            .size(px(13.0))
            .rounded(px(6.5))
            .border_1()
            .border_color(move || {
                if active() {
                    colors::TEXT
                } else {
                    colors::BORDER
                }
            })
            .cursor_pointer()
            .role(AccessibilityRole::Button)
            .label(highlight.label())
            .on_press(move || {
                if let Some(id) = ctx.read(|m| m.detail.as_ref().map(|detail| detail.summary.id)) {
                    ctx.act(|m| m.set_highlight(id, highlight));
                }
            });
        if let Some(fill) = highlight_color(highlight) {
            dot = dot.bg(fill);
        } else {
            dot = dot
                .bg(colors::RAISED)
                .items_center()
                .justify_center()
                .child(text("×").text_size(px(9.0)).text_color(colors::MUTED));
        }
        bar = bar.child(dot);
    }
    bar.child(view().w(px(6.0)))
        .child(text("备注").text_size(px(11.0)).text_color(colors::MUTED))
        .child(
            input()
                .flex_1()
                .h(px(24.0))
                .px(px(8.0))
                .rounded(px(radius::CONTROL))
                .bg(colors::SURFACE)
                .border_1()
                .border_color(colors::BORDER)
                .text_size(px(11.5))
                .text_color(colors::TEXT)
                .placeholder("为这条请求添加备注…")
                .placeholder_color(color(colors::FAINT))
                .value(move || {
                    ctx.read(|m| {
                        m.detail
                            .as_ref()
                            .map_or_else(String::new, |detail| detail.comment.clone())
                    })
                })
                .on_change(move |value: &str| {
                    let comment = value.to_string();
                    if let Some(id) =
                        ctx.read(|m| m.detail.as_ref().map(|detail| detail.summary.id))
                    {
                        ctx.act(|m| m.set_comment(id, comment));
                    }
                }),
        )
}

fn empty_detail() -> impl IntoElement {
    view()
        .w_full()
        .py(px(28.0))
        .items_center()
        .justify_center()
        .gap(px(8.0))
        .child(icon(IconName::Info, 28.0, colors::BORDER))
        .child(
            text("选择请求后在这里查看内容")
                .text_size(px(12.0))
                .text_color(colors::FAINT),
        )
}

fn request_pane(ctx: Ctx) -> impl IntoElement {
    view()
        .flex_grow(0.92)
        .flex_shrink(1.0)
        .basis(px(0.0))
        .min_w(px(0.0))
        .h_full()
        .flex_col()
        .overflow_hidden()
        .border_r_1()
        .border_r_color(colors::BORDER)
        .child(pane_tabs_request(ctx))
        .child(request_content(ctx))
}

fn response_pane(ctx: Ctx) -> impl IntoElement {
    view()
        .flex_grow(1.08)
        .flex_shrink(1.0)
        .basis(px(0.0))
        .min_w(px(0.0))
        .h_full()
        .flex_col()
        .overflow_hidden()
        .child(pane_tabs_response(ctx))
        .child(response_content(ctx))
}

fn pane_tabs_request(ctx: Ctx) -> impl IntoElement {
    let mut bar = view()
        .h(px(size::INSPECTOR_HEADER))
        .w_full()
        .flex_row()
        .items_center()
        .px(px(6.0))
        .gap(px(2.0))
        .bg(colors::CHROME)
        .border_b_1()
        .border_b_color(colors::BORDER);
    for tab in RequestTab::ALL {
        bar = bar.child(tab_button(
            tab.label(),
            move || ctx.read(|m| m.request_tab == tab),
            move || ctx.act(|m| m.set_request_tab(tab)),
        ));
    }
    let copy = {
        move || {
            let raw = ctx.read(|m| {
                m.detail
                    .as_ref()
                    .map_or_else(String::new, |d| d.request_raw.clone())
            });
            set_clipboard(raw);
        }
    };
    bar.child(view().flex_1())
        .child(icon_button(IconName::Copy, "复制请求", colors::FAINT, copy))
}

fn pane_tabs_response(ctx: Ctx) -> impl IntoElement {
    let mut bar = view()
        .h(px(size::INSPECTOR_HEADER))
        .w_full()
        .flex_row()
        .items_center()
        .px(px(6.0))
        .gap(px(2.0))
        .bg(colors::CHROME)
        .border_b_1()
        .border_b_color(colors::BORDER);
    for tab in ResponseTab::ALL {
        bar = bar.child(tab_button(
            tab.label(),
            move || ctx.read(|m| m.response_tab == tab),
            move || ctx.act(|m| m.set_response_tab(tab)),
        ));
    }
    let copy = {
        move || {
            let raw = ctx.read(|m| {
                m.detail
                    .as_ref()
                    .map_or_else(String::new, |d| d.response_raw.clone())
            });
            set_clipboard(raw);
        }
    };
    bar.child(view().flex_1())
        .child(icon_button(IconName::Copy, "复制响应", colors::FAINT, copy))
}

fn request_content(ctx: Ctx) -> impl IntoElement {
    view()
        .flex_1()
        .min_h(px(0.0))
        .w_full()
        .overflow_hidden()
        .child(
            when(
                move || ctx.read(|m| m.detail.is_some()),
                move || request_tabs_content(ctx),
            )
            .otherwise(|| scrolled(empty_detail())),
        )
}

fn request_tabs_content(ctx: Ctx) -> impl IntoElement {
    view().flex_1().min_h(px(0.0)).w_full().child(
        when(
            move || ctx.read(|m| m.request_tab == RequestTab::Summary),
            move || scrolled(request_summary(ctx)),
        )
        .otherwise({
            move || {
                view().flex_1().min_h(px(0.0)).w_full().child(
                    when(
                        move || ctx.read(|m| m.request_tab == RequestTab::Headers),
                        move || scrolled(header_table(ctx, true)),
                    )
                    .otherwise({
                        move || {
                            view().flex_1().min_h(px(0.0)).w_full().child(
                                when(move || ctx.read(|m| m.request_tab == RequestTab::Query), {
                                    move || scrolled(query_table(ctx))
                                })
                                .otherwise(move || body_view(ctx, true)),
                            )
                        }
                    }),
                )
            }
        }),
    )
}

fn response_content(ctx: Ctx) -> impl IntoElement {
    view()
        .flex_1()
        .min_h(px(0.0))
        .w_full()
        .overflow_hidden()
        .child(
            when(
                move || ctx.read(|m| m.detail.is_some()),
                move || response_tabs_content(ctx),
            )
            .otherwise(|| scrolled(empty_detail())),
        )
}

fn response_tabs_content(ctx: Ctx) -> impl IntoElement {
    view().flex_1().min_h(px(0.0)).w_full().child(
        when(
            move || ctx.read(|m| m.response_tab == ResponseTab::Summary),
            move || scrolled(response_summary(ctx)),
        )
        .otherwise({
            move || {
                view().flex_1().min_h(px(0.0)).w_full().child(
                    when(
                        move || ctx.read(|m| m.response_tab == ResponseTab::Headers),
                        move || scrolled(header_table(ctx, false)),
                    )
                    .otherwise(move || body_view(ctx, false)),
                )
            }
        }),
    )
}

/// 可滚动的内容页（概要、首部、查询与纯文本正文）。
fn scrolled(content: impl IntoElement) -> impl IntoElement {
    scroll().flex_1().min_h(px(0.0)).child(content)
}

fn request_summary(ctx: Ctx) -> impl IntoElement {
    let ctx1 = ctx;
    let ctx2 = ctx;
    let ctx3 = ctx;
    let ctx4 = ctx;
    view()
        .w_full()
        .p(px(10.0))
        .gap(px(4.0))
        .child(summary_title("请求"))
        .child(kv("URL", move || {
            ctx1.read(|m| {
                m.detail
                    .as_ref()
                    .map_or_else(String::new, |d| d.summary.url.clone())
            })
        }))
        .child(kv("方法", move || {
            ctx2.read(|m| {
                m.detail
                    .as_ref()
                    .map_or_else(String::new, |d| d.summary.method.clone())
            })
        }))
        .child(kv("客户端", move || {
            ctx3.read(|m| {
                m.detail
                    .as_ref()
                    .map_or_else(String::new, |d| d.client.clone())
            })
        }))
        .child(kv("内容类型", move || {
            ctx4.read(|m| {
                m.detail
                    .as_ref()
                    .and_then(|d| d.content_type.clone())
                    .unwrap_or_else(|| "—".to_string())
            })
        }))
        .child(summary_title("连接"))
        .child(kv("主机", {
            move || {
                ctx.read(|m| {
                    m.detail.as_ref().map_or_else(String::new, |d| {
                        format!("{}:{}", d.summary.host, d.summary.scheme.default_port())
                    })
                })
            }
        }))
        .child(kv("耗时", {
            move || {
                ctx.read(|m| {
                    m.detail
                        .as_ref()
                        .map_or_else(String::new, |d| format::format_duration(d.duration_ms))
                })
            }
        }))
        .child(kv("请求大小", {
            move || {
                ctx.read(|m| {
                    m.detail
                        .as_ref()
                        .map_or_else(String::new, |d| format::format_bytes(d.request_bytes))
                })
            }
        }))
        .child(kv("开始时间", {
            move || {
                ctx.read(|m| {
                    m.detail
                        .as_ref()
                        .map_or_else(String::new, |d| format::format_time(d.summary.started_at))
                })
            }
        }))
}

fn response_summary(ctx: Ctx) -> impl IntoElement {
    let ctx1 = ctx;
    let ctx2 = ctx;
    let ctx3 = ctx;
    let ctx4 = ctx;
    let ctx5 = ctx;
    view()
        .w_full()
        .p(px(10.0))
        .gap(px(4.0))
        .child(summary_title("响应"))
        .child(kv("状态", move || {
            ctx1.read(|m| {
                m.detail.as_ref().map_or_else(String::new, |d| {
                    format::status_text(d.status, true, d.error.as_deref())
                })
            })
        }))
        .child(kv("内容类型", move || {
            ctx2.read(|m| {
                m.detail
                    .as_ref()
                    .and_then(|d| d.response_content_type.clone())
                    .unwrap_or_else(|| "—".to_string())
            })
        }))
        .child(kv("大小", move || {
            ctx3.read(|m| {
                m.detail
                    .as_ref()
                    .map_or_else(String::new, |d| format::format_bytes(d.response_bytes))
            })
        }))
        .child(kv("客户端", move || {
            ctx4.read(|m| {
                m.detail
                    .as_ref()
                    .map_or_else(String::new, |d| d.client.clone())
            })
        }))
        .child(kv("错误", move || {
            ctx5.read(|m| {
                m.detail
                    .as_ref()
                    .and_then(|d| d.error.clone())
                    .unwrap_or_else(|| "—".to_string())
            })
        }))
}

fn summary_title(label: &'static str) -> impl IntoElement {
    view().mt(px(6.0)).child(
        text(label)
            .text_size(px(10.5))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(colors::FAINT),
    )
}

fn kv(label: &'static str, value: impl Fn() -> String + 'static) -> impl IntoElement {
    view()
        .flex_row()
        .gap(px(8.0))
        .h(px(20.0))
        .items_center()
        .child(
            text(label)
                .w(px(76.0))
                .flex_shrink_0()
                .text_size(px(11.0))
                .text_color(colors::MUTED),
        )
        .child(
            text(value)
                .flex_1()
                .number_of_lines(1)
                .ellipsize_mode(EllipsizeMode::Tail)
                .font_family(crate::theme::MONO)
                .text_size(px(11.0))
                .text_color(colors::TEXT),
        )
}

fn header_table(ctx: Ctx, request: bool) -> impl IntoElement {
    let rows = {
        move || {
            ctx.read(|m| {
                m.detail.as_ref().map_or_else(Vec::new, |detail| {
                    let rows = if request {
                        &detail.request_headers
                    } else {
                        &detail.response_headers
                    };
                    rows.iter()
                        .enumerate()
                        .map(|(index, (name, value))| (index, name.clone(), value.clone()))
                        .collect()
                })
            })
        }
    };
    view()
        .w_full()
        .child(
            view()
                .h(px(24.0))
                .flex_row()
                .items_center()
                .px(px(10.0))
                .bg(colors::RAISED)
                .border_b_1()
                .border_b_color(colors::BORDER)
                .child(header_label("Key", true))
                .child(header_label("Value", false)),
        )
        .child(each(
            rows,
            |(index, name, value): &(usize, String, String)| format!("{index}:{name}:{value}"),
            {
                move |row, _| {
                    let (_, name, value) = row.get_untracked();
                    view()
                        .min_h(px(22.0))
                        .flex_row()
                        .items_start()
                        .px(px(10.0))
                        .py(px(2.0))
                        .border_b_1()
                        .border_b_color(colors::LINE)
                        .child(
                            text(name)
                                .w(px(150.0))
                                .flex_shrink_0()
                                .font_family(crate::theme::MONO)
                                .text_size(px(10.5))
                                .text_color(colors::MUTED),
                        )
                        .child(
                            text(value)
                                .flex_1()
                                .font_family(crate::theme::MONO)
                                .text_size(px(10.5))
                                .text_color(colors::TEXT),
                        )
                }
            },
        ))
}

fn header_label(label: &'static str, fixed: bool) -> impl IntoElement {
    let element = text(label)
        .text_size(px(10.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(colors::FAINT);
    if fixed {
        element.w(px(150.0)).flex_shrink_0().into_element()
    } else {
        element.flex_1().into_element()
    }
}

fn query_table(ctx: Ctx) -> impl IntoElement {
    let rows = {
        move || {
            ctx.read(|m| {
                m.detail.as_ref().map_or_else(Vec::new, |detail| {
                    detail
                        .query
                        .iter()
                        .enumerate()
                        .map(|(index, (name, value))| (index, name.clone(), value.clone()))
                        .collect()
                })
            })
        }
    };
    view()
        .w_full()
        .child(
            view()
                .h(px(24.0))
                .flex_row()
                .items_center()
                .px(px(10.0))
                .bg(colors::RAISED)
                .border_b_1()
                .border_b_color(colors::BORDER)
                .child(header_label("Key", true))
                .child(header_label("Value", false)),
        )
        .child(each(
            rows,
            |(index, name, value): &(usize, String, String)| format!("{index}:{name}:{value}"),
            |row, _| {
                let (_, name, value) = row.get_untracked();
                view()
                    .min_h(px(22.0))
                    .flex_row()
                    .items_start()
                    .px(px(10.0))
                    .py(px(2.0))
                    .border_b_1()
                    .border_b_color(colors::LINE)
                    .child(
                        text(name)
                            .w(px(150.0))
                            .flex_shrink_0()
                            .font_family(crate::theme::MONO)
                            .text_size(px(10.5))
                            .text_color(colors::MUTED),
                    )
                    .child(
                        text(value)
                            .flex_1()
                            .font_family(crate::theme::MONO)
                            .text_size(px(10.5))
                            .text_color(colors::TEXT),
                    )
            },
        ))
        .child(
            when(
                move || ctx.read(|m| m.detail.as_ref().is_some_and(|d| d.query.is_empty())),
                || {
                    view().p(px(12.0)).child(
                        text("此请求没有查询参数")
                            .text_size(px(11.0))
                            .text_color(colors::FAINT),
                    )
                },
            )
            .otherwise(view),
        )
}

fn body_view(ctx: Ctx, request: bool) -> impl IntoElement {
    view().flex_1().min_h(px(0.0)).w_full().child(
        when(
            move || body_visible(ctx, request),
            move || {
                when(
                    move || !raw_body(ctx, request) && body_json(ctx, request).is_some(),
                    move || json_body_view(ctx, request),
                )
                .otherwise(move || scrolled(plain_body(ctx, request)))
            },
        )
        .otherwise(view),
    )
}

fn body_visible(ctx: Ctx, request: bool) -> bool {
    ctx.read(|m| {
        if request {
            m.request_tab == RequestTab::Body || m.request_tab == RequestTab::Raw
        } else {
            m.response_tab == ResponseTab::Body
                || m.response_tab == ResponseTab::Raw
                || m.response_tab == ResponseTab::Json
        }
    })
}

fn raw_body(ctx: Ctx, request: bool) -> bool {
    ctx.read(|m| {
        if request {
            m.request_tab == RequestTab::Raw
        } else {
            m.response_tab == ResponseTab::Raw
        }
    })
}

/// body 的 JSON 美化文本；不是 JSON 时为 `None`。
fn body_json(ctx: Ctx, request: bool) -> Option<String> {
    ctx.read(|m| {
        m.detail.as_ref().and_then(|detail| {
            if request {
                detail.request_json.clone()
            } else {
                detail.response_json.clone()
            }
        })
    })
}

/// 带行号的纯文本正文（「原始」页与非 JSON 正文）。
fn plain_body(ctx: Ctx, request: bool) -> impl IntoElement {
    view().w_full().p(px(8.0)).child(
        text(move || numbered(&body_text(ctx, request)))
            .w_full()
            .font_family(crate::theme::MONO)
            .text_size(px(10.5))
            .line_height(px(15.0))
            .text_color(colors::TEXT),
    )
}

/// JSON 正文：只读代码编辑器，自定义语法高亮、行号与折叠。
fn json_body_view(ctx: Ctx, request: bool) -> impl IntoElement {
    view().flex_1().min_h(px(0.0)).w_full().child(
        editor()
            .read_only(true)
            .language(SharedString::from("json"))
            .line_numbers(true)
            .folding(true)
            .active_line(false)
            .indent_guides(false)
            .tab_size(2u32)
            .font_size(px(11.0))
            .line_height(px(15.0))
            .accessibility_label("JSON 正文")
            .value(move || SharedString::from(body_json(ctx, request).unwrap_or_default()))
            .size_full(),
    )
}

fn body_text(ctx: Ctx, request: bool) -> String {
    ctx.read(|m| {
        let raw = if request {
            m.request_tab == RequestTab::Raw
        } else {
            m.response_tab == ResponseTab::Raw
        };
        m.detail
            .as_ref()
            .map_or_else(String::new, |detail| match (request, raw) {
                (true, true) => detail.request_raw.clone(),
                (true, false) => detail.request_body.clone(),
                (false, true) => detail.response_raw.clone(),
                (false, false) => detail.response_body.clone(),
            })
    })
}

fn numbered(text: &str) -> String {
    if text.lines().count() <= 1 {
        return format!("   1  {text}");
    }
    text.lines()
        .enumerate()
        .map(|(index, line)| format!("{:>4}  {line}", index + 1))
        .collect::<Vec<_>>()
        .join("\n")
}
