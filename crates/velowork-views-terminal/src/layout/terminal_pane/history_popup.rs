//! Floating autocomplete popup for terminal command history and quick commands.

use gpui::prelude::*;
use gpui::*;
use velowork_i18n::i18n;
use velowork_ui::icon::AppIcon;
use velowork_ui::theme::{surface_bg_t, ThemeColors};
use velowork_ui::tokens::{
    ui_text_xs, ICON_STD, RADIUS_MD, RADIUS_STD, SPACE_SM, SPACE_XS,
};
use velowork_ui::tooltip::Tooltip;
use velowork_ui::tree::tree_row_appearance;
use velowork_ui::{h_flex, v_flex};
use velowork_workspace::quick_commands::QuickCommandVar;
use velowork_workspace::repositories::HistoryEntry;

#[derive(Clone, Debug)]
pub enum TerminalCompletionItem {
    History(HistoryEntry),
    QuickCommand {
        id: String,
        name: String,
        command: String,
        variables: Vec<QuickCommandVar>,
    },
}

impl TerminalCompletionItem {
    pub fn id(&self) -> String {
        match self {
            Self::History(h) => format!("hist-{}", h.id),
            Self::QuickCommand { id, .. } => format!("qc-{}", id),
        }
    }

    pub fn command(&self) -> &str {
        match self {
            Self::History(h) => &h.command,
            Self::QuickCommand { command, .. } => command,
        }
    }

    pub fn is_quick_command(&self) -> bool {
        matches!(self, Self::QuickCommand { .. })
    }
}

pub fn render_history_popup(
    items: &[TerminalCompletionItem],
    selected_index: Option<usize>,
    cursor_pos: Option<Point<Pixels>>,
    t: &ThemeColors,
    on_select: impl Fn(usize, &mut Window, &mut App) + 'static + Send + Sync,
    cx: &App,
) -> AnyElement {
    let on_select = std::sync::Arc::new(on_select);
    let ap = tree_row_appearance(t, cx);

    let tooltip_time_label = i18n!(cx, "panel.command_history.tooltip_time");
    let tooltip_count_label = i18n!(cx, "panel.command_history.tooltip_count");
    let times_suffix = i18n!(cx, "command_history.executed_times");
    let badge_qc = i18n!(cx, "quick_commands.badge_quick_command");
    let badge_hist = i18n!(cx, "quick_commands.badge_history");

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
        .min_w(px(260.0))
        .max_w(px(640.0))
        .max_h(px(260.0))
        .bg(surface_bg_t(t.bg_panel, t))
        .border_1()
        .border_color(rgb(t.border))
        .rounded(RADIUS_MD)
        .shadow_lg()
        .p(px(2.0))
        .overflow_hidden()
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        .child(
            v_flex()
                .w_full()
                .gap(px(2.0))
                .children(items.iter().enumerate().map(|(idx, item)| {
                    let is_selected = selected_index == Some(idx);
                    let row_id = format!("comp-pop-{}", item.id());
                    let cb = on_select.clone();
                    let t_color = *t;

                    match item {
                        TerminalCompletionItem::History(entry) => {
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
                                .on_click(move |_, window, cx| {
                                    cx.stop_propagation();
                                    cb(idx, window, cx);
                                })
                                .child(
                                    h_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .items_center()
                                        .gap(px(6.0))
                                        .child(
                                            div()
                                                .id(ElementId::Name(format!("hist-ico-{}", entry.id).into()))
                                                .flex_shrink_0()
                                                .w(ICON_STD)
                                                .h(ICON_STD)
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .tooltip({
                                                    let tip = badge_hist.clone();
                                                    move |_, cx| cx.new(|_| Tooltip::new(tip.clone())).into()
                                                })
                                                .child(
                                                    AppIcon::History
                                                        .svg()
                                                        .size(ICON_STD)
                                                        .text_color(if is_selected {
                                                            rgb(t.text_primary)
                                                        } else {
                                                            rgb(t.text_muted)
                                                        }),
                                                ),
                                        )
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
                                        ),
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
                        }
                        TerminalCompletionItem::QuickCommand {
                            id,
                            name,
                            command,
                            variables,
                        } => {
                            let lines: Vec<&str> = command.lines().collect();
                            let first_line = lines
                                .iter()
                                .find(|l| !l.trim().is_empty())
                                .copied()
                                .unwrap_or(command);
                            let pure_name = name.rsplit(['/', '\\']).next().unwrap_or(name);

                            let mut tip = format!("{}\n{}", name, command);
                            if !variables.is_empty() {
                                tip.push_str("\n\n");
                                for v in variables {
                                    tip.push_str(&format!("• {}: {}\n", v.name, v.hint));
                                }
                            }

                            let qc_id = id.clone();
                            let badge_qc_tip = badge_qc.clone();

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
                                    let t = tip.clone();
                                    cx.new(|_| Tooltip::new(t)).into()
                                })
                                .on_click(move |_, window, cx| {
                                    cx.stop_propagation();
                                    cb(idx, window, cx);
                                })
                                .child(
                                    h_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .items_center()
                                        .gap(px(6.0))
                                        .child(
                                            div()
                                                .id(ElementId::Name(format!("qc-ico-{}", qc_id).into()))
                                                .flex_shrink_0()
                                                .w(ICON_STD)
                                                .h(ICON_STD)
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .tooltip({
                                                    let tip = badge_qc_tip.clone();
                                                    move |_, cx| cx.new(|_| Tooltip::new(tip.clone())).into()
                                                })
                                                .child(
                                                    AppIcon::QuickCommand
                                                        .svg()
                                                        .size(ICON_STD)
                                                        .text_color(if is_selected {
                                                            rgb(t.text_primary)
                                                        } else {
                                                            rgb(t.text_muted)
                                                        }),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .flex_shrink_0()
                                                .text_size(ap.font_size)
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(if is_selected {
                                                    rgb(t.text_primary)
                                                } else {
                                                    rgb(t.text_secondary)
                                                })
                                                .child(pure_name.to_string()),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .text_size(ap.font_size)
                                                .text_color(rgb(t.text_muted))
                                                .font_family(".SystemMonoFont")
                                                .overflow_hidden()
                                                .whitespace_nowrap()
                                                .text_ellipsis()
                                                .child(first_line.to_string()),
                                        ),
                                )
                        }
                    }
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
