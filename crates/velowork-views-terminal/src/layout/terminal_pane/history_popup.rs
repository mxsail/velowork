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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerminalCursorPlacement {
    /// Cursor position: bottom-left of the cursor cell (relative to container)
    pub cursor_pos: Point<Pixels>,
    /// Height of one terminal line in pixels.
    pub line_height: Pixels,
    /// Width and height of the content element.
    pub container_size: Size<Pixels>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PopupVerticalPlacement {
    Below { top: Pixels, max_h: Pixels },
    Above { bottom: Pixels, max_h: Pixels },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PopupGeometry {
    pub left: Pixels,
    pub min_w: Pixels,
    pub max_w: Pixels,
    pub vertical: PopupVerticalPlacement,
}

pub fn compute_popup_geometry(
    placement: Option<TerminalCursorPlacement>,
    item_count: usize,
    row_height: Pixels,
) -> PopupGeometry {
    const POPUP_GAP: Pixels = px(4.0);
    const MAX_POPUP_HEIGHT: Pixels = px(260.0);
    const DEFAULT_MIN_W: Pixels = px(260.0);
    const DEFAULT_MAX_W: Pixels = px(640.0);

    let Some(p) = placement else {
        return PopupGeometry {
            left: px(8.0),
            min_w: DEFAULT_MIN_W,
            max_w: DEFAULT_MAX_W,
            vertical: PopupVerticalPlacement::Below {
                top: px(36.0),
                max_h: MAX_POPUP_HEIGHT,
            },
        };
    };

    let container_w = p.container_size.width;
    let container_h = p.container_size.height;
    let cursor_bottom_y = p.cursor_pos.y;
    let cursor_top_y = cursor_bottom_y - p.line_height;

    // Estimate needed height: padding/borders (6px) + each item (row_height + 2px gap)
    let estimated_h = px(6.0) + (row_height + px(2.0)) * item_count as f32;
    let ideal_h = estimated_h.min(MAX_POPUP_HEIGHT);

    let space_below = (container_h - cursor_bottom_y - POPUP_GAP).max(px(0.0));
    let space_above = (cursor_top_y - POPUP_GAP).max(px(0.0));

    // Vertical placement decision
    let vertical = if space_below >= ideal_h {
        PopupVerticalPlacement::Below {
            top: (cursor_bottom_y + POPUP_GAP).max(px(4.0)),
            max_h: space_below.min(MAX_POPUP_HEIGHT).max(row_height + px(8.0)),
        }
    } else if space_above > space_below {
        PopupVerticalPlacement::Above {
            bottom: (container_h - cursor_top_y + POPUP_GAP).max(px(4.0)),
            max_h: space_above.min(MAX_POPUP_HEIGHT).max(row_height + px(8.0)),
        }
    } else {
        PopupVerticalPlacement::Below {
            top: (cursor_bottom_y + POPUP_GAP).max(px(4.0)),
            max_h: space_below.min(MAX_POPUP_HEIGHT).max(row_height + px(8.0)),
        }
    };

    // Horizontal placement decision
    let min_w = DEFAULT_MIN_W.min(container_w - px(8.0)).max(px(120.0));
    let max_left = (container_w - min_w - px(4.0)).max(px(4.0));
    let left = p.cursor_pos.x.clamp(px(4.0), max_left);
    let max_w = (container_w - left - px(4.0)).min(DEFAULT_MAX_W).max(min_w);

    PopupGeometry {
        left,
        min_w,
        max_w,
        vertical,
    }
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
    placement: Option<TerminalCursorPlacement>,
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

    let geom = compute_popup_geometry(placement, items.len(), ap.height);

    let mut popup = div()
        .id("terminal-history-popup")
        .occlude()
        .absolute()
        .left(geom.left)
        .min_w(geom.min_w)
        .max_w(geom.max_w)
        .bg(surface_bg_t(t.bg_panel, t))
        .border_1()
        .border_color(rgb(t.border))
        .rounded(RADIUS_MD)
        .shadow_lg()
        .p(px(2.0))
        .overflow_hidden()
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        });

    match geom.vertical {
        PopupVerticalPlacement::Below { top, max_h } => {
            popup = popup.top(top).max_h(max_h);
        }
        PopupVerticalPlacement::Above { bottom, max_h } => {
            popup = popup.bottom(bottom).max_h(max_h);
        }
    }

    popup
        .child(
            v_flex()
                .id("terminal-history-popup-list")
                .w_full()
                .overflow_y_scroll()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_geometry_normal_middle_cursor_renders_below() {
        let placement = TerminalCursorPlacement {
            cursor_pos: point(px(100.0), px(200.0)),
            line_height: px(20.0),
            container_size: size(px(800.0), px(600.0)),
        };
        let geom = compute_popup_geometry(Some(placement), 5, px(24.0));
        assert_eq!(geom.left, px(100.0));
        assert_eq!(geom.min_w, px(260.0));
        match geom.vertical {
            PopupVerticalPlacement::Below { top, max_h } => {
                assert_eq!(top, px(204.0)); // 200 + 4 gap
                assert_eq!(max_h, px(260.0));
            }
            PopupVerticalPlacement::Above { .. } => panic!("Expected Below for middle cursor"),
        }
    }

    #[test]
    fn test_geometry_bottom_cursor_flips_above() {
        let placement = TerminalCursorPlacement {
            cursor_pos: point(px(100.0), px(590.0)),
            line_height: px(20.0),
            container_size: size(px(800.0), px(600.0)),
        };
        let geom = compute_popup_geometry(Some(placement), 5, px(24.0));
        assert_eq!(geom.left, px(100.0));
        match geom.vertical {
            PopupVerticalPlacement::Above { bottom, max_h } => {
                // cursor_top_y = 590 - 20 = 570. bottom = 600 - 570 + 4 = 34
                assert_eq!(bottom, px(34.0));
                assert_eq!(max_h, px(260.0));
            }
            PopupVerticalPlacement::Below { .. } => panic!("Expected Above for bottom line cursor"),
        }
    }

    #[test]
    fn test_geometry_right_edge_clamps_left() {
        let placement = TerminalCursorPlacement {
            cursor_pos: point(px(750.0), px(200.0)),
            line_height: px(20.0),
            container_size: size(px(800.0), px(600.0)),
        };
        let geom = compute_popup_geometry(Some(placement), 3, px(24.0));
        // max_left = 800 - 260 - 4 = 536. 750 clamped to 536.
        assert_eq!(geom.left, px(536.0));
        assert_eq!(geom.min_w, px(260.0));
    }

    #[test]
    fn test_geometry_tiny_container() {
        let placement = TerminalCursorPlacement {
            cursor_pos: point(px(10.0), px(60.0)),
            line_height: px(20.0),
            container_size: size(px(200.0), px(80.0)),
        };
        let geom = compute_popup_geometry(Some(placement), 5, px(24.0));
        // min_w is constrained by container_w - 8 = 192
        assert_eq!(geom.min_w, px(192.0));
        match geom.vertical {
            PopupVerticalPlacement::Above { max_h, .. } => {
                assert!(max_h <= px(80.0));
            }
            PopupVerticalPlacement::Below { max_h, .. } => {
                assert!(max_h <= px(80.0));
            }
        }
    }

    #[test]
    fn test_geometry_none_placement_fallback() {
        let geom = compute_popup_geometry(None, 5, px(24.0));
        assert_eq!(geom.left, px(8.0));
        assert_eq!(geom.min_w, px(260.0));
        assert_eq!(geom.max_w, px(640.0));
        assert_eq!(
            geom.vertical,
            PopupVerticalPlacement::Below {
                top: px(36.0),
                max_h: px(260.0)
            }
        );
    }
}
