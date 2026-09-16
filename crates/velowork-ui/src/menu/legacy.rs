//! Legacy menu helper functions for backwards compatibility.

use crate::design::semantic::SemanticPalette;
use crate::icon::AppIcon;
use crate::theme::ThemeColors;
use crate::tokens::*;
use gpui::*;
use gpui::prelude::FluentBuilder;
use crate::behavior::{HoverBehavior, StatefulElementBehaviorExt};

/// Context menu item with icon and label.
pub fn menu_item(
    id: impl Into<ElementId>,
    icon: impl Into<AppIcon>,
    label: impl Into<SharedString>,
    t: &ThemeColors,
    cx: &App,
) -> Stateful<Div> {
    menu_item_with_shortcut(id, icon, label, None, t, cx)
}

/// Context menu item without an icon (e.g. plain text actions).
pub fn menu_item_no_icon(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    t: &ThemeColors,
    cx: &App,
) -> Stateful<Div> {
    menu_item_no_icon_with_shortcut(id, label, None, t, cx)
}

/// Like [`menu_item_no_icon`] but shows a keyboard shortcut on the right side.
pub fn menu_item_no_icon_with_shortcut(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    shortcut: Option<SharedString>,
    t: &ThemeColors,
    cx: &App,
) -> Stateful<Div> {
    let p = SemanticPalette::from_context(cx);
    let text = ui_text_md(cx);
    let mut el = div()
        .id(id)
        .w_full()
        .px(SPACE_MD)
        .py(SPACE_SM)
        .flex()
        .items_center()
        .gap(SPACE_MD)
        .rounded(RADIUS_STD)
        .cursor_pointer()
        .text_size(text)
        .text_color(rgb(t.text_primary))
        .stateful_behavior(HoverBehavior { hover_bg: p.surface_hover, ..Default::default() })
        .child(div().flex_1().min_w_0().child(label.into()));
    if let Some(k) = shortcut {
        el = el.child(
            div()
                .ml(SPACE_LG)
                .text_size(ui_text_xs(cx))
                .text_color(rgb(t.text_muted))
                .child(k),
        );
    }
    el
}

/// Like [`menu_item`] but shows a keyboard shortcut on the right side.
pub fn menu_item_with_shortcut(
    id: impl Into<ElementId>,
    icon: impl Into<AppIcon>,
    label: impl Into<SharedString>,
    shortcut: Option<SharedString>,
    t: &ThemeColors,
    cx: &App,
) -> Stateful<Div> {
    menu_item_with_color_and_shortcut(
        id,
        icon,
        label,
        t.text_primary,
        t.text_muted,
        shortcut,
        t,
        cx,
    )
}

/// Context menu item with custom text and icon colors.
pub fn menu_item_with_color(
    id: impl Into<ElementId>,
    icon: impl Into<AppIcon>,
    label: impl Into<SharedString>,
    text_color: u32,
    icon_color: u32,
    t: &ThemeColors,
    cx: &App,
) -> Stateful<Div> {
    menu_item_with_color_and_shortcut(id, icon, label, text_color, icon_color, None, t, cx)
}

/// Like [`menu_item_with_color`] but shows a keyboard shortcut on the right side.
pub fn menu_item_with_color_and_shortcut(
    id: impl Into<ElementId>,
    icon: impl Into<AppIcon>,
    label: impl Into<SharedString>,
    text_color: u32,
    icon_color: u32,
    shortcut: Option<SharedString>,
    _t: &ThemeColors,
    cx: &App,
) -> Stateful<Div> {
    let p = SemanticPalette::from_context(cx);
    let text = ui_text_md(cx);
    let icon_sz = ui_icon_md(cx);
    let mut el = div()
        .id(id)
        .w_full()
        .px(SPACE_MD)
        .py(SPACE_SM)
        .flex()
        .items_center()
        .gap(SPACE_MD)
        .rounded(RADIUS_STD)
        .cursor_pointer()
        .text_size(text)
        .text_color(rgb(text_color))
        .stateful_behavior(HoverBehavior { hover_bg: p.surface_hover, ..Default::default() })
        .child(
            icon
                .into()
                .size(icon_sz)
                .text_color(rgb(icon_color)),
        )
        .child(div().flex_1().min_w_0().child(label.into()));
    if let Some(k) = shortcut {
        el = el.child(
            div()
                .ml(SPACE_LG)
                .text_size(ui_text_xs(cx))
                .text_color(p.text_muted)
                .child(k),
        );
    }
    el
}

/// Context menu item in disabled state (no hover, default cursor).
pub fn menu_item_disabled(
    id: impl Into<ElementId>,
    icon: impl Into<AppIcon>,
    label: impl Into<SharedString>,
    t: &ThemeColors,
    cx: &App,
) -> Stateful<Div> {
    menu_item_disabled_with_shortcut(id, icon, label, None, t, cx)
}

/// Like [`menu_item_disabled`] but preserves the keyboard shortcut display.
pub fn menu_item_disabled_with_shortcut(
    id: impl Into<ElementId>,
    icon: impl Into<AppIcon>,
    label: impl Into<SharedString>,
    shortcut: Option<SharedString>,
    _t: &ThemeColors,
    cx: &App,
) -> Stateful<Div> {
    let p = SemanticPalette::from_context(cx);
    let text = ui_text_md(cx);
    let icon_sz = ui_icon_md(cx);
    let mut el = div()
        .id(id)
        .w_full()
        .px(SPACE_MD)
        .py(SPACE_SM)
        .flex()
        .items_center()
        .gap(SPACE_MD)
        .rounded(RADIUS_STD)
        .text_size(text)
        .text_color(p.text_muted)
        .child(
            icon
                .into()
                .size(icon_sz)
                .text_color(p.text_muted),
        )
        .child(div().flex_1().min_w_0().child(label.into()));
    if let Some(k) = shortcut {
        el = el.child(
            div()
                .ml(SPACE_LG)
                .text_size(ui_text_xs(cx))
                .text_color(p.text_muted)
                .child(k),
        );
    }
    el
}

/// Context menu item with conditional enabled/disabled state.
pub fn menu_item_conditional(
    id: impl Into<ElementId>,
    icon: impl Into<AppIcon>,
    label: impl Into<SharedString>,
    enabled: bool,
    t: &ThemeColors,
    cx: &App,
) -> Stateful<Div> {
    menu_item_conditional_with_shortcut(id, icon, label, enabled, None, t, cx)
}

/// Like [`menu_item_conditional`] but shows a keyboard shortcut on the right side.
pub fn menu_item_conditional_with_shortcut(
    id: impl Into<ElementId>,
    icon: impl Into<AppIcon>,
    label: impl Into<SharedString>,
    enabled: bool,
    shortcut: Option<SharedString>,
    t: &ThemeColors,
    cx: &App,
) -> Stateful<Div> {
    if enabled {
        menu_item_with_shortcut(id, icon, label, shortcut, t, cx)
    } else {
        menu_item_disabled_with_shortcut(id, icon, label, shortcut, t, cx)
    }
}

/// Context menu panel with standard styling.
pub fn context_menu_panel(id: impl Into<ElementId>, _t: &ThemeColors, cx: &App) -> Stateful<Div> {
    let p = SemanticPalette::from_context(cx);
    div()
        .id(id)
        .occlude()
        .bg(p.surface_overlay)
        .border_1()
        .border_color(p.border_subtle)
        .rounded(RADIUS_LG)
        .shadow_xl()
        .min_w(px(160.0))
        .p(SPACE_XS)
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

/// Menu separator - 1px horizontal line.
pub fn menu_separator(t: &ThemeColors) -> Div {
    let p = SemanticPalette::from_theme(t);
    div()
        .h(px(1.0))
        .my(SPACE_XS)
        .bg(p.border_subtle)
}

/// Context menu item with an on/off state shown on the right side.
pub fn menu_item_toggle(
    id: impl Into<ElementId>,
    icon: impl Into<AppIcon>,
    label: impl Into<SharedString>,
    state_text: impl Into<SharedString>,
    _t: &ThemeColors,
    cx: &App,
) -> Stateful<Div> {
    let p = SemanticPalette::from_context(cx);
    let text = ui_text_md(cx);
    let icon_sz = ui_icon_md(cx);
    div()
        .id(id)
        .w_full()
        .px(SPACE_MD)
        .py(SPACE_SM)
        .flex()
        .items_center()
        .justify_between()
        .rounded(RADIUS_STD)
        .text_size(text)
        .text_color(p.text_primary)
        .cursor(CursorStyle::PointingHand)
        .stateful_behavior(HoverBehavior { hover_bg: p.surface_hover, ..Default::default() })
        .child(
            div()
                .flex()
                .flex_1()
                .items_center()
                .gap(SPACE_MD)
                .child(
                    icon
                        .into()
                        .size(icon_sz)
                        .text_color(p.text_muted),
                )
                .child(div().flex_1().min_w_0().child(label.into())),
        )
        .child(
            div()
                .text_color(p.text_muted)
                .text_size(ui_text_xs(cx))
                .child(state_text.into()),
        )
}

/// Context menu item that opens a submenu on hover.
pub fn menu_item_submenu(
    id: impl Into<ElementId>,
    icon: impl Into<AppIcon>,
    label: impl Into<SharedString>,
    on_bounds: impl Fn(Bounds<Pixels>, &mut Window, &mut App) + 'static,
    _t: &ThemeColors,
    cx: &App,
) -> Stateful<Div> {
    let p = SemanticPalette::from_context(cx);
    let text = ui_text_md(cx);
    let icon_sz = ui_icon_md(cx);
    div()
        .id(id)
        .w_full()
        .px(SPACE_MD)
        .py(SPACE_SM)
        .flex()
        .items_center()
        .justify_between()
        .rounded(RADIUS_SM)
        .text_size(text)
        .text_color(p.text_primary)
        .cursor(CursorStyle::PointingHand)
        .stateful_behavior(HoverBehavior { hover_bg: p.surface_hover, ..Default::default() })
        .child(
            div()
                .flex()
                .flex_1()
                .items_center()
                .gap(SPACE_MD)
                .child(
                    icon
                        .into()
                        .size(icon_sz)
                        .text_color(p.text_muted),
                )
                .child(div().flex_1().min_w_0().child(label.into())),
        )
        .child(
            AppIcon::ChevronRight
                .size(icon_sz)
                .text_color(p.text_muted),
        )
        .child(canvas(on_bounds, |_, _, _, _| {}).absolute().inset_0())
}

/// Context menu item with checkmark icon when `checked` is true.
pub fn menu_item_checkbox(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    checked: bool,
    shortcut: Option<SharedString>,
    t: &ThemeColors,
    cx: &App,
) -> Stateful<Div> {
    let p = SemanticPalette::from_context(cx);
    let text = ui_text_md(cx);
    let icon_sz = ui_icon_md(cx);
    let mut el = div()
        .id(id)
        .w_full()
        .px(SPACE_MD)
        .py(SPACE_SM)
        .flex()
        .items_center()
        .gap(SPACE_MD)
        .rounded(RADIUS_SM)
        .cursor_pointer()
        .text_size(text)
        .text_color(rgb(t.text_primary))
        .stateful_behavior(HoverBehavior { hover_bg: p.surface_hover, ..Default::default() })
        .child(
            div()
                .w(icon_sz)
                .flex()
                .items_center()
                .justify_center()
                .when(checked, |d| {
                    d.child(
                        AppIcon::Check
                            .size(icon_sz)
                            .text_color(rgb(t.accent)),
                    )
                }),
        )
        .child(div().flex_1().min_w_0().child(label.into()));
    if let Some(k) = shortcut {
        el = el.child(
            div()
                .ml(SPACE_LG)
                .text_size(ui_text_xs(cx))
                .text_color(p.text_muted)
                .child(k),
        );
    }
    el
}

