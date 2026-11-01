//! Proxyman 风格的配色与密度。
//!
//! 所有颜色都是 [`AdaptiveColor`]，深浅色由平台原生切换，不触发重绘。

use desktop::storage::Highlight;
use nsh_core::element::{AdaptiveColor, Rgba, rgb, rgba};

/// `#rrggbb` + alpha（CSS 的 `rgba(r, g, b, a)`）。
pub const fn rgb_a(hex: u32, alpha: f32) -> Rgba {
    rgba(hex << 8 | (alpha * 255.0 + 0.5) as u32)
}

const fn tone(light: Rgba, dark: Rgba) -> AdaptiveColor {
    AdaptiveColor {
        light: light.to_hsla(),
        dark: dark.to_hsla(),
    }
}

pub mod colors {
    use super::{AdaptiveColor, rgb, tone};

    /// 窗口底色。
    pub const WINDOW: AdaptiveColor = tone(rgb(0xffffff), rgb(0x1b1b1b));
    /// 工具栏 / 检查器底板。
    pub const CHROME: AdaptiveColor = tone(rgb(0xfbfbfd), rgb(0x1e1e1e));
    /// 侧边栏（浅色略暖）。
    pub const SIDEBAR: AdaptiveColor = tone(rgb(0xfbf9f7), rgb(0x1b1b1b));
    /// 表格与内容区。
    pub const SURFACE: AdaptiveColor = tone(rgb(0xffffff), rgb(0x1e1e1e));
    /// 表头、分区标题、悬停底色。
    pub const RAISED: AdaptiveColor = tone(rgb(0xf5f5f5), rgb(0x2d2d2d));
    /// 隔行底色。
    pub const ALT: AdaptiveColor = tone(rgb(0xf5f5f5), rgb(0x292929));
    /// 分隔线。
    pub const BORDER: AdaptiveColor = tone(rgb(0xe3e3e3), rgb(0x424242));
    /// 更淡的内部分隔线。
    pub const LINE: AdaptiveColor = tone(rgb(0xededed), rgb(0x323232));
    /// 正文。
    pub const TEXT: AdaptiveColor = tone(rgb(0x262626), rgb(0xdcdcdc));
    /// 次要文字。
    pub const MUTED: AdaptiveColor = tone(rgb(0x818181), rgb(0x9a9a9a));
    /// 更安静的文字（列标题、序号）。
    pub const FAINT: AdaptiveColor = tone(rgb(0x9a9a9a), rgb(0x6e6e6e));

    /// Proxyman 的橙色强调。
    pub const ACCENT: AdaptiveColor = tone(rgb(0xf5822f), rgb(0xf5822f));
    /// 选中行 / 选中侧边栏。
    pub const SELECTION: AdaptiveColor = tone(rgb(0xca712b), rgb(0xbb6625));
    /// 选中行上的文字。
    pub const ON_ACCENT: AdaptiveColor = tone(rgb(0xffffff), rgb(0xffffff));
    /// 橙色浅底（筛选标签、状态胶囊）。
    pub const ACCENT_SOFT: AdaptiveColor =
        tone(super::rgb_a(0xf5822f, 0.14), super::rgb_a(0xf5822f, 0.18));
    /// 主按钮 / 链接蓝色。
    pub const BLUE: AdaptiveColor = tone(rgb(0x0a66c2), rgb(0x3b9bff));

    pub const GREEN: AdaptiveColor = tone(rgb(0x22b24c), rgb(0x32d74b));
    pub const RED: AdaptiveColor = tone(rgb(0xe5484d), rgb(0xff5f57));
    pub const ORANGE: AdaptiveColor = tone(rgb(0xe8912d), rgb(0xffa726));
    pub const PURPLE: AdaptiveColor = tone(rgb(0x8e5bd9), rgb(0xc084ff));
    pub const TEAL: AdaptiveColor = tone(rgb(0x2a9d8f), rgb(0x40c8b0));
    pub const GRAY: AdaptiveColor = tone(rgb(0x8e8e93), rgb(0x8e8e93));
    pub const WHITE: AdaptiveColor = tone(rgb(0xffffff), rgb(0xffffff));
}

pub mod radius {
    pub const CONTROL: f32 = 6.0;
    pub const CHIP: f32 = 4.0;
    pub const PANEL: f32 = 8.0;
}

pub mod size {
    pub const TOOLBAR_HEIGHT: f32 = 48.0;
    pub const TAB_HEIGHT: f32 = 34.0;
    pub const TABLE_HEADER: f32 = 28.0;
    pub const ROW: f32 = 26.0;
    pub const SIDEBAR_WIDTH: f32 = 248.0;
    pub const INSPECTOR_HEADER: f32 = 30.0;
    pub const STATUS_BAR: f32 = 30.0;
}

pub const MONO: &str = "SF Mono";

/// 高亮标记的颜色；`None` 表示没有标记。
pub fn highlight_color(highlight: Highlight) -> Option<AdaptiveColor> {
    Some(match highlight {
        Highlight::None => return None,
        Highlight::Red => colors::RED,
        Highlight::Orange => colors::ORANGE,
        Highlight::Yellow => color_tone(0xd4a017, 0xffd60a),
        Highlight::Green => colors::GREEN,
        Highlight::Blue => colors::BLUE,
        Highlight::Purple => colors::PURPLE,
        Highlight::Gray => colors::GRAY,
    })
}

const fn color_tone(light: u32, dark: u32) -> AdaptiveColor {
    AdaptiveColor::rgb(light, dark)
}
pub const SANS: &str = "Helvetica Neue";
