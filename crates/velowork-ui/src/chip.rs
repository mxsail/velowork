//! Chip indicator components.

use crate::theme::ThemeColors;
use crate::icon::AppIcon;
use crate::tokens::*;
use gpui::*;
use crate::h_flex;
use crate::design::appearance::{ControlAppearance, ControlSize, ControlVariant};
use crate::design::density::UiDensity;
use crate::design::semantic::SemanticPalette;
use crate::behavior::{HoverBehavior, StatefulElementBehaviorExt};

/// Shell indicator chip showing current shell name with dropdown chevron.
///
/// Returns a Stateful<Div> that can have `.on_mouse_down()` and `.tooltip()` chained.
///
/// Geometry (radius) and chrome colors (bg / hover / text) are resolved from the
/// frozen design system via `ControlAppearance` + `SemanticPalette` so the chip
/// shares the same visual weight as every other `Compact` control. The fixed
/// `HEIGHT_CHIP` / `SPACE_SM` sizing is retained to preserve the dense indicator
/// footprint.
pub fn shell_indicator_chip(
    id: impl Into<ElementId>,
    shell_name: impl Into<SharedString>,
    t: &ThemeColors,
    cx: &App,
) -> Stateful<Div> {
    let p = SemanticPalette::from_context(cx);
    let density = crate::tokens::get_ui_density(cx);
    let scale = crate::tokens::ui_text_scale(cx);
    shell_indicator_chip_with_palette(id, shell_name, t, &p, density, scale)
}

/// Like [`shell_indicator_chip`] but driven by explicit UI density and scale.
pub fn shell_indicator_chip_scaled(
    id: impl Into<ElementId>,
    shell_name: impl Into<SharedString>,
    t: &ThemeColors,
    density: UiDensity,
    scale: f32,
) -> Stateful<Div> {
    let p = SemanticPalette::from_theme(t);
    shell_indicator_chip_with_palette(id, shell_name, t, &p, density, scale)
}

fn shell_indicator_chip_with_palette(
    id: impl Into<ElementId>,
    shell_name: impl Into<SharedString>,
    _t: &ThemeColors,
    p: &SemanticPalette,
    density: UiDensity,
    scale: f32,
) -> Stateful<Div> {
    let geom = ControlAppearance::resolve(
        ControlSize::Compact,
        ControlVariant::Secondary,
        &p,
        density,
        scale,
    );
    div()
        .id(id)
        .cursor_pointer()
        .px(SPACE_SM)
        .h(geom.height)
        .flex()
        .items_center()
        .justify_center()
        .rounded(geom.radius)
        .bg(geom.bg)
        .stateful_behavior(HoverBehavior { hover_bg: geom.bg_hover, ..Default::default() })
        .child(
            h_flex()
                .gap(SPACE_XS)
                .child(
                    div()
                        .text_size(px(10.0 * scale))
                        .text_color(p.text_secondary)
                        .child(shell_name.into()),
                )
                .child(
                AppIcon::ChevronDown
                    .size(ICON_SM)
                    .text_color(p.text_secondary),
                ),
        )
}
