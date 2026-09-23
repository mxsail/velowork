//! Code block container component.

use crate::design::semantic::SemanticPalette;
use crate::theme::ThemeColors;
use crate::tokens::*;
use gpui::prelude::FluentBuilder;
use gpui::*;
use crate::v_flex;

/// Code block container with rounded corners, bg, border, overflow_hidden, and optional language label.
///
/// Caller adds `.child(...)` for the code content area.
pub fn code_block_container(language: Option<&str>, _t: &ThemeColors, cx: &App) -> Div {
    let p = SemanticPalette::from_context(cx);
    let lang_label = language.unwrap_or("");
    v_flex()
        .rounded(px(6.0))
        .bg(p.surface_base)
        .border_1()
        .border_color(p.border_subtle)
        .overflow_hidden()
        // Code / logs always render in the configured monospace family.
        .font_family(mono_font_family(cx))
        .when(!lang_label.is_empty(), |d| {
            d.child(
                div()
                    .px(SPACE_LG)
                    .py(SPACE_XS)
                    .bg(p.surface_header)
                    .border_b_1()
                    .border_color(p.border_subtle)
                    .text_size(ui_text_sm(cx))
                    .text_color(p.text_muted)
                    .child(lang_label.to_string()),
            )
        })
}
