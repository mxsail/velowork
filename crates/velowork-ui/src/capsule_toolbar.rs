//! Floating capsule toolbar component conforming to Ant Design specifications.
//!
//! Provides a floating pill-shaped action bar with glass/surface styling,
//! hover transitions, micro-dividers, and occlusion protection.

use crate::design::semantic::SemanticPalette;
use crate::icon::AppIcon;
use crate::tokens::{elevation_menu_shadow, ui_text_md, RADIUS_MD, RADIUS_SM, SPACE_SM, SPACE_XS};
use gpui::*;

/// Styled capsule toolbar container.
///
/// Sets up unified rounded geometry, shadow, border, surface overlay color (respecting global opacity),
/// and event occlusion.
pub fn capsule_toolbar_container(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    let p = SemanticPalette::from_context(cx);

    div()
        .id(id)
        .occlude()
        .flex()
        .items_center()
        .h(px(32.0))
        .px(SPACE_SM)
        .gap(SPACE_XS)
        .bg(p.surface_overlay)
        .border_1()
        .border_color(p.border_subtle)
        .rounded(RADIUS_MD)
        .shadow(elevation_menu_shadow())
        .text_size(ui_text_md(cx))
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        .on_mouse_down(MouseButton::Right, |_, _, cx| {
            cx.stop_propagation();
        })
        .on_scroll_wheel(|_, _, cx| {
            cx.stop_propagation();
        })
}

/// A compact action button inside a capsule toolbar.
pub fn capsule_action_button(
    id: impl Into<ElementId>,
    icon: AppIcon,
    label: impl Into<SharedString>,
    cx: &App,
) -> Stateful<Div> {
    let p = SemanticPalette::from_context(cx);
    let lbl = label.into();
    let text_size = ui_text_md(cx);

    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(4.0))
        .h(px(24.0))
        .px(px(6.0))
        .rounded(RADIUS_SM)
        .text_size(text_size)
        .text_color(p.text_secondary)
        .hover(|s| s.bg(p.surface_hover).text_color(p.text_primary))
        .active(|s| s.bg(p.surface_accent))
        .cursor_pointer()
        .child(icon.size(text_size).text_color(p.text_secondary))
        .child(lbl)
}

/// An icon-only button inside a capsule toolbar.
pub fn capsule_icon_button(
    id: impl Into<ElementId>,
    icon: AppIcon,
    cx: &App,
) -> Stateful<Div> {
    let p = SemanticPalette::from_context(cx);
    let icon_size = ui_text_md(cx);

    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .w(px(24.0))
        .h(px(24.0))
        .rounded(RADIUS_SM)
        .text_color(p.text_secondary)
        .hover(|s| s.bg(p.surface_hover).text_color(p.text_primary))
        .active(|s| s.bg(p.surface_accent))
        .cursor_pointer()
        .child(icon.size(icon_size).text_color(p.text_secondary))
}

/// Vertical hairline divider separating button groups in the capsule bar.
pub fn capsule_divider(cx: &App) -> Div {
    let p = SemanticPalette::from_context(cx);
    div().w(px(1.0)).h(px(14.0)).bg(p.border_subtle)
}
