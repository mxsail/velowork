//! Unified application icon management.
//!
//! The icon *catalog* is the `AppIcon` enum in `icon_gen.rs` — a GENERATED
//! file produced by `scripts/gen_icons.py` from the SVGs under `assets/icons/`.
//! Every variant maps to a standalone, compile-time-embedded SVG.
//!
//! Design contract:
//! - **Style unification**: every SVG is normalized to a 24×24 grid,
//!   `stroke="currentColor"`, `fill="none"`, `stroke-width="2"`,
//!   no hard-coded `width`/`height`.
//! - **Theme adaptation**: glyph strokes use `currentColor`, so the
//!   rendered color follows the element's effective `text_color` — by
//!   default inherited from the parent container, never hard-coded in
//!   the asset.
//! - **DPI adaptation**: SVG is vector; GPUI scales it by the window's
//!   scale factor, so a single asset stays crisp on any display.
//! - **Zero-cost lookup**: `AppIcon` is a `Copy` enum; `path()` /
//!   `svg_source()` resolve through static `match` arms to
//!   `&'static str` (no runtime maps, no allocation).
//!
//! Usage:
//! ```ignore
//! // Default design-system size, inherits parent text_color:
//! div().child(AppIcon::Server)
//!
//! // Chained semantic size:
//! div().child(AppIcon::Folder.size(IconSize::Small))
//!
//! // Custom size / color via the builder:
//! AppIcon::Terminal.size(px(18.0)).text_color(rgb(t.accent))
//!
//! // Default size + color shorthand:
//! AppIcon::Server.color(rgb(t.text_muted))
//! ```

use gpui::*;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::design::appearance::ControlSize;
use crate::tokens::{ICON_LG, ICON_MICRO, ICON_STD};

mod icon_gen;
pub use icon_gen::*;

mod folder_icon;
pub use folder_icon::*;

/// 图标尺寸语义 Role（与 Design Tokens 协同）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconSize {
    /// 12px - 树节点折叠 Chevron
    Micro,
    /// 14px - Inline 菜单, 状态栏
    Small,
    /// 继承 Control 默认 (14px) - 标准按钮, 工具栏
    Default,
    /// 20px - 活动栏 Main Nav
    Large,
    /// 自定义像素
    Custom(u32),
}

impl IconSize {
    pub fn to_pixels(self) -> Pixels {
        match self {
            IconSize::Micro => ICON_MICRO,
            IconSize::Small => ICON_STD,
            IconSize::Default => px(ControlSize::default().icon_size()),
            IconSize::Large => ICON_LG,
            IconSize::Custom(p) => px(p as f32),
        }
    }
}

impl From<IconSize> for Pixels {
    #[inline(always)]
    fn from(size: IconSize) -> Self {
        size.to_pixels()
    }
}

/// Create a `gpui::Svg` element for `app_icon`, pre-sized to the
/// design-system default icon size (`ControlSize::Default`). Callers may
/// override with `.size(..)` and/or set `.text_color(..)`; when no color
/// is set, the glyph inherits the parent's `text_color` (SVGs use
/// `currentColor`).
pub fn icon(app_icon: AppIcon) -> Svg {
    app_icon.svg()
}

/// Create a `gpui::Svg` element for `app_icon` with an explicit size
/// (accepts `Pixels`, `IconSize`, or anything `Into<Pixels>`).
pub fn icon_sized(app_icon: AppIcon, size: impl Into<Pixels>) -> Svg {
    app_icon.svg().size(size.into())
}

/// Chainable element constructors on the icon catalog itself.
impl AppIcon {
    /// Convert to a `gpui::Svg` element at the design-system default size.
    #[inline(always)]
    pub fn svg(self) -> Svg {
        svg().path(self.path()).size(IconSize::Default.to_pixels())
    }

    /// Convert to a `gpui::Svg` element with an explicit size
    /// (`Pixels`, `IconSize`, `f32`, …).
    #[inline(always)]
    pub fn size(self, size: impl Into<Pixels>) -> Svg {
        svg().path(self.path()).size(size.into())
    }

    /// Convert to a `gpui::Svg` element at the default size with the given
    /// color: `AppIcon::Server.color(rgb(t.text_muted))`.
    #[inline(always)]
    pub fn color(self, color: impl Into<Hsla>) -> Svg {
        self.svg().text_color(color.into())
    }
}

/// `AppIcon` renders directly as an element:
/// `div().child(AppIcon::Server)` — default size, inherited color.
impl IntoElement for AppIcon {
    type Element = Svg;

    #[inline(always)]
    fn into_element(self) -> Self::Element {
        self.svg()
    }
}

impl From<AppIcon> for SharedString {
    #[inline(always)]
    fn from(icon: AppIcon) -> Self {
        SharedString::from(icon.path())
    }
}

/// Fallback icon for missing / unrecognized values (e.g. on deserialization
/// of legacy persisted data).
impl Default for AppIcon {
    #[inline(always)]
    fn default() -> Self {
        AppIcon::Folder
    }
}

impl Serialize for AppIcon {
    #[inline(always)]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

impl<'de> Deserialize<'de> for AppIcon {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        if raw.is_empty() {
            return Ok(AppIcon::default());
        }
        // Accept either the kebab-case name ("monitor") or a full asset path
        // ("icons/monitor.svg") so both legacy and current persistence formats load.
        let name = raw
            .strip_prefix("icons/")
            .and_then(|s| s.strip_suffix(".svg"))
            .unwrap_or(&raw);
        Ok(AppIcon::from_str(name).unwrap_or_else(AppIcon::default))
    }
}
