//! Icon size tokens.

use gpui::{px, App, Pixels};

// =============================================================================
// Icon sizes — unscaled design constants (resolution-independent)
// =============================================================================

/// Small icon (10px) - inline icons, chevrons
pub const ICON_SM: Pixels = px(10.0);

/// Micro icon (12px) - tree node collapse chevrons
pub const ICON_MICRO: Pixels = px(12.0);

/// Standard icon (14px) - inline buttons
pub const ICON_STD: Pixels = px(14.0);

/// Medium icon (16px) - context menus, control icons
pub const ICON_MD: Pixels = px(16.0);

/// Large icon (20px) - activity bar / main navigation
pub const ICON_LG: Pixels = px(20.0);

// =============================================================================
// Icon sizes — scaled by the global UI zoom
// =============================================================================

pub fn ui_icon_sm(cx: &App) -> Pixels {
    px(10.0 * super::scale::ui_zoom_factor(cx))
}
pub fn ui_icon_micro(cx: &App) -> Pixels {
    px(12.0 * super::scale::ui_zoom_factor(cx))
}
pub fn ui_icon_std(cx: &App) -> Pixels {
    px(14.0 * super::scale::ui_zoom_factor(cx))
}
pub fn ui_icon_md(cx: &App) -> Pixels {
    px(16.0 * super::scale::ui_zoom_factor(cx))
}
pub fn ui_icon_lg(cx: &App) -> Pixels {
    px(20.0 * super::scale::ui_zoom_factor(cx))
}

/// Icon size that tracks the *text* scaling pipe (`ui_text_scale`), so icons stay
/// in a fixed ratio to surrounding text even when the user adjusts the font-size
/// trim (`ui_font_size`). Use for icons sitting inline with labels.
pub fn ui_icon_std_ts(cx: &App) -> Pixels {
    px(14.0 * super::scale::ui_text_scale(cx))
}

/// Medium icon variant that also tracks `ui_text_scale` (see [`ui_icon_std_ts`]).
pub fn ui_icon_md_ts(cx: &App) -> Pixels {
    px(16.0 * super::scale::ui_text_scale(cx))
}
