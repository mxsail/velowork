//! Floating autocomplete popup for terminal command history.

use gpui::prelude::*;
use gpui::*;
use velowork_i18n::i18n;
use velowork_ui::theme::{surface_bg_t, ThemeColors};
use velowork_ui::tokens::{
    ui_text_xs, RADIUS_MD, RADIUS_STD, SPACE_SM, SPACE_XS,
};
use velowork_ui::tooltip::Tooltip;
use velowork_ui::tree::tree_row_appearance;
use velowork_ui::{h_flex, v_flex};
use velowork_workspace::repositories::HistoryEntry;

pub fn render_history_popup(
    items: &[HistoryEntry],
    selected_index: Option<usize>,
    cursor_pos: Option<Point<Pixels>>,
    t: &ThemeColors,
    on_select: impl Fn(usize, &mut App) + 'static + Send + Sync,
    cx: &App,
) -> AnyElement {
    let on_select = std::sync::Arc::new(on_select);
    let ap = tree_row_appearance(t, cx);

    let tooltip_time_label = i18n!(cx, "panel.command_history.tooltip_time");
    let tooltip_count_label = i18n!(cx, "panel.command_history.tooltip_count");
    let times_suffix = i18n!(cx, "command_history.executed_times");

    let (left, top) = match cursor_pos {
        Some(pos) => (pos.x.max(px(4.0)), (pos.y + px(4.0)).max(px(4.0))),
        None => (px(8.0), px(36.0)),
    };

    div()
        .id("terminal-history-popup")
        .occlude()
        .absolute()
        .left(left)
        .top(top)
        .min_w(px(240.0))
        .max_w(px(600.0))
        .max_h(px(240.0))
        .bg(surface_bg_t(t.bg_panel, t))
        .border_1()
        .border_color(rgb(t.border))
        .rounded(RADIUS_MD)
        .shadow_lg()
        .p(px(2.0))
        .overflow_hidden()
        .child(
            v_flex()
                .w_full()
                .gap(px(2.0))
                .children(items.iter().enumerate().map(|(idx, entry)| {
                    let is_selected = selected_index == Some(idx);
                    let row_id = format!("hist-pop-{}", entry.id);
                    let cb = on_select.clone();
                    let t_color = *t;

                    let lines: Vec<&str> = entry.command.lines().collect();
                    let first_line = lines
                        .iter()
                        .find(|l| !l.trim().is_empty())
                        .copied()
                        .unwrap_or(&entry.command);
                    let is_multiline = lines.len() > 1;
                    let extra_lines_count = lines.len().saturating_sub(1);

                    let tip_text = if entry.timestamp.is_empty() {
                        entry.command.clone()
                    } else {
                        format!(
                            "{}\n{}: {}\n{}: {} {}",
                            entry.command,
                            tooltip_time_label,
                            format_full_timestamp(&entry.timestamp),
                            tooltip_count_label,
                            entry.execution_count,
                            times_suffix,
                        )
                    };

                    h_flex()
                        .id(ElementId::Name(row_id.into()))
                        .w_full()
                        .h(ap.height)
                        .items_center()
                        .justify_between()
                        .px(SPACE_SM)
                        .rounded(RADIUS_STD)
                        .gap(SPACE_XS)
                        .cursor_pointer()
                        .border_1()
                        .border_color(if is_selected {
                            rgb(t.border_active)
                        } else {
                            rgba(0x00000000)
                        })
                        .bg(if is_selected {
                            surface_bg_t(t.bg_selection, t)
                        } else {
                            transparent_black()
                        })
                        .hover(move |s| {
                            if !is_selected {
                                s.bg(surface_bg_t(t_color.bg_hover, &t_color))
                            } else {
                                s
                            }
                        })
                        .tooltip(move |_, cx| {
                            let tip = tip_text.clone();
                            cx.new(|_| Tooltip::new(tip)).into()
                        })
                        .on_click(move |_, _window, cx| {
                            cx.stop_propagation();
                            cb(idx, cx);
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(ap.font_size)
                                .text_color(if is_selected {
                                    rgb(t.text_primary)
                                } else {
                                    rgb(t.text_secondary)
                                })
                                .font_family(".SystemMonoFont")
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(first_line.to_string()),
                        )
                        .when(is_multiline, |d| {
                            d.child(
                                div()
                                    .flex_shrink_0()
                                    .text_size(ui_text_xs(cx))
                                    .text_color(rgb(t.text_muted))
                                    .child(format!("+{}", extra_lines_count)),
                            )
                        })
                })),
        )
        .into_any_element()
}

fn format_full_timestamp(ts: &str) -> String {
    if ts.len() >= 19 {
        ts[..19].replace('T', " ")
    } else {
        ts.to_string()
    }
}
