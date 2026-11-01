//! 规则引擎页面：匹配条件、动作编辑与保存应用。

use nsh_core::{
    element::{AccessibilityRole, FontWeight, input},
    geometry::px,
    prelude::*,
};
use proxy_engine::rules::{Rule, RuleAction};

use crate::model::View;
use crate::theme::{colors, radius};
use crate::ui::icons::{IconName, icon};
use crate::ui::widgets::{Ctx, accent_button, icon_button, subtle_button, toggle};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActionKind {
    Block,
    Mock,
    RequestHeader,
    ResponseHeader,
}

fn action_kind(action: &RuleAction) -> ActionKind {
    match action {
        RuleAction::Block { .. } => ActionKind::Block,
        RuleAction::Mock { .. } => ActionKind::Mock,
        RuleAction::AddRequestHeader { .. } => ActionKind::RequestHeader,
        RuleAction::AddResponseHeader { .. } => ActionKind::ResponseHeader,
    }
}

fn default_action(kind: ActionKind) -> RuleAction {
    match kind {
        ActionKind::Block => RuleAction::Block { status: 403 },
        ActionKind::Mock => RuleAction::Mock {
            status: 200,
            content_type: "application/json".to_string(),
            body: "{\"ok\":true}".to_string(),
        },
        ActionKind::RequestHeader => RuleAction::AddRequestHeader {
            name: "X-Debug".to_string(),
            value: "1".to_string(),
        },
        ActionKind::ResponseHeader => RuleAction::AddResponseHeader {
            name: "X-Debug".to_string(),
            value: "1".to_string(),
        },
    }
}

pub fn rules_page(ctx: Ctx) -> impl IntoElement {
    let back = { move || ctx.act(|m| m.set_view(View::Traffic)) };
    let add = { move || ctx.act(|m| m.add_rule()) };
    let apply = { move || ctx.act(|m| m.apply_rules()) };
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
                    text("规则引擎")
                        .text_size(px(16.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors::TEXT),
                )
                .child(when(
                    move || ctx.read(|m| m.rules_dirty),
                    || {
                        view()
                            .flex_row()
                            .items_center()
                            .gap(px(4.0))
                            .child(
                                view()
                                    .size(px(6.0))
                                    .rounded(px(3.0))
                                    .bg(colors::ORANGE),
                            )
                            .child(
                                text("有未应用的修改")
                                    .text_size(px(11.0))
                                    .text_color(colors::ORANGE),
                            )
                            .into_element()
                    },
                )
                .otherwise(view))
                .child(view().flex_1())
                .child(subtle_button("新建规则", add))
                .child(accent_button("应用规则", apply)),
        )
        .child(
            scroll().flex_1().child(
                view()
                    .w_full()
                    .items_center()
                    .py(px(16.0))
                    .child(
                        view()
                            .w(px(780.0))
                            .gap(px(10.0))
                            .child(
                                text("规则按顺序匹配；阻止与模拟响应会直接结束转发，首部规则在转发前后生效。保存后会重启代理。")
                                    .text_size(px(11.0))
                                    .text_color(colors::FAINT),
                            )
                            .child(
                                when(
                                    move || ctx.read(|m| m.settings.rules.is_empty()),
                                    {
                                        move || empty_state(ctx)
                                    },
                                )
                                .otherwise(move || {
                                    each(
                                        move || ctx.read(|m| m.settings.rules.clone()),
                                        |rule: &Rule| rule.id.clone(),
                                        move |rule, index| rule_card(ctx, rule, index),
                                    )
                                }),
                            ),
                    ),
            ),
        )
}

fn empty_state(ctx: Ctx) -> impl IntoElement {
    let add = { move || ctx.act(|m| m.add_rule()) };
    view()
        .w_full()
        .py(px(40.0))
        .items_center()
        .gap(px(10.0))
        .rounded(px(radius::PANEL))
        .bg(colors::SURFACE)
        .border_1()
        .border_color(colors::BORDER)
        .child(icon(IconName::Rule, 34.0, colors::BORDER))
        .child(
            text("还没有规则")
                .text_size(px(15.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors::TEXT),
        )
        .child(
            text("新建一条规则来阻止请求、模拟响应，或在转发时改写首部。")
                .text_size(px(11.5))
                .text_color(colors::MUTED),
        )
        .child(accent_button("新建规则", add))
}

fn rule_card(ctx: Ctx, rule: ReadSignal<Rule>, index: ReadSignal<usize>) -> impl IntoElement {
    let enabled_checked = { move || rule.get().enabled };
    let enabled_toggle = {
        move || {
            let index = index.get();
            let enabled =
                ctx.read(|m| m.settings.rules.get(index).is_some_and(|rule| rule.enabled));
            ctx.act(move |m| {
                m.update_rule(index, |rule| rule.enabled = !enabled);
            });
        }
    };
    let name = { move || rule.get().name.clone() };
    let name_change = {
        move |value: &str| {
            let value = value.to_string();
            let index = index.get();
            ctx.act(move |m| {
                m.update_rule(index, |rule| rule.name = value);
            });
        }
    };
    let delete = {
        move || {
            let index = index.get();
            ctx.act(move |m| m.remove_rule(index));
        }
    };

    view()
        .w_full()
        .p(px(12.0))
        .gap(px(8.0))
        .rounded(px(radius::PANEL))
        .bg(colors::SURFACE)
        .border_1()
        .border_color(colors::BORDER)
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(toggle(enabled_checked, enabled_toggle))
                .child(
                    input()
                        .flex_1()
                        .h(px(28.0))
                        .px(px(8.0))
                        .rounded(px(radius::CONTROL))
                        .bg(colors::RAISED)
                        .border_1()
                        .border_color(colors::BORDER)
                        .text_size(px(12.5))
                        .text_color(colors::TEXT)
                        .placeholder("规则名称")
                        .placeholder_color(color(colors::FAINT))
                        .value(name)
                        .on_change(name_change),
                )
                .child(icon_button(
                    IconName::Trash,
                    "删除规则",
                    colors::MUTED,
                    delete,
                )),
        )
        .child(match_row(ctx, rule, index))
        .child(action_row(ctx, rule, index))
        .child(action_fields(ctx, rule, index))
        .child(
            text(move || {
                let rule = rule.get();
                format!(
                    "匹配：{} · 动作：{}",
                    rule.summary(),
                    rule.action.description()
                )
            })
            .text_size(px(10.5))
            .text_color(colors::FAINT),
        )
}

fn match_row(ctx: Ctx, rule: ReadSignal<Rule>, index: ReadSignal<usize>) -> impl IntoElement {
    let ctx_method = ctx;
    let ctx_url = ctx;
    let ctx_host = ctx;
    view()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .child(
            text("匹配")
                .w(px(76.0))
                .flex_shrink_0()
                .text_size(px(11.0))
                .text_color(colors::MUTED),
        )
        .child(
            input()
                .w(px(92.0))
                .h(px(26.0))
                .px(px(7.0))
                .rounded(px(radius::CONTROL))
                .bg(colors::RAISED)
                .border_1()
                .border_color(colors::BORDER)
                .text_size(px(11.0))
                .text_color(colors::TEXT)
                .placeholder("方法")
                .placeholder_color(color(colors::FAINT))
                .value(move || rule.get().method.clone().unwrap_or_default())
                .on_change(move |value: &str| {
                    let value = if value.trim().is_empty() {
                        None
                    } else {
                        Some(value.trim().to_uppercase())
                    };
                    let index = index.get();
                    ctx_method.act(move |m| {
                        m.update_rule(index, |rule| rule.method = value);
                    });
                }),
        )
        .child(
            input()
                .flex_1()
                .h(px(26.0))
                .px(px(7.0))
                .rounded(px(radius::CONTROL))
                .bg(colors::RAISED)
                .border_1()
                .border_color(colors::BORDER)
                .text_size(px(11.0))
                .text_color(colors::TEXT)
                .placeholder("URL 包含…")
                .placeholder_color(color(colors::FAINT))
                .value(move || rule.get().url_contains.clone())
                .on_change(move |value: &str| {
                    let value = value.to_string();
                    let index = index.get();
                    ctx_url.act(move |m| {
                        m.update_rule(index, |rule| rule.url_contains = value);
                    });
                }),
        )
        .child(
            input()
                .flex_1()
                .h(px(26.0))
                .px(px(7.0))
                .rounded(px(radius::CONTROL))
                .bg(colors::RAISED)
                .border_1()
                .border_color(colors::BORDER)
                .text_size(px(11.0))
                .text_color(colors::TEXT)
                .placeholder("域名后缀，如 example.com")
                .placeholder_color(color(colors::FAINT))
                .value(move || rule.get().host_suffix.clone())
                .on_change(move |value: &str| {
                    let value = value.to_string();
                    let index = index.get();
                    ctx_host.act(move |m| {
                        m.update_rule(index, |rule| rule.host_suffix = value);
                    });
                }),
        )
}

fn action_row(ctx: Ctx, rule: ReadSignal<Rule>, index: ReadSignal<usize>) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .child(
            text("动作")
                .w(px(76.0))
                .flex_shrink_0()
                .text_size(px(11.0))
                .text_color(colors::MUTED),
        )
        .child(action_button(
            ctx,
            rule,
            index,
            ActionKind::Block,
            "阻止请求",
        ))
        .child(action_button(
            ctx,
            rule,
            index,
            ActionKind::Mock,
            "模拟响应",
        ))
        .child(action_button(
            ctx,
            rule,
            index,
            ActionKind::RequestHeader,
            "请求头",
        ))
        .child(action_button(
            ctx,
            rule,
            index,
            ActionKind::ResponseHeader,
            "响应头",
        ))
}

fn action_button(
    ctx: Ctx,
    rule: ReadSignal<Rule>,
    index: ReadSignal<usize>,
    kind: ActionKind,
    label: &'static str,
) -> impl IntoElement {
    let active_bg = { move || action_kind(&rule.get().action) == kind };
    let active_border = active_bg;
    let press = {
        move || {
            let index = index.get();
            ctx.act(move |m| {
                m.update_rule(index, |rule| rule.action = default_action(kind));
            });
        }
    };
    view()
        .h(px(26.0))
        .px(px(10.0))
        .items_center()
        .justify_center()
        .rounded(px(radius::CONTROL))
        .bg(move || {
            if active_bg() {
                colors::ACCENT_SOFT
            } else {
                colors::RAISED
            }
        })
        .border_1()
        .border_color(move || {
            if active_border() {
                colors::ACCENT
            } else {
                colors::BORDER
            }
        })
        .cursor_pointer()
        .role(AccessibilityRole::Button)
        .label(label)
        .on_press(press)
        .child(text(label).text_size(px(11.0)).text_color(colors::TEXT))
}

fn action_fields(ctx: Ctx, rule: ReadSignal<Rule>, index: ReadSignal<usize>) -> impl IntoElement {
    view().w_full().child(
        when(
            move || action_kind(&rule.get().action) == ActionKind::Block,
            move || status_field(ctx, rule, index, "状态码"),
        )
        .otherwise({
            move || {
                view().w_full().child(
                    when(
                        move || action_kind(&rule.get().action) == ActionKind::Mock,
                        move || mock_fields(ctx, rule, index),
                    )
                    .otherwise(move || header_fields(ctx, rule, index)),
                )
            }
        }),
    )
}

fn status_field(
    ctx: Ctx,
    rule: ReadSignal<Rule>,
    index: ReadSignal<usize>,
    label: &'static str,
) -> impl IntoElement {
    let ctx_change = ctx;
    view()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .child(
            text(label)
                .w(px(76.0))
                .flex_shrink_0()
                .text_size(px(11.0))
                .text_color(colors::MUTED),
        )
        .child(
            input()
                .w(px(92.0))
                .h(px(26.0))
                .px(px(7.0))
                .rounded(px(radius::CONTROL))
                .bg(colors::RAISED)
                .border_1()
                .border_color(colors::BORDER)
                .text_size(px(11.0))
                .text_color(colors::TEXT)
                .value(move || match &rule.get().action {
                    RuleAction::Block { status } | RuleAction::Mock { status, .. } => {
                        status.to_string()
                    }
                    _ => String::new(),
                })
                .on_change(move |value: &str| {
                    if let Ok(status) = value.trim().parse::<u16>() {
                        let index = index.get();
                        ctx_change.act(move |m| {
                            m.update_rule(index, |rule| match &mut rule.action {
                                RuleAction::Block { status: current }
                                | RuleAction::Mock {
                                    status: current, ..
                                } => {
                                    *current = status;
                                }
                                _ => {}
                            });
                        });
                    }
                }),
        )
}

fn mock_fields(ctx: Ctx, rule: ReadSignal<Rule>, index: ReadSignal<usize>) -> impl IntoElement {
    let ctx_status = ctx;
    let ctx_type = ctx;
    let ctx_body = ctx;
    view()
        .w_full()
        .gap(px(6.0))
        .child(status_field(ctx_status, rule, index, "状态码"))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .child(
                    text("类型")
                        .w(px(76.0))
                        .flex_shrink_0()
                        .text_size(px(11.0))
                        .text_color(colors::MUTED),
                )
                .child(
                    input()
                        .flex_1()
                        .h(px(26.0))
                        .px(px(7.0))
                        .rounded(px(radius::CONTROL))
                        .bg(colors::RAISED)
                        .border_1()
                        .border_color(colors::BORDER)
                        .text_size(px(11.0))
                        .text_color(colors::TEXT)
                        .placeholder("application/json")
                        .placeholder_color(color(colors::FAINT))
                        .value(move || match &rule.get().action {
                            RuleAction::Mock { content_type, .. } => content_type.clone(),
                            _ => String::new(),
                        })
                        .on_change(move |value: &str| {
                            let value = value.to_string();
                            let index = index.get();
                            ctx_type.act(move |m| {
                                m.update_rule(index, |rule| {
                                    if let RuleAction::Mock { content_type, .. } = &mut rule.action
                                    {
                                        *content_type = value;
                                    }
                                });
                            });
                        }),
                ),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .child(
                    text("响应体")
                        .w(px(76.0))
                        .flex_shrink_0()
                        .text_size(px(11.0))
                        .text_color(colors::MUTED),
                )
                .child(
                    input()
                        .flex_1()
                        .h(px(26.0))
                        .px(px(7.0))
                        .rounded(px(radius::CONTROL))
                        .bg(colors::RAISED)
                        .border_1()
                        .border_color(colors::BORDER)
                        .text_size(px(11.0))
                        .text_color(colors::TEXT)
                        .placeholder("响应正文")
                        .placeholder_color(color(colors::FAINT))
                        .value(move || match &rule.get().action {
                            RuleAction::Mock { body, .. } => body.clone(),
                            _ => String::new(),
                        })
                        .on_change(move |value: &str| {
                            let value = value.to_string();
                            let index = index.get();
                            ctx_body.act(move |m| {
                                m.update_rule(index, |rule| {
                                    if let RuleAction::Mock { body, .. } = &mut rule.action {
                                        *body = value;
                                    }
                                });
                            });
                        }),
                ),
        )
}

fn header_fields(ctx: Ctx, rule: ReadSignal<Rule>, index: ReadSignal<usize>) -> impl IntoElement {
    let ctx_name = ctx;
    let ctx_value = ctx;
    view()
        .w_full()
        .gap(px(6.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .child(
                    text("首部名")
                        .w(px(76.0))
                        .flex_shrink_0()
                        .text_size(px(11.0))
                        .text_color(colors::MUTED),
                )
                .child(
                    input()
                        .flex_1()
                        .h(px(26.0))
                        .px(px(7.0))
                        .rounded(px(radius::CONTROL))
                        .bg(colors::RAISED)
                        .border_1()
                        .border_color(colors::BORDER)
                        .text_size(px(11.0))
                        .text_color(colors::TEXT)
                        .placeholder("首部名称")
                        .placeholder_color(color(colors::FAINT))
                        .value(move || header_name(&rule.get().action))
                        .on_change(move |value: &str| {
                            let value = value.to_string();
                            let index = index.get();
                            ctx_name.act(move |m| {
                                m.update_rule(index, |rule| match &mut rule.action {
                                    RuleAction::AddRequestHeader { name, .. }
                                    | RuleAction::AddResponseHeader { name, .. } => {
                                        *name = value;
                                    }
                                    _ => {}
                                });
                            });
                        }),
                ),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .child(
                    text("首部值")
                        .w(px(76.0))
                        .flex_shrink_0()
                        .text_size(px(11.0))
                        .text_color(colors::MUTED),
                )
                .child(
                    input()
                        .flex_1()
                        .h(px(26.0))
                        .px(px(7.0))
                        .rounded(px(radius::CONTROL))
                        .bg(colors::RAISED)
                        .border_1()
                        .border_color(colors::BORDER)
                        .text_size(px(11.0))
                        .text_color(colors::TEXT)
                        .placeholder("首部值")
                        .placeholder_color(color(colors::FAINT))
                        .value(move || header_value(&rule.get().action))
                        .on_change(move |value: &str| {
                            let value = value.to_string();
                            let index = index.get();
                            ctx_value.act(move |m| {
                                m.update_rule(index, |rule| match &mut rule.action {
                                    RuleAction::AddRequestHeader { value: current, .. }
                                    | RuleAction::AddResponseHeader { value: current, .. } => {
                                        *current = value;
                                    }
                                    _ => {}
                                });
                            });
                        }),
                ),
        )
}

fn header_name(action: &RuleAction) -> String {
    match action {
        RuleAction::AddRequestHeader { name, .. } | RuleAction::AddResponseHeader { name, .. } => {
            name.clone()
        }
        _ => String::new(),
    }
}

fn header_value(action: &RuleAction) -> String {
    match action {
        RuleAction::AddRequestHeader { value, .. }
        | RuleAction::AddResponseHeader { value, .. } => value.clone(),
        _ => String::new(),
    }
}
