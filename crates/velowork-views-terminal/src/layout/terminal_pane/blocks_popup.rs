//! Blocks panel / popup displaying structured command execution blocks for a terminal.

use gpui::prelude::*;
use gpui::*;
use velowork_core::theme::ThemeColors;
use velowork_i18n::i18n;
use velowork_terminal::terminal::TerminalBlock;
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::scrollable::ScrollableElement;
use velowork_ui::theme::surface_bg_t;
use velowork_ui::tokens::{
    ui_font_family, RADIUS_MD, RADIUS_STD, SPACE_SM, SPACE_XS,
};
use velowork_ui::{h_flex, v_flex};

/// Render a list of structured execution blocks.
pub fn render_blocks_list(
    blocks: &[TerminalBlock],
    t: &ThemeColors,
    on_copy_output: impl Fn(&str, &mut App) + 'static + Send + Sync,
    on_re_run: impl Fn(&str, &mut App) + 'static + Send + Sync,
    on_send_to_ai: impl Fn(&TerminalBlock, &mut App) + 'static + Send + Sync,
    cx: &App,
) -> AnyElement {
    let p = SemanticPalette::from_context(cx);
    let on_copy = std::sync::Arc::new(on_copy_output);
    let on_rerun = std::sync::Arc::new(on_re_run);
    let on_ai = std::sync::Arc::new(on_send_to_ai);

    if blocks.is_empty() {
        return div()
            .p(SPACE_SM)
            .text_color(p.text_muted)
            .font_family(ui_font_family(cx))
            .child(i18n!(cx, "terminal.no_blocks_recorded"))
            .into_any_element();
    }

    v_flex()
        .w_full()
        .gap(SPACE_SM)
        .children(blocks.iter().rev().map(|block| {
            let b = block.clone();
            let is_success = b.is_success();
            let status_color = if is_success {
                rgb(0x10b981) // Green
            } else {
                rgb(0xef4444) // Red
            };

            let status_text = match b.exit_code {
                Some(code) => format!("exit {}", code),
                None => "running".to_string(),
            };

            let dur_text = b
                .duration_ms
                .map(|d| format!("{}ms", d))
                .unwrap_or_default();

            let cmd_str = b.command.clone();
            let output_str = b.clean_output.clone();
            let copy_cb = on_copy.clone();
            let rerun_cb = on_rerun.clone();
            let ai_cb = on_ai.clone();
            let b_for_ai = b.clone();

            v_flex()
                .id(ElementId::Name(format!("block-card-{}", b.id).into()))
                .w_full()
                .bg(surface_bg_t(t.bg_panel, t))
                .border_1()
                .border_color(rgb(t.border))
                .rounded(RADIUS_MD)
                .p(SPACE_SM)
                .gap(SPACE_XS)
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .justify_between()
                        .child(
                            h_flex()
                                .items_center()
                                .gap(SPACE_XS)
                                .child(
                                    div()
                                        .w(px(8.0))
                                        .h(px(8.0))
                                        .rounded_full()
                                        .bg(status_color),
                                )
                                .child(
                                    div()
                                        .font_family(ui_font_family(cx))
                                        .text_size(px(12.0))
                                        .text_color(p.text_primary)
                                        .font_weight(FontWeight::BOLD)
                                        .child(format!("$ {}", b.command)),
                                )
                                .child(
                                    div()
                                        .text_size(px(10.0))
                                        .text_color(p.text_muted)
                                        .child(format!("({} {})", status_text, dur_text)),
                                ),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap(SPACE_XS)
                                .child(
                                    div()
                                        .id(ElementId::Name(format!("btn-copy-{}", b.id).into()))
                                        .cursor_pointer()
                                        .text_size(px(11.0))
                                        .text_color(p.text_secondary)
                                        .hover(|s| s.text_color(rgb(0x3b82f6)))
                                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                            copy_cb(&output_str, cx);
                                        })
                                        .child(i18n!(cx, "common.copy")),
                                )
                                .child(
                                    div()
                                        .id(ElementId::Name(format!("btn-rerun-{}", b.id).into()))
                                        .cursor_pointer()
                                        .text_size(px(11.0))
                                        .text_color(p.text_secondary)
                                        .hover(|s| s.text_color(rgb(0x10b981)))
                                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                            rerun_cb(&cmd_str, cx);
                                        })
                                        .child(i18n!(cx, "terminal.rerun")),
                                )
                                .child(
                                    div()
                                        .id(ElementId::Name(format!("btn-ai-{}", b.id).into()))
                                        .cursor_pointer()
                                        .text_size(px(11.0))
                                        .text_color(p.text_secondary)
                                        .hover(|s| s.text_color(rgb(0x8b5cf6)))
                                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                            ai_cb(&b_for_ai, cx);
                                        })
                                        .child(i18n!(cx, "terminal.ask_ai")),
                                ),
                        ),
                )
                .child(
                    div()
                        .w_full()
                        .max_h(px(160.0))
                        .overflow_y_scrollbar()
                        .bg(rgb(0x0a0a0a))
                        .rounded(RADIUS_STD)
                        .p(SPACE_XS)
                        .font_family(ui_font_family(cx))
                        .text_size(px(11.0))
                        .text_color(rgb(0xd1d5db))
                        .child(if b.clean_output.trim().is_empty() {
                            "(no output)".to_string()
                        } else {
                            b.clean_output.clone()
                        }),
                )
        }))
        .into_any_element()
}
