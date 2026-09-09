//! Empty state placeholder for lists and containers.

use crate::design::semantic::SemanticPalette;
use crate::theme::ThemeColors;
use crate::tokens::{ui_text, SPACE_LG};
use gpui::*;

/// Empty state placeholder message for lists.
///
/// Centered muted text, typically shown when a filtered list has no results.
pub fn empty_state(message: impl Into<SharedString>, t: &ThemeColors, cx: &App) -> Div {
    let p = SemanticPalette::from_theme(t);
    div()
        .px(SPACE_LG)
        .py(px(20.0))
        .text_size(ui_text(13.0, cx))
        .text_color(p.text_muted)
        .child(message.into())
}
