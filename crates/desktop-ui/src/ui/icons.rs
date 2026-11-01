//! 线性图标：在 20 × 20 的框里用 `canvas` 绘制，随外观切换颜色。

use std::f32::consts::TAU;

use nsh_core::{
    Bounds,
    canvas::{LineCap, LineJoin},
    element::AdaptiveColor,
    geometry::{point, px, size},
    prelude::*,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IconName {
    #[default]
    ChevronDown,
    ChevronRight,
    ChevronLeft,
    Play,
    Pause,
    Filter,
    Trash,
    Gear,
    Search,
    Globe,
    App,
    Pin,
    Error,
    Close,
    Check,
    Info,
    Copy,
    Export,
    Sort,
    Sidebar,
    Shield,
    Clock,
    Rule,
    More,
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> Bounds<nsh_core::Points> {
    Bounds::new(point(px(x), px(y)), size(px(w), px(h)))
}

fn line(p: Painter, points: &[f32]) -> Painter {
    let mut p = p.begin_path().move_to(points[0], points[1]);
    for xy in points[2..].chunks(2) {
        p = p.line_to(xy[0], xy[1]);
    }
    p.stroke()
}

fn rounded(p: Painter, x: f32, y: f32, w: f32, h: f32, r: f32) -> Painter {
    p.begin_path().round_rect(rect(x, y, w, h), px(r)).stroke()
}

fn circle(p: Painter, x: f32, y: f32, r: f32, fill: bool) -> Painter {
    let p = p.begin_path().arc(x, y, r, 0.0, TAU);
    if fill { p.fill() } else { p.stroke() }
}

fn draw(name: IconName, c: Painter) -> Painter {
    match name {
        IconName::ChevronDown => line(c, &[5.0, 8.0, 10.0, 13.0, 15.0, 8.0]),
        IconName::ChevronRight => line(c, &[8.0, 5.0, 13.0, 10.0, 8.0, 15.0]),
        IconName::ChevronLeft => line(c, &[13.0, 5.0, 8.0, 10.0, 13.0, 15.0]),
        IconName::Play => {
            let c = c
                .begin_path()
                .move_to(7.0, 4.5)
                .line_to(16.0, 10.0)
                .line_to(7.0, 15.5);
            c.close_path().fill()
        }
        IconName::Pause => {
            let c = c
                .begin_path()
                .round_rect(rect(6.0, 4.5, 3.0, 11.0), px(1.0))
                .fill();
            c.begin_path()
                .round_rect(rect(11.0, 4.5, 3.0, 11.0), px(1.0))
                .fill()
        }
        IconName::Filter => line(
            c,
            &[
                3.0, 5.0, 17.0, 5.0, 11.5, 11.0, 11.5, 16.0, 8.5, 14.0, 8.5, 11.0, 3.0, 5.0,
            ],
        ),
        IconName::Trash => {
            let c = line(c, &[4.0, 6.0, 16.0, 6.0]);
            let c = line(c, &[6.0, 6.0, 6.8, 16.0, 13.2, 16.0, 14.0, 6.0]);
            let c = line(c, &[8.0, 6.0, 8.5, 3.5, 11.5, 3.5, 12.0, 6.0]);
            line(c, &[8.3, 9.0, 8.6, 13.5])
        }
        IconName::Gear => {
            let c = line(c, &[3.5, 6.0, 16.5, 6.0]);
            let c = line(c, &[3.5, 14.0, 16.5, 14.0]);
            let c = circle(c, 7.5, 6.0, 1.8, true);
            circle(c, 12.5, 14.0, 1.8, true)
        }
        IconName::Search => {
            let c = circle(c, 9.0, 9.0, 5.0, false);
            line(c, &[12.8, 12.8, 17.0, 17.0])
        }
        IconName::Globe => {
            let c = circle(c, 10.0, 10.0, 7.0, false);
            let c = line(c, &[3.0, 10.0, 17.0, 10.0]);
            line(
                c,
                &[
                    10.0, 3.0, 7.5, 6.0, 7.0, 10.0, 7.5, 14.0, 10.0, 17.0, 12.5, 14.0, 13.0, 10.0,
                    12.5, 6.0, 10.0, 3.0,
                ],
            )
        }
        IconName::App => {
            let c = rounded(c, 3.0, 3.5, 14.0, 13.0, 2.5);
            line(c, &[3.0, 7.0, 17.0, 7.0])
        }
        IconName::Pin => {
            let c = line(c, &[10.0, 3.0, 10.0, 12.0, 7.0, 9.0]);
            line(
                c,
                &[
                    10.0, 3.0, 13.0, 6.0, 12.0, 12.0, 8.0, 12.0, 7.0, 6.0, 10.0, 3.0,
                ],
            )
        }
        IconName::Error => {
            let c = circle(c, 10.0, 10.0, 7.0, false);
            let c = line(c, &[10.0, 6.0, 10.0, 11.0]);
            line(c, &[10.0, 14.0, 10.0, 14.1])
        }
        IconName::Close => line(c, &[5.5, 5.5, 14.5, 14.5, 14.5, 5.5, 5.5, 14.5]),
        IconName::Check => line(c, &[4.5, 10.5, 8.5, 14.5, 15.5, 6.0]),
        IconName::Info => {
            let c = circle(c, 10.0, 10.0, 7.0, false);
            let c = line(c, &[10.0, 9.0, 10.0, 14.0]);
            line(c, &[10.0, 6.0, 10.0, 6.1])
        }
        IconName::Copy => {
            let c = rounded(c, 7.0, 7.0, 9.0, 10.0, 2.0);
            rounded(c, 4.0, 3.5, 9.0, 10.0, 2.0)
        }
        IconName::Export => {
            let c = line(c, &[10.0, 12.0, 10.0, 3.5]);
            let c = line(c, &[6.5, 7.0, 10.0, 3.5, 13.5, 7.0]);
            line(c, &[4.0, 11.0, 4.0, 16.0, 16.0, 16.0, 16.0, 11.0])
        }
        IconName::Sort => line(
            c,
            &[
                4.0, 6.0, 16.0, 6.0, 7.0, 10.0, 13.0, 10.0, 10.0, 14.0, 10.0, 14.0,
            ],
        ),
        IconName::Sidebar => {
            let c = rounded(c, 3.0, 4.0, 14.0, 12.0, 2.0);
            line(c, &[8.0, 4.0, 8.0, 16.0])
        }
        IconName::Shield => {
            let c = line(
                c,
                &[
                    10.0, 3.0, 16.0, 5.5, 16.0, 10.0, 13.5, 14.5, 10.0, 17.0, 6.5, 14.5, 4.0, 10.0,
                    4.0, 5.5, 10.0, 3.0,
                ],
            );
            line(c, &[7.0, 10.0, 9.0, 12.0, 13.0, 8.0])
        }
        IconName::Clock => {
            let c = circle(c, 10.0, 10.0, 7.0, false);
            line(c, &[10.0, 5.5, 10.0, 10.0, 13.0, 12.0])
        }
        IconName::Rule => {
            let c = line(c, &[3.0, 6.0, 8.0, 6.0, 8.0, 14.0, 17.0, 14.0]);
            line(c, &[3.0, 14.0, 5.0, 14.0])
        }
        IconName::More => {
            let c = circle(c, 4.5, 10.0, 1.3, true);
            let c = circle(c, 10.0, 10.0, 1.3, true);
            circle(c, 15.5, 10.0, 1.3, true)
        }
    }
}

/// 一个 `size` × `size` 的图标，颜色随绑定变化。
pub fn icon_reactive(
    name: IconName,
    size: f32,
    tint: impl Fn() -> AdaptiveColor + 'static,
) -> impl IntoElement {
    view()
        .size(px(size))
        .pointer_events(PointerEvents::None)
        .child(
            canvas(move || {
                let stroke = color(tint());
                let scale = size / 20.0;
                let painter = Painter::new()
                    .scale(scale, scale)
                    .stroke_color(stroke)
                    .fill_color(stroke)
                    .line_width(1.6)
                    .line_cap(LineCap::Round)
                    .line_join(LineJoin::Round);
                draw(name, painter)
            })
            .size_full(),
        )
}

/// 一个 `size` × `size` 的图标。
pub fn icon(name: IconName, size: f32, tint: AdaptiveColor) -> impl IntoElement {
    view()
        .size(px(size))
        .pointer_events(PointerEvents::None)
        .child(
            canvas(move || {
                let stroke = color(tint);
                let scale = size / 20.0;
                let painter = Painter::new()
                    .scale(scale, scale)
                    .stroke_color(stroke)
                    .fill_color(stroke)
                    .line_width(1.6)
                    .line_cap(LineCap::Round)
                    .line_join(LineJoin::Round);
                draw(name, painter)
            })
            .size_full(),
        )
}
