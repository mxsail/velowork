//! Control appearance resolver.
//!
//! `ControlAppearance` is an **immutable, pre-resolved snapshot** of every
//! visual property a control needs to render: geometry (height, padding, gap,
//! radius), typography (font size, line height, weight), and color (bg, hover,
//! text, border). It replaces ad-hoc per-widget size/color resolution with a
//! single deterministic function of `(ControlSize, ControlVariant, palette,
//! density, scale)`.
///
// Compact {
//         font: 12,
//         line_height: 16,
//         icon: 14,
//         row: 26,
//     },

//     Default {
//         font: 13,
//         line_height: 18,
//         icon: 16,
//         row: 30,
//     },

//     Large {
//         font: 15,
//         line_height: 22,
//         icon: 20,
//         row: 38,
//     }
use super::density::UiDensity;
use super::semantic::SemanticPalette;
use gpui::{App, FontWeight, Hsla, Pixels, px};

/// Logical control size tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ControlSize {
    /// 24 px base — tree items, dense tables, chips.
    Compact,
    /// 30 px base — standard buttons, inputs.
    #[default]
    Default,
    /// 40 px base — hero actions, quick-pick input.
    Large,
}

impl ControlSize {
    /// Base height in logical pixels (before density offset + scale).
    fn base_height(self) -> f32 {
        match self {
            ControlSize::Compact => 24.0,
            ControlSize::Default => 28.0,
            ControlSize::Large => 36.0,
        }
    }

    /// Font size for this control tier.
    fn font_size(self) -> f32 {
        match self {
            ControlSize::Compact => 11.5,
            ControlSize::Default => 12.5,
            ControlSize::Large => 14.0,
        }
    }

    /// Icon size for this control tier.
    pub fn icon_size(self) -> f32 {
        match self {
            ControlSize::Compact => 13.0,
            ControlSize::Default => 15.0,
            ControlSize::Large => 18.0,
        }
    }
}

/// Semantic control variant (visual intent).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ControlVariant {
    /// Main call-to-action: accent bg, on-accent text.
    Primary,
    /// Secondary action: raised bg, primary text, subtle border.
    #[default]
    Secondary,
    /// Destructive action: danger bg, on-accent text.
    Danger,
    /// Ghost / text button: transparent bg until hover.
    Ghost,
    /// Outlined button: transparent bg, subtle border.
    Outline,
    /// Inline link / text action: transparent bg, accent text, underline on hover.
    Link,
}

/// Fully resolved visual appearance for a control.
///
/// Construct via [`ControlAppearance::resolve`]. This struct is `Copy` so it
/// can be cheaply passed around in render functions.
#[derive(Debug, Clone, Copy)]
pub struct ControlAppearance {
    pub height: Pixels,
    pub padding_x: Pixels,
    pub padding_y: Pixels,
    pub gap: Pixels,
    pub radius: Pixels,
    pub font_size: Pixels,
    pub line_height: Pixels,
    pub font_weight: FontWeight,
    pub bg: Hsla,
    pub bg_hover: Hsla,
    pub text_color: Hsla,
    pub border_color: Hsla,
    pub icon_size: Pixels,
}

impl ControlAppearance {
    /// Resolve the complete visual appearance for the given parameters.
    ///
    /// * `size` — logical tier (Compact / Default / Large).
    /// * `variant` — semantic intent (Primary / Secondary / …).
    /// * `palette` — semantic color palette from the active theme.
    /// * `density` — UI density setting.
    /// * `scale` — global UI zoom factor (1.0 = 100 %).
    pub fn resolve(
        size: ControlSize,
        variant: ControlVariant,
        palette: &SemanticPalette,
        density: UiDensity,
        scale: f32,
    ) -> Self {
        let height = (size.base_height() + density.height_offset()) * scale;

        let (bg, bg_hover, text_color, border_color, weight) = match variant {
            ControlVariant::Primary => (
                palette.surface_accent,
                palette.surface_accent_hover,
                palette.text_on_accent,
                palette.border_transparent,
                FontWeight::SEMIBOLD,
            ),
            ControlVariant::Secondary => (
                palette.surface_card,
                palette.surface_raised,
                palette.text_primary,
                palette.border_subtle,
                FontWeight::MEDIUM,
            ),
            ControlVariant::Danger => (
                palette.surface_danger,
                palette.surface_danger, // danger stays the same on hover (opacity change)
                palette.text_on_accent,
                palette.border_transparent,
                FontWeight::SEMIBOLD,
            ),
            ControlVariant::Ghost => (
                palette.border_transparent,
                palette.surface_card,
                palette.text_primary,
                palette.border_transparent,
                FontWeight::MEDIUM,
            ),
            ControlVariant::Outline => (
                palette.border_transparent,
                palette.surface_card,
                palette.text_primary,
                palette.border_subtle,
                FontWeight::MEDIUM,
            ),
            ControlVariant::Link => (
                palette.border_transparent,
                palette.surface_accent_hover,
                palette.surface_accent,
                palette.border_transparent,
                FontWeight::MEDIUM,
            ),
        };

        let padding_x = match size {
            ControlSize::Compact => 8.0,
            ControlSize::Default => 12.0,
            ControlSize::Large => 16.0,
        };

        Self {
            height: px(height),
            padding_x: px(padding_x * scale),
            padding_y: px(2.0 * scale),
            gap: px(5.0 * scale),
            radius: crate::tokens::RADIUS_MD,
            font_size: px(size.font_size() * scale),
            line_height: px((size.font_size() + 4.0) * scale),
            font_weight: weight,
            bg,
            bg_hover,
            text_color,
            border_color,
            icon_size: px(size.icon_size() * scale),
        }
    }

    /// Resolve visual appearance dynamically driven by current UI settings
    /// (`ui_density`, `ui_font_size`, `ui_scale`).
    pub fn resolve_from_settings(
        size: ControlSize,
        variant: ControlVariant,
        palette: &SemanticPalette,
        ui_density: UiDensity,
        ui_font_size: f32,
        ui_scale: f32,
    ) -> Self {
        let base_font_size_ratio = (ui_font_size / 13.0).clamp(0.6, 2.5);
        let scale = ((ui_scale / 100.0) * base_font_size_ratio).clamp(0.5, 3.0);
        Self::resolve(size, variant, palette, ui_density, scale)
    }
}

/// 树形/列表控件的统一行高（纯几何，与主题颜色无关）。
///
/// 供会话树、隧道树、快捷指令树、SFTP 文件列表等所有「树状/列表行」共用，
/// 确保行高完全一致，并随「界面密度」(`ui_density`) 与全局 UI 缩放
/// (`ui_text_scale`) 实时变化。
///
/// 基于 `ControlSize::Default` + `ControlVariant::Ghost` 的几何值。颜色使用
/// `dark_default` 调色板，因为行高只取决于尺寸/密度/缩放，与具体主题配色无关，
/// 这与会话树 `session_row_height` 等历史实现保持一致。
pub fn tree_row_height(cx: &App) -> Pixels {
    ControlAppearance::resolve(
        ControlSize::Default,
        ControlVariant::Ghost,
        &SemanticPalette::dark_default(),
        crate::tokens::get_ui_density(cx),
        crate::tokens::ui_text_scale(cx),
    )
    .height
}

/// 统一输入控件（Input, Select, Stepper, Toggle）的响应式默认高度。
///
/// 几何计算基于 [`ControlAppearance`] 标准级（Default），自动随应用
/// 界面密度 (`ui_density`) 及全局 UI 缩放 (`ui_text_scale`) 响应缩变。
pub fn control_height(cx: &App) -> Pixels {
    control_height_for_size(ControlSize::Default, cx)
}

/// 指定尺寸等级（Compact / Default / Large）的统一控件高度。
pub fn control_height_for_size(size: ControlSize, cx: &App) -> Pixels {
    ControlAppearance::resolve(
        size,
        ControlVariant::Secondary,
        &SemanticPalette::dark_default(),
        crate::tokens::get_ui_density(cx),
        crate::tokens::ui_text_scale(cx),
    )
    .height
}

/// 统一控制组件的行高（文字行高），与 [`ControlAppearance::line_height`] 一致。
///
/// 供 Toggle 等需与相邻文字「行高」对齐的控件使用，避免控件高度明显大于
/// 周围文字而看起来过大。
pub fn control_line_height(cx: &App) -> Pixels {
    ControlAppearance::resolve(
        ControlSize::Default,
        ControlVariant::Secondary,
        &SemanticPalette::dark_default(),
        crate::tokens::get_ui_density(cx),
        crate::tokens::ui_text_scale(cx),
    )
    .line_height
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::semantic::SemanticPalette;

    #[test]
    fn test_default_size_default_variant() {
        let palette = SemanticPalette::dark_default();
        let a = ControlAppearance::resolve(
            ControlSize::Default,
            ControlVariant::Secondary,
            &palette,
            UiDensity::Default,
            1.0,
        );
        assert_eq!(a.height, px(28.0));
        assert_eq!(a.font_size, px(12.5));
    }

    #[test]
    fn test_compact_density_reduces_height() {
        let palette = SemanticPalette::dark_default();
        let a = ControlAppearance::resolve(
            ControlSize::Default,
            ControlVariant::Secondary,
            &palette,
            UiDensity::Compact,
            1.0,
        );
        assert_eq!(a.height, px(24.0)); // 28 - 4
    }

    #[test]
    fn test_scale_factor() {
        let palette = SemanticPalette::dark_default();
        let a = ControlAppearance::resolve(
            ControlSize::Default,
            ControlVariant::Secondary,
            &palette,
            UiDensity::Default,
            1.5,
        );
        assert_eq!(a.height, px(42.0)); // 28 * 1.5
        assert_eq!(a.font_size, px(18.75)); // 12.5 * 1.5
    }

    #[test]
    fn test_primary_variant_uses_accent() {
        let palette = SemanticPalette::dark_default();
        let a = ControlAppearance::resolve(
            ControlSize::Default,
            ControlVariant::Primary,
            &palette,
            UiDensity::Default,
            1.0,
        );
        assert_eq!(a.bg, palette.surface_accent);
        assert_eq!(a.text_color, palette.text_on_accent);
        assert_eq!(a.font_weight, FontWeight::SEMIBOLD);
    }

    #[test]
    fn test_ghost_variant_transparent_bg() {
        let palette = SemanticPalette::dark_default();
        let a = ControlAppearance::resolve(
            ControlSize::Default,
            ControlVariant::Ghost,
            &palette,
            UiDensity::Default,
            1.0,
        );
        assert_eq!(a.bg.a, 0.0); // transparent
        assert_eq!(a.border_color.a, 0.0); // transparent
    }
}
