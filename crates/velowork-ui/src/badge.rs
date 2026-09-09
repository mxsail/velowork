//! Badge and keyboard hint components.

use crate::theme::ThemeColors;
use crate::tokens::*;
use gpui::*;
use crate::h_flex;
use crate::design::semantic::SemanticPalette;

/// Small pill label for categories like "Custom", "worktree", etc.
pub fn badge(text: impl Into<SharedString>, _t: &ThemeColors, cx: &App) -> Div {
    let p = SemanticPalette::from_context(cx);
    div()
        .px(SPACE_SM)
        .py(px(1.0))
        .rounded(RADIUS_MD)
        .bg(p.surface_raised)
        .text_size(ui_text_xs(cx))
        .text_color(p.text_muted)
        .child(text.into())
}

/// Keyboard key badge (e.g., "Enter", "Esc").
pub fn kbd(key: impl Into<SharedString>, _t: &ThemeColors, cx: &App) -> Div {
    let p = SemanticPalette::from_context(cx);
    div()
        .px(SPACE_XS)
        .py(px(1.0))
        .rounded(RADIUS_MD)
        .bg(p.surface_raised)
        .text_size(ui_text_sm(cx))
        .text_color(p.text_muted)
        .child(key.into())
}

/// Keyboard key badge + description text (e.g., `[Enter] to select`).
pub fn keyboard_hint(
    key: impl Into<SharedString>,
    description: impl Into<SharedString>,
    t: &ThemeColors,
    cx: &App,
) -> Div {
    let p = SemanticPalette::from_context(cx);
    h_flex()
        .gap(SPACE_XS)
        .child(kbd(key, t, cx))
        .child(
            div()
                .text_size(ui_text_sm(cx))
                .text_color(p.text_muted)
                .child(description.into()),
        )
}

/// Footer bar with a row of keyboard hints.
///
/// `hints` is a slice of `(key, description)` pairs.
pub fn keyboard_hints_footer(hints: &[(&str, &str)], t: &ThemeColors, cx: &App) -> Div {
    let p = SemanticPalette::from_context(cx);
    let mut footer = div()
        .px(SPACE_LG)
        .py(SPACE_MD)
        .border_t_1()
        .border_color(p.border_subtle)
        .flex()
        .items_center()
        .gap(SPACE_XL);

    for &(key, desc) in hints {
        footer = footer.child(keyboard_hint(key.to_string(), desc.to_string(), t, cx));
    }

    footer
}
