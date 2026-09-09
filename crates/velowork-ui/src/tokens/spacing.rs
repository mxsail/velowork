//! Spacing tokens for padding, margin, and gap.

use gpui::{px, App, Pixels};

// =============================================================================
// Spacing — unscaled design constants
// =============================================================================

/// Double extra small spacing (2px) - minimal gaps, dense padding
pub const SPACE_2XS: Pixels = px(2.0);

/// Extra small spacing (4px) - tight gaps, small padding
pub const SPACE_XS: Pixels = px(4.0);

/// Small spacing (6px) - compact padding
pub const SPACE_SM: Pixels = px(6.0);

/// Medium spacing (8px) - standard gaps
pub const SPACE_MD: Pixels = px(8.0);

/// Large spacing (12px) - section padding, larger gaps
pub const SPACE_LG: Pixels = px(12.0);

/// Extra large spacing (16px) - modal/dialog padding
pub const SPACE_XL: Pixels = px(16.0);

/// Double extra large spacing (160px) - inline control width (slider, wide inputs)
pub const SPACE_2XL: Pixels = px(160.0);

/// Card / floating panel spacing gap token (6px)
pub const SPACE_CARD_GAP: Pixels = SPACE_SM;

/// Standard tree node indentation per nesting level (14px)
pub const SPACE_TREE_INDENT: Pixels = px(14.0);

// =============================================================================
// Spacing — scaled by UI density and global UI zoom
// =============================================================================

/// Active density factor for spacing (Compact: 0.80, Default: 1.00, Comfortable: 1.25).
#[inline]
pub fn ui_space_density_factor(cx: &App) -> f32 {
    super::global::get_ui_density(cx).spacing_factor()
}

/// Dynamic spacing calculated from base pixels, UI density factor, and global UI zoom.
///
/// Snapped to integer physical pixels via `.round()` and guarded with `.max(1.0)`
/// to prevent subpixel blur or zero-pixel collapse.
#[inline]
pub fn ui_space(base_px: f32, cx: &App) -> Pixels {
    let factor = ui_space_density_factor(cx);
    let zoom = super::scale::ui_zoom_factor(cx);
    px((base_px * factor * zoom).round().max(1.0))
}

/// Double extra small spacing (2px base) scaled by density and zoom.
pub fn ui_space_2xs(cx: &App) -> Pixels {
    ui_space(2.0, cx)
}

/// Extra small spacing (4px base) scaled by density and zoom.
pub fn ui_space_xs(cx: &App) -> Pixels {
    ui_space(4.0, cx)
}

/// Small spacing (6px base) scaled by density and zoom.
pub fn ui_space_sm(cx: &App) -> Pixels {
    ui_space(6.0, cx)
}

/// Medium spacing (8px base) scaled by density and zoom.
pub fn ui_space_md(cx: &App) -> Pixels {
    ui_space(8.0, cx)
}

/// Large spacing (12px base) scaled by density and zoom.
pub fn ui_space_lg(cx: &App) -> Pixels {
    ui_space(12.0, cx)
}

/// Extra large spacing (16px base) scaled by density and zoom.
pub fn ui_space_xl(cx: &App) -> Pixels {
    ui_space(16.0, cx)
}

/// Tree indentation per nesting level (14px base) scaled by density and zoom.
pub fn ui_space_tree_indent(cx: &App) -> Pixels {
    ui_space(14.0, cx)
}

/// Card / floating panel spacing gap token scaled by UI density and zoom.
/// (Base: 6px; Compact: 5px, Default: 6px, Comfortable: 8px)
#[inline]
pub fn ui_space_card_gap(cx: &App) -> Pixels {
    ui_space_sm(cx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unscaled_constants() {
        assert_eq!(SPACE_2XS, px(2.0));
        assert_eq!(SPACE_XS, px(4.0));
        assert_eq!(SPACE_SM, px(6.0));
        assert_eq!(SPACE_MD, px(8.0));
        assert_eq!(SPACE_LG, px(12.0));
        assert_eq!(SPACE_XL, px(16.0));
        assert_eq!(SPACE_TREE_INDENT, px(14.0));
        assert_eq!(SPACE_CARD_GAP, px(6.0));
    }

    #[gpui::test]
    fn test_ui_space_card_gap_densities(cx: &mut gpui::App) {
        use crate::tokens::global::GlobalUiDensity;
        use velowork_core::theme::UiDensity;

        // Default density: 6px * 1.0 = 6px
        cx.set_global(GlobalUiDensity(|_| UiDensity::Default));
        assert_eq!(ui_space_card_gap(cx), px(6.0));

        // Compact density: 6px * 0.8 = 4.8px -> 5px
        cx.set_global(GlobalUiDensity(|_| UiDensity::Compact));
        assert_eq!(ui_space_card_gap(cx), px(5.0));

        // Comfortable density: 6px * 1.25 = 7.5px -> 8px
        cx.set_global(GlobalUiDensity(|_| UiDensity::Comfortable));
        assert_eq!(ui_space_card_gap(cx), px(8.0));
    }
}
