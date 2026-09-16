//! Dropdown component for selecting from a list of options.
//!
//! Provides a reusable dropdown button with overlay list.

use crate::design::semantic::SemanticPalette;
use crate::input::focus_ring_shadows;
use crate::theme::ThemeColors;
use crate::tokens::{ui_text_md, ui_text_sm, RADIUS_LG, RADIUS_STD, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS};
use gpui::*;
use crate::behavior::{HoverBehavior, SelectedBehavior, StatefulElementBehaviorExt};
use gpui::prelude::*;

/// Create a dropdown trigger button that tracks its own bounds for overlay positioning.
///
/// The `on_bounds` callback is called during each paint with the button's window-absolute bounds.
/// Use these bounds with `dropdown_anchored_below()` to position the overlay.
///
/// The button is fixed at `220px` wide by default (matches the rest of the app's
/// dropdowns). Pass `None` to `dropdown_button_sized` for a content-driven width that
/// shrinks to fit the label instead of reserving a fixed 220px column.
pub fn dropdown_button(
    id: impl Into<SharedString>,
    label: &str,
    is_open: bool,
    t: &ThemeColors,
    cx: &App,
    on_bounds: impl Fn(Bounds<Pixels>, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    dropdown_button_sized(id, label, is_open, t, cx, on_bounds, Some(px(220.0)))
}

/// Like [`dropdown_button`], but with an explicit width policy.
///
/// * `width = Some(w)` — fixed width (the default `dropdown_button` behaviour).
/// * `width = None` — content-driven width: the button shrinks to fit its label
///   (with a small floor) so short labels such as the Commands panel's
///   target-host selector don't reserve a wide, empty column.
pub fn dropdown_button_sized(
    id: impl Into<SharedString>,
    label: &str,
    is_open: bool,
    t: &ThemeColors,
    cx: &App,
    on_bounds: impl Fn(Bounds<Pixels>, &mut Window, &mut App) + 'static,
    width: Option<Pixels>,
) -> Stateful<Div> {
    let p = SemanticPalette::from_context(cx);
    let ring = focus_ring_shadows(t);
    let button = div()
        .id(ElementId::Name(id.into()))
        .relative()
        .cursor_pointer()
        .min_w(px(140.0))
        .h(px(28.0))
        .px(px(10.0))
        .rounded(RADIUS_STD)
        .bg(if is_open { p.surface_hover } else { p.surface_card })
        .border_1()
        .border_color(if is_open { p.border_active } else { p.border_subtle })
        .when(is_open, |d| d.shadow(ring))
        .when(!is_open, |d| {
            d.hover(|s| s.border_color(p.surface_accent.opacity(0.6)).bg(p.surface_hover))
        })
        .flex()
        .items_center()
        .justify_between()
        .gap(SPACE_MD)
        .child(
            div()
                .text_size(ui_text_md(cx))
                .text_color(p.text_primary)
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
                .child(label.to_string()),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_size(ui_text_sm(cx))
                .text_color(p.text_muted)
                .child(if is_open { "▲" } else { "▼" }),
        )
        .child(canvas(on_bounds, |_, _, _, _| {}).absolute().inset_0());

    if let Some(w) = width {
        button.w(w)
    } else {
        button.min_w(px(100.0))
    }
}

/// Position a dropdown overlay below the given trigger bounds.
///
/// The overlay's width is automatically synced to the trigger button's width.
pub fn dropdown_anchored_below(bounds: Bounds<Pixels>, child: impl IntoElement) -> Deferred {
    let width = bounds.size.width;
    deferred(
        anchored()
            .position(point(bounds.origin.x, bounds.origin.y + bounds.size.height + px(2.0)))
            .snap_to_window()
            .child(
                div()
                    .w(width)
                    .child(child),
            )
    )
}

/// Position a dropdown overlay above the given trigger bounds.
///
/// Uses `Anchor::BottomLeft` so the overlay's bottom edge aligns just above
/// the button's top edge. Width is synced to the trigger button.
pub fn dropdown_anchored_above(bounds: Bounds<Pixels>, child: impl IntoElement) -> Deferred {
    let width = bounds.size.width;
    deferred(
        anchored()
            .position(point(bounds.origin.x, bounds.origin.y - px(2.0)))
            .anchor(gpui::Anchor::BottomLeft)
            .snap_to_window()
            .child(
                div()
                    .w(width)
                    .child(child),
            )
    )
}

/// Create a dropdown overlay container.
///
/// Visual style is intentionally identical to `context_menu_panel` so that
/// dropdown popups and right-click menus share one consistent look
/// (background color, border, radius, shadow, padding).
pub fn dropdown_overlay(
    id: impl Into<SharedString>,
    _t: &ThemeColors,
    cx: &App,
) -> Stateful<Div> {
    let p = SemanticPalette::from_context(cx);
    div()
        .id(ElementId::Name(id.into()))
        .occlude()
        .w_full()
        .max_h(px(220.0))
        .overflow_y_scroll()
        .bg(p.surface_overlay)
        .border_1()
        .border_color(p.border_subtle)
        .rounded(RADIUS_LG)
        .shadow_xl()
        .p(SPACE_XS)
        // Prevent scroll events from propagating to terminal underneath
        .on_scroll_wheel(|_, _, cx| {
            cx.stop_propagation();
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
}

/// Create a single dropdown option row.
///
/// Visual style mirrors `menu_item` (inset rounded pill, `bg_hover` highlight)
/// so the selected/hovered highlight is identical to the context menu.
pub fn dropdown_option(
    id: impl Into<SharedString>,
    label: &str,
    is_selected: bool,
    _t: &ThemeColors,
    cx: &App,
) -> Stateful<Div> {
    let p = SemanticPalette::from_context(cx);
    let check_color = if is_selected {
        p.border_active
    } else {
        p.text_muted
    };
    let row = div()
        .id(ElementId::Name(id.into()))
        .w_full()
        .px(SPACE_LG)
        .py(SPACE_SM)
        .rounded(RADIUS_STD)
        .cursor_pointer()
        .text_size(ui_text_md(cx))
        .text_color(p.text_primary)
        .stateful_behavior(SelectedBehavior { selected: is_selected, bg: p.surface_selection, fg: None })
        .stateful_behavior(HoverBehavior { hover_bg: p.surface_hover, ..Default::default() })
        .flex()
        .items_center()
        .justify_between()
        .gap(SPACE_LG)
        .child(label.to_string());

    if is_selected {
        row.child(
            div()
                .text_size(ui_text_sm(cx))
                .text_color(check_color)
                .child("✓"),
        )
    } else {
        row
    }
}
