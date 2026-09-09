//! Typography tokens for text sizing.

use gpui::{App, Pixels, px};

// =============================================================================
// Text sizes — unscaled design constants (kept for reference / non-scaled use)
// =============================================================================

/// Extra small text (10px) - badges, tags
pub const TEXT_XS: Pixels = px(10.0);

/// Small text (11px) - secondary labels, hints
pub const TEXT_SM: Pixels = px(11.0);

/// Medium-small text (12px) - compact UI, button labels
pub const TEXT_MS: Pixels = px(12.0);

/// Medium text (13px) - default body text, menu items
pub const TEXT_MD: Pixels = px(13.0);

/// Large text (15px) - section titles, panel headers
pub const TEXT_LG: Pixels = px(15.0);

/// Extra large text (18px) - headings, modal titles
pub const TEXT_XL: Pixels = px(18.0);

/// 2X Large text (22px) - hero titles, prominent modal headers
pub const TEXT_2XL: Pixels = px(22.0);

// =============================================================================
// Text sizes — scaled by ui_font_size trim x ui_scale zoom x OS scale
// =============================================================================

pub fn ui_text_xs(cx: &App) -> Pixels {
    px(10.0 * super::scale::ui_text_scale(cx))
}

pub fn ui_text_sm(cx: &App) -> Pixels {
    px(11.0 * super::scale::ui_text_scale(cx))
}

pub fn ui_text_ms(cx: &App) -> Pixels {
    px(12.0 * super::scale::ui_text_scale(cx))
}

pub fn ui_text_md(cx: &App) -> Pixels {
    px(13.0 * super::scale::ui_text_scale(cx))
}

pub fn ui_text_lg(cx: &App) -> Pixels {
    px(15.0 * super::scale::ui_text_scale(cx))
}

pub fn ui_text_xl(cx: &App) -> Pixels {
    px(18.0 * super::scale::ui_text_scale(cx))
}

pub fn ui_text_2xl(cx: &App) -> Pixels {
    px(22.0 * super::scale::ui_text_scale(cx))
}

/// Arbitrary base px scaled by the text factor (e.g. `ui_text(13.0, cx)`).
pub fn ui_text(default_px: f32, cx: &App) -> Pixels {
    px(default_px * super::scale::ui_text_scale(cx))
}

// =============================================================================
// Line heights — standard compact leading scale (1.35x - 1.45x)
// =============================================================================

/// Extra small text line height (14px) - badges, micro tags
pub const LINE_HEIGHT_XS: Pixels = px(14.0);

/// Small text line height (16px) - secondary labels, hints
pub const LINE_HEIGHT_SM: Pixels = px(16.0);

/// Medium text line height (18px) - default body text, menu items
pub const LINE_HEIGHT_MD: Pixels = px(18.0);

/// Large text line height (20px) - section titles, panel headers
pub const LINE_HEIGHT_LG: Pixels = px(20.0);

/// Extra large text line height (24px) - headings, modal titles
pub const LINE_HEIGHT_XL: Pixels = px(24.0);

pub fn ui_line_height_xs(cx: &App) -> Pixels {
    px(14.0 * super::scale::ui_text_scale(cx))
}

pub fn ui_line_height_sm(cx: &App) -> Pixels {
    px(16.0 * super::scale::ui_text_scale(cx))
}

pub fn ui_line_height_md(cx: &App) -> Pixels {
    px(18.0 * super::scale::ui_text_scale(cx))
}

pub fn ui_line_height_lg(cx: &App) -> Pixels {
    px(20.0 * super::scale::ui_text_scale(cx))
}

pub fn ui_line_height_xl(cx: &App) -> Pixels {
    px(24.0 * super::scale::ui_text_scale(cx))
}

