//! Shared tab styling for terminal tabs and service-panel (Docker) tabs.
//!
//! Both the terminal tab bar and the per-project service panel tab header use
//! the exact same visual identity so the two UIs stay consistent across:
//!
//! * background color (default + hover)
//! * selected/active effect (highlight background **and** a bottom border
//!   indicator)
//! * typography (font size, weight, color)
//! * padding and radius
//!
//! Callers build a tab with [`tab_style`] (passing a `Stateful<Div>` that
//! already has its `id` set, e.g. `div().id(...)`) and append the active
//! indicator via [`tab_active_indicator`] when the tab is active.

use gpui::*;
use gpui::prelude::*;
use crate::theme::ThemeColors;
use crate::tokens::ui_text_md;
use crate::design::semantic::SemanticPalette;

/// Unified tab height in pixels. Matches modern IDE specs (32px default),
/// giving max vertical viewport height for terminal output and editor content.
pub const TAB_HEIGHT: f32 = 32.0;

/// Returns the density-adjusted tab height, scaled by the UI scale factor.
pub fn tab_height(cx: &App) -> f32 {
    let density = crate::tokens::get_ui_density(cx);
    let scale = crate::tokens::ui_scale_factor(cx);
    (TAB_HEIGHT + density.height_offset()) * scale
}

/// Unified horizontal inner padding of a tab (unscaled constant).
pub const TAB_H_PADDING: f32 = 12.0;

/// Returns the density-adjusted and scaled horizontal inner padding of a tab.
pub fn tab_h_padding(cx: &App) -> Pixels {
    crate::tokens::ui_space_lg(cx)
}

/// Height of the active-tab bottom indicator bar.
pub const TAB_INDICATOR_H: f32 = 2.0;

/// Apply the canonical tab visual style to a `Div`.
///
/// Handles the fixed height, horizontal padding, typography, active vs
/// inactive text color, active highlight background, and hover background.
/// The returned element is `relative()` so callers can append
/// [`tab_active_indicator`] as an absolutely-positioned child for the active
/// state.
///
/// `is_active` selects the active highlight treatment.
pub fn tab_style(div: Stateful<Div>, _t: &ThemeColors, is_active: bool, cx: &App) -> Stateful<Div> {
    let palette = SemanticPalette::from_context(cx);
    div
        .relative()
        .cursor_pointer()
        .h(px(tab_height(cx) - 6.0))
        .px(tab_h_padding(cx))
        .flex()
        .items_center()
        .flex_shrink_0()
        .rounded(crate::tokens::RADIUS_MD)
        .text_size(ui_text_md(cx))
        .when(is_active, |d| {
            d.text_color(palette.text_primary)
                .bg(palette.surface_selection)
                .border_1()
                .border_color(palette.border_subtle)
                .hover(move |s| s.bg(palette.surface_hover))
        })
        .when(!is_active, |d| {
            d.text_color(palette.text_secondary)
                .hover(move |s| s.bg(palette.surface_hover).text_color(palette.text_primary))
        })
}

/// Absolutely-positioned bottom indicator shown on the active tab.
///
/// Pair this with [`tab_style`] by appending it as a child when `is_active`
/// is true. The caller supplies the indicator color so each surface can pick
/// the right semantic color:
/// * terminal tab in a single-pane (no split) project → the app accent;
/// * terminal tab in an active split pane → the system accent (`border_active`);
/// * terminal tab in an inactive split pane → the global hover background;
/// * service-panel tab → the system accent.
pub fn tab_active_indicator(color: impl Into<Hsla>) -> Div {
    div()
        .absolute()
        .bottom_0()
        .left_0()
        .right_0()
        .h(px(TAB_INDICATOR_H))
        .bg(color.into())
}
