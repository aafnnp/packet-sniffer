//! Packet Sniffer 的 NSH 原生界面。
//!
//! 整个界面是一个 Rust 组件：引擎事件在后台运行时里产生，界面用 120ms 的
//! 定时器搬运事件并递增 `revision`，所有文本与样式绑定都读取它，因此只更新
//! 变化的部分。

#![allow(clippy::too_many_arguments)]

pub mod format;
pub mod model;
pub mod theme;
pub mod ui;

use std::cell::RefCell;
use std::time::Duration;

use desktop::state::{Settings, default_settings_path};
use nsh_core::{geometry::px, prelude::*};

use crate::model::{Model, View};
use crate::theme::colors;
use crate::ui::chrome::{error_banner, notice_banner, sidebar, status_bar, toolbar};
use crate::ui::inspector::inspector;
use crate::ui::rules::rules_page;
use crate::ui::settings::settings_page;
use crate::ui::table::request_table;
use crate::ui::widgets::Ctx;

#[component]
pub fn App() -> impl IntoElement {
    let settings = Settings::load(&default_settings_path());
    // 应用级单例：界面组件只运行一次，模型与进程同寿命。
    let model: &'static RefCell<Model> = Box::leak(Box::new(RefCell::new(Model::new(settings))));
    let revision = signal_with(0_u64, SignalEquals::Never);
    let filter_ref = element_ref();

    model.borrow_mut().start();

    set_interval(Duration::from_millis(120), move || {
        let changed = model.borrow_mut().poll();
        if changed {
            revision.set(revision.get_untracked().wrapping_add(1));
        }
    });
    cleanup(move || model.borrow_mut().shutdown());

    let ctx = Ctx {
        model,
        revision,
        filter_ref,
    };

    let key_filter = ctx;
    on_key("mod+f", move |_| key_filter.filter_ref.focus());
    let key_clear = ctx;
    on_key("mod+k", move |_| key_clear.act(|m| m.clear()));

    view()
        .flex_1()
        .w_full()
        .h_full()
        .flex_col()
        .bg(colors::WINDOW)
        .child(toolbar(ctx))
        .child(error_banner(ctx))
        .child(notice_banner(ctx))
        .child(
            when(move || ctx.read(|m| m.view == View::Settings), {
                move || settings_page(ctx)
            })
            .otherwise(move || {
                when(
                    move || ctx.read(|m| m.view == View::Rules),
                    move || rules_page(ctx),
                )
                .otherwise(move || traffic(ctx))
            }),
        )
        .child(status_bar(ctx))
}

fn traffic(ctx: Ctx) -> impl IntoElement {
    view()
        .flex_1()
        .w_full()
        .min_h(px(0.0))
        .flex_row()
        .overflow_hidden()
        .child(sidebar(ctx))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .min_h(px(0.0))
                .h_full()
                .flex_col()
                .overflow_hidden()
                .child(request_table(ctx))
                .child(inspector(ctx)),
        )
}
