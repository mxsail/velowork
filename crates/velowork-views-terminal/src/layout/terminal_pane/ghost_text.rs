//! Inline Ghost Text completion overlay for terminal pane.

use gpui::prelude::*;
use gpui::*;
use velowork_i18n::i18n;
use velowork_ui::h_flex;
use velowork_ui::theme::{surface_bg_t, with_alpha, ThemeColors};
use velowork_ui::tokens::SPACE_XS;

/// State for active ghost text command suggestion.
#[derive(Clone, Debug, PartialEq)]
pub struct GhostTextState {
    /// The input buffer that triggered this ghost text (e.g. "# list docker" or "git st")
    pub prompt: String,
    /// The full suggested command (e.g. "docker ps -a" or "git status")
    pub suggestion: String,
    /// Whether this suggestion came from a natural language trigger (e.g. starts with "#")
    pub is_natural_language: bool,
}

/// Renders the inline ghost text directly adjacent to the terminal cursor.
pub fn render_ghost_text_overlay(
    state: &GhostTextState,
    cursor_pos: Point<Pixels>,
    cell_height: Pixels,
    font_size: Pixels,
    font_family: &str,
    t: &ThemeColors,
    cx: &App,
) -> AnyElement {
    let display_text = if state.is_natural_language {
        format!("→ {}", state.suggestion)
    } else if state.suggestion.starts_with(&state.prompt) {
        state.suggestion[state.prompt.len()..].to_string()
    } else {
        format!("→ {}", state.suggestion)
    };

    if display_text.is_empty() {
        return div().into_any_element();
    }

    let top = (cursor_pos.y - cell_height).max(px(0.0));
    let left = (cursor_pos.x + px(3.0)).max(px(0.0));

    div()
        .absolute()
        .left(left)
        .top(top)
        .h(cell_height)
        .items_center()
        .flex()
        .flex_row()
        .gap(SPACE_XS)
        .child(
            div()
                .text_size(font_size)
                .text_color(with_alpha(t.text_primary, 0.45))
                .font_family(font_family.to_string())
                .whitespace_nowrap()
                .child(display_text),
        )
        .child(
            h_flex()
                .gap(px(3.0))
                .items_center()
                .child(
                    div()
                        .px(px(4.0))
                        .py(px(0.5))
                        .rounded(px(3.0))
                        .bg(surface_bg_t(t.bg_selection, t))
                        .text_color(with_alpha(t.text_secondary, 0.75))
                        .text_size(px(10.0))
                        .child(i18n!(cx, "ai.ghost_text.tab_accept")),
                )
                .child(
                    div()
                        .px(px(4.0))
                        .py(px(0.5))
                        .rounded(px(3.0))
                        .bg(surface_bg_t(t.bg_selection, t))
                        .text_color(with_alpha(t.text_muted, 0.6))
                        .text_size(px(10.0))
                        .child(i18n!(cx, "ai.ghost_text.esc_dismiss")),
                ),
        )
        .into_any_element()
}
