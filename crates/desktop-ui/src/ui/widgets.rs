//! 通用小部件：按钮、标签、开关、分区标题。

use std::cell::RefCell;
use std::rc::Rc;

use nsh_core::{
    element::{AccessibilityRole, AdaptiveColor, ElementRef, FontWeight},
    geometry::px,
    prelude::*,
};

use crate::model::Model;
use crate::theme::{colors, radius};
use crate::ui::icons::{IconName, icon};

#[derive(Clone, Copy)]
pub struct Ctx {
    pub model: &'static RefCell<Model>,
    pub revision: Signal<u64>,
    pub filter_ref: ElementRef,
}

impl Ctx {
    pub fn act(&self, action: impl FnOnce(&mut Model)) {
        action(&mut self.model.borrow_mut());
        self.touch();
    }

    pub fn read<T>(&self, read: impl FnOnce(&Model) -> T) -> T {
        self.revision.get();
        read(&self.model.borrow())
    }

    pub fn touch(&self) {
        self.revision
            .set(self.revision.get_untracked().wrapping_add(1));
    }
}

pub fn icon_button(
    name: IconName,
    label: &'static str,
    tint: AdaptiveColor,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    view()
        .size(px(28.0))
        .items_center()
        .justify_center()
        .rounded(px(radius::CONTROL))
        .cursor_pointer()
        .role(AccessibilityRole::Button)
        .label(label)
        .hover(|s| s.bg(colors::RAISED))
        .active(|s| s.opacity(0.55))
        .on_press(on_press)
        .child(icon(name, 16.0, tint))
}

pub fn accent_button(label: &'static str, on_press: impl Fn() + 'static) -> impl IntoElement {
    view()
        .h(px(28.0))
        .px(px(12.0))
        .items_center()
        .justify_center()
        .rounded(px(radius::CONTROL))
        .bg(colors::ACCENT)
        .cursor_pointer()
        .role(AccessibilityRole::Button)
        .label(label)
        .active(|s| s.opacity(0.75))
        .on_press(on_press)
        .child(
            text(label)
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors::ON_ACCENT),
        )
}

pub fn subtle_button(label: impl Into<String>, on_press: impl Fn() + 'static) -> impl IntoElement {
    let label: String = label.into();
    view()
        .h(px(28.0))
        .px(px(10.0))
        .items_center()
        .justify_center()
        .rounded(px(radius::CONTROL))
        .bg(colors::RAISED)
        .border_1()
        .border_color(colors::BORDER)
        .cursor_pointer()
        .role(AccessibilityRole::Button)
        .label(label.clone())
        .hover(|s| s.bg(colors::ALT))
        .active(|s| s.opacity(0.65))
        .on_press(on_press)
        .child(text(label).text_size(px(12.0)).text_color(colors::TEXT))
}

pub fn plain_text(value: impl Into<String>, size: f32) -> impl IntoElement {
    text(value.into())
        .text_size(px(size))
        .text_color(colors::TEXT)
}

pub fn muted_text(value: impl Into<String>, size: f32) -> impl IntoElement {
    text(value.into())
        .text_size(px(size))
        .text_color(colors::MUTED)
}

pub fn mono(value: impl Into<String>, size: f32) -> impl IntoElement {
    text(value.into())
        .text_size(px(size))
        .font_family(crate::theme::MONO)
        .text_color(colors::TEXT)
}

pub fn chip(label: impl Into<String>, color: AdaptiveColor) -> impl IntoElement {
    let label: String = label.into();
    view()
        .px(px(5.0))
        .py(px(1.0))
        .rounded(px(radius::CHIP))
        .bg(move || color.opacity(0.16))
        .child(
            text(label)
                .text_size(px(10.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color),
        )
}

pub fn status_dot(color: AdaptiveColor, size: f32) -> impl IntoElement {
    view()
        .size(px(size))
        .rounded(px(size / 2.0))
        .bg(color)
        .pointer_events(PointerEvents::None)
}

pub fn toggle(
    checked: impl Fn() -> bool + 'static,
    on_toggle: impl Fn() + 'static,
) -> impl IntoElement {
    let checked: Rc<dyn Fn() -> bool> = Rc::new(checked);
    let bg_checked = checked.clone();
    let knob_checked = checked.clone();
    view()
        .w(px(34.0))
        .h(px(20.0))
        .p(px(2.0))
        .rounded(px(10.0))
        .bg(move || {
            if bg_checked() {
                colors::ACCENT
            } else {
                colors::BORDER
            }
        })
        .cursor_pointer()
        .role(AccessibilityRole::Switch)
        .label("切换")
        .on_press(on_toggle)
        .child(
            view()
                .size(px(16.0))
                .rounded(px(8.0))
                .bg(colors::WHITE)
                .translate_x(move || px(if knob_checked() { 14.0 } else { 0.0 })),
        )
}

pub fn tab_button(
    label: &'static str,
    active: impl Fn() -> bool + 'static,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    let active: Rc<dyn Fn() -> bool> = Rc::new(active);
    let border_active = active.clone();
    let text_active = active.clone();
    view()
        .h_full()
        .px(px(9.0))
        .items_center()
        .justify_center()
        .cursor_pointer()
        .role(AccessibilityRole::Button)
        .label(label)
        .border_b(px(2.0))
        .border_b_color(move || {
            if border_active() {
                colors::ACCENT
            } else {
                AdaptiveColor::TRANSPARENT
            }
        })
        .on_press(on_press)
        .child(
            text(label)
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || {
                    if text_active() {
                        colors::ACCENT
                    } else {
                        colors::MUTED
                    }
                }),
        )
}

pub fn section_title(
    label: &'static str,
    count: Option<usize>,
    expanded: impl Fn() -> bool + 'static,
    on_toggle: impl Fn() + 'static,
) -> impl IntoElement {
    let expanded: Rc<dyn Fn() -> bool> = Rc::new(expanded);
    view()
        .flex_row()
        .items_center()
        .gap(px(4.0))
        .px(px(8.0))
        .h(px(26.0))
        .cursor_pointer()
        .role(AccessibilityRole::Button)
        .label(label)
        .hover(|s| s.bg(colors::RAISED))
        .on_press(on_toggle)
        .child(
            view()
                .size(px(14.0))
                .items_center()
                .justify_center()
                .rotate(move || deg(if expanded() { 0.0 } else { -90.0 }))
                .child(icon(IconName::ChevronDown, 12.0, colors::FAINT)),
        )
        .child(
            text(label)
                .flex_1()
                .text_size(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors::MUTED),
        )
        .child(match count {
            Some(count) => view()
                .child(
                    text(count.to_string())
                        .text_size(px(10.5))
                        .text_color(colors::FAINT),
                )
                .into_element(),
            None => view().into_element(),
        })
}

pub fn vertical_divider() -> impl IntoElement {
    view().w(px(1.0)).flex_1().bg(colors::BORDER)
}

pub fn horizontal_divider() -> impl IntoElement {
    view().h(px(1.0)).w_full().bg(colors::BORDER)
}
