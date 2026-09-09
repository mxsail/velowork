//! Settings panel components.

use crate::design::semantic::SemanticPalette;
use crate::icon::AppIcon;
use crate::theme::ThemeColors;
use crate::tokens::{
    ui_space_lg, ui_text, ui_text_md, ui_text_sm, ui_text_xl, ICON_STD, RADIUS_STD, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS,
};
use gpui::*;
use crate::v_flex;
use crate::behavior::{HoverBehavior, StatefulElementBehaviorExt};

/// Render a section header (disabled: all settings flattened).
pub fn section_header(_title: &str, _t: &ThemeColors, _cx: &App) -> impl IntoElement {
    div()
}

/// Render a settings section container (pure layout container, flattened).
pub fn section_container(_t: &ThemeColors) -> Div {
    v_flex().w_full().gap(SPACE_XS)
}

/// A grouped settings card with an icon + title header (flattened).
pub fn settings_card(
    title: impl Into<SharedString>,
    icon: AppIcon,
    content: impl IntoElement,
    t: &ThemeColors,
    cx: &App,
) -> Div {
    let p = SemanticPalette::from_theme(t);
    v_flex()
        .w_full()
        .mb(ui_space_lg(cx))
        .child(
            div()
                .flex()
                .items_center()
                .gap(SPACE_MD)
                .px(SPACE_LG)
                .pt(SPACE_LG)
                .pb(SPACE_MD)
                .child(
                    icon.size(ICON_STD).text_color(p.text_secondary),
                )
                .child(
                    div()
                        .text_size(ui_text_md(cx))
                        .font_weight(FontWeight::BOLD)
                        .text_color(p.text_secondary)
                        .child(title.into()),
                ),
        )
        .child(content)
}

/// Render a settings row container.
pub fn settings_row(id: impl Into<SharedString>, label: &str, t: &ThemeColors, cx: &App, _has_border: bool) -> Stateful<Div> {
    let p = SemanticPalette::from_theme(t);
    div()
        .id(ElementId::Name(id.into()))
        .py(SPACE_SM)
        .flex()
        .items_center()
        .justify_between()
        .gap(SPACE_MD)
        .child(
            div()
                .text_size(ui_text_md(cx))
                .text_color(p.text_secondary)
                .child(label.to_string()),
        )
}

/// Render a settings row with label and description.
pub fn settings_row_with_desc(id: impl Into<SharedString>, label: &str, desc: &str, t: &ThemeColors, cx: &App, _has_border: bool) -> Stateful<Div> {
    let p = SemanticPalette::from_theme(t);
    div()
        .id(ElementId::Name(id.into()))
        .py(SPACE_SM)
        .flex()
        .items_center()
        .justify_between()
        .gap(SPACE_MD)
        .child(
            v_flex()
                .gap(px(2.0))
                .child(
                    div()
                        .text_size(ui_text_md(cx))
                        .text_color(p.text_secondary)
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .text_size(ui_text_sm(cx))
                        .text_color(p.text_muted)
                        .child(desc.to_string()),
                ),
        )
}

/// Render a +/- stepper button.
pub fn stepper_button(id: impl Into<SharedString>, label: &str, _t: &ThemeColors, cx: &App) -> Stateful<Div> {
    let p = SemanticPalette::from_context(cx);
    let ctrl_h = crate::design::appearance::control_height(cx);
    div()
        .id(ElementId::Name(id.into()))
        .cursor_pointer()
        .w(ctrl_h)
        .h(ctrl_h)
        .flex()
        .items_center()
        .justify_center()
        .rounded(RADIUS_STD)
        .bg(p.surface_raised)
        .border_1()
        .border_color(p.border_subtle)
        .stateful_behavior(HoverBehavior { hover_bg: p.surface_hover, ..Default::default() })
        .text_size(ui_text_xl(cx))
        .text_color(p.text_secondary)
        .child(label.to_string())
}

/// Render a value display box.
pub fn value_display(value: String, width: f32, _t: &ThemeColors, cx: &App) -> Div {
    let p = SemanticPalette::from_context(cx);
    let ctrl_h = crate::design::appearance::control_height(cx);
    div()
        .w(px(width))
        .h(ctrl_h)
        .flex()
        .items_center()
        .justify_center()
        .rounded(RADIUS_STD)
        .bg(p.surface_raised)
        .border_1()
        .border_color(p.border_subtle)
        .text_size(ui_text(13.0, cx))
        .font_family("monospace")
        .text_color(p.text_primary)
        .child(value)
}
