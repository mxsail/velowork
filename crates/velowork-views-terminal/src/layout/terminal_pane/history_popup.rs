//! Floating autocomplete popup for terminal command history.

use gpui::prelude::*;
use gpui::*;
use velowork_ui::theme::{surface_bg_t, with_alpha, ThemeColors};
use velowork_ui::tokens::{
    ui_font_family, RADIUS_MD, RADIUS_STD, SPACE_SM, SPACE_XS,
};
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

    let (left, top) = match cursor_pos {
        Some(pos) => (pos.x.max(px(4.0)), (pos.y + px(4.0)).max(px(4.0))),
        None => (px(8.0), px(36.0)),
    };

    div()
        .absolute()
        .left(left)
        .top(top)
        .min_w(px(240.0))
        .max_w(px(600.0))
        .max_h(px(200.0))
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
                .gap(px(1.0))
                .children(items.iter().enumerate().map(|(idx, entry)| {
                    let is_selected = selected_index == Some(idx);
                    let row_id = format!("hist-pop-{}", entry.id);
                    let cb = on_select.clone();
                    let t_color = t.clone();

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
                        .on_click(move |_, _window, cx| {
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
                                .font_family(ui_font_family(cx))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(entry.command.clone()),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap(SPACE_XS)
                                .flex_shrink_0()
                                .when(entry.execution_count > 1, |d| {
                                    d.child(
                                        div()
                                            .px(px(4.0))
                                            .py(px(1.0))
                                            .rounded(px(3.0))
                                            .bg(if is_selected {
                                                with_alpha(t.border_active, 0.25)
                                            } else {
                                                surface_bg_t(t.bg_hover, t)
                                            })
                                            .text_color(if is_selected {
                                                rgb(t.text_primary)
                                            } else {
                                                rgb(t.text_muted)
                                            })
                                            .text_size(px(10.0))
                                            .child(format!("{}×", entry.execution_count)),
                                    )
                                }),
                        )
                })),
        )
        .into_any_element()
}
