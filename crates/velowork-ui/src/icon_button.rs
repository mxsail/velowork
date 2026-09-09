//! Reusable icon button component.
//!
//! A small square button containing an SVG icon with hover background.
//!
//! Geometry and hover are driven by [`ControlAppearance`] (Compact tier) so
//! icon buttons share the same visual weight as other controls instead of
//! carrying magic numbers.

use crate::behavior::{HoverBehavior, StatefulElementBehaviorExt};
use crate::icon::AppIcon;
use crate::design::appearance::{ControlAppearance, ControlSize, ControlVariant};
use crate::design::density::UiDensity;
use crate::design::semantic::SemanticPalette;
use crate::input::focus_ring_shadows;
use crate::theme::ThemeColors;
use crate::tokens::RADIUS_STD;
use gpui::*;

/// Resolve the canonical appearance for an icon button (Compact, Ghost).
#[allow(dead_code)]
fn icon_button_appearance(t: &ThemeColors, cx: &App) -> ControlAppearance {
    let density = crate::tokens::get_ui_density(cx);
    let scale = crate::tokens::ui_text_scale(cx);
    icon_button_appearance_scaled(t, density, scale)
}

/// Like [`icon_button_appearance`] but driven by explicit density and scale.
fn icon_button_appearance_scaled(
    t: &ThemeColors,
    density: UiDensity,
    scale: f32,
) -> ControlAppearance {
    let palette = SemanticPalette::from_theme(t);
    ControlAppearance::resolve(
        ControlSize::Compact,
        ControlVariant::Ghost,
        &palette,
        density,
        scale,
    )
}

/// Small icon button (Compact tier: 24x24, 12px icon).
///
/// Returns a `Stateful<Div>` ready for `.on_click()`, `.tooltip()`, etc.
///
/// # Example
///
/// ```rust,ignore
/// icon_button("close-btn", AppIcon::Close, &t, cx)
///     .on_click(cx.listener(|this, _, _, cx| this.close(cx)))
/// ```
pub fn icon_button(
    id: impl Into<ElementId>,
    icon: impl Into<AppIcon>,
    _t: &ThemeColors,
    cx: &App,
) -> Stateful<Div> {
    let p = SemanticPalette::from_context(cx);
    let density = crate::tokens::get_ui_density(cx);
    let scale = crate::tokens::ui_text_scale(cx);
    let a = ControlAppearance::resolve(
        ControlSize::Compact,
        ControlVariant::Ghost,
        &p,
        density,
        scale,
    );
    let hover_bg = p.surface_hover;
    let el_id = id.into();
    let group_id = SharedString::from(format!("icon-btn-{:?}", el_id));
    div()
        .id(el_id)
        .group(group_id.clone())
        .flex_shrink_0()
        .cursor_pointer()
        .w(a.height)
        .h(a.height)
        .flex()
        .items_center()
        .justify_center()
        .rounded(RADIUS_STD)
        .stateful_behavior(HoverBehavior { hover_bg, ..Default::default() })
        .child(
            icon
                .into()
                .size(a.icon_size)
                .text_color(p.text_secondary)
                .group_hover(group_id, |s| s.text_color(p.text_primary)),
        )
}

/// Compact icon button with loading state.
///
/// When `loading` is true, displays an animated rotating `LoaderCircle` spinner,
/// dims the button (`opacity(0.5)`), sets the cursor to `Arrow`, and skips hover behavior.
pub fn icon_button_loading(
    id: impl Into<ElementId>,
    icon: impl Into<AppIcon>,
    loading: bool,
    _t: &ThemeColors,
    cx: &App,
) -> Stateful<Div> {
    let p = SemanticPalette::from_context(cx);
    let density = crate::tokens::get_ui_density(cx);
    let scale = crate::tokens::ui_text_scale(cx);
    let a = ControlAppearance::resolve(
        ControlSize::Compact,
        ControlVariant::Ghost,
        &p,
        density,
        scale,
    );
    let el_id = id.into();
    let group_id = SharedString::from(format!("icon-btn-{:?}", el_id));

    let mut container = div()
        .id(el_id.clone())
        .group(group_id.clone())
        .flex_shrink_0()
        .w(a.height)
        .h(a.height)
        .flex()
        .items_center()
        .justify_center()
        .rounded(RADIUS_STD);

    if loading {
        let anim_id = format!("{:?}-spinner", el_id);
        let spinner = crate::spinner::loading_spinner(anim_id, a.icon_size, p.text_secondary);
        container = container
            .opacity(0.50)
            .cursor(CursorStyle::Arrow)
            .child(spinner);
    } else {
        let hover_bg = p.surface_hover;
        container = container
            .cursor_pointer()
            .stateful_behavior(HoverBehavior { hover_bg, ..Default::default() })
            .child(
                icon
                    .into()
                    .size(a.icon_size)
                    .text_color(p.text_secondary)
                    .group_hover(group_id, |s| s.text_color(p.text_primary)),
            );
    }

    container
}

/// Icon button with custom button and icon sizes.
///
/// Prefer [`icon_button`] unless a non-Compact size is explicitly required.
pub fn icon_button_sized(
    id: impl Into<ElementId>,
    icon: impl Into<AppIcon>,
    button_size: f32,
    icon_size: f32,
    t: &ThemeColors,
) -> Stateful<Div> {
    icon_button_sized_px(id, icon, px(button_size), px(icon_size), t)
}

/// Icon button with custom button and icon sizes expressed as [`gpui::Pixels`].
///
/// Use this when sizes should follow the UI text scale (e.g. `ui_text_xl(cx)`).
pub fn icon_button_sized_px(
    id: impl Into<ElementId>,
    icon: impl Into<AppIcon>,
    button_size: Pixels,
    icon_size: Pixels,
    t: &ThemeColors,
) -> Stateful<Div> {
    let p = SemanticPalette::from_theme(t);
    let hover_bg = p.surface_raised;
    let el_id = id.into();
    let group_id = SharedString::from(format!("icon-btn-{:?}", el_id));
    div()
        .id(el_id)
        .group(group_id.clone())
        .flex_shrink_0()
        .cursor_pointer()
        .w(button_size)
        .h(button_size)
        .flex()
        .items_center()
        .justify_center()
        .rounded(RADIUS_STD)
        .stateful_behavior(HoverBehavior { hover_bg, ..Default::default() })
        .child(
            icon
                .into()
                .size(icon_size)
                .text_color(p.text_secondary)
                .group_hover(group_id, |s| s.text_color(p.text_primary)),
        )
}

/// Small icon button whose geometry scales with the active UI density and
/// scale factor read from application settings.
pub fn icon_button_scaled(
    id: impl Into<ElementId>,
    icon: impl Into<AppIcon>,
    t: &ThemeColors,
    density: UiDensity,
    scale: f32,
) -> Stateful<Div> {
    let p = SemanticPalette::from_theme(t);
    let a = icon_button_appearance_scaled(t, density, scale);
    let hover_bg = a.bg_hover;
    let el_id = id.into();
    let group_id = SharedString::from(format!("icon-btn-{:?}", el_id));
    div()
        .id(el_id)
        .group(group_id.clone())
        .flex_shrink_0()
        .cursor_pointer()
        .w(a.height)
        .h(a.height)
        .flex()
        .items_center()
        .justify_center()
        .rounded(RADIUS_STD)
        .stateful_behavior(HoverBehavior { hover_bg, ..Default::default() })
        .child(
            icon
                .into()
                .size(a.icon_size)
                .text_color(p.text_secondary)
                .group_hover(group_id, |s| s.text_color(p.text_primary)),
        )
}

/// Fluent extension to attach keyboard accessibility and standard design focus ring
/// to icon buttons.
pub trait FocusableIconButtonExt: gpui::StatefulInteractiveElement + Styled + Sized {
    /// Tracks `handle`, applies a 1px transparent border to prevent layout shifts,
    /// applies `border_active` and `focus_ring_shadows(t)` when focused,
    /// focuses on mouse down, and triggers `on_trigger` on click or on Enter / Space.
    fn focus_action<F>(self, handle: &FocusHandle, t: &ThemeColors, on_trigger: F) -> Self
    where
        F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    {
        let p = SemanticPalette::from_theme(t);
        let ring = focus_ring_shadows(t);
        let trigger = std::sync::Arc::new(on_trigger);
        let fh = handle.clone();

        self.track_focus(handle)
            .border_1()
            .border_color(gpui::transparent_black())
            .focus(move |s| s.border_color(p.border_active).shadow(ring))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                window.focus(&fh, cx);
            })
            .on_click(move |ev, window, cx| {
                trigger(ev, window, cx);
            })
    }
}

impl<E: gpui::StatefulInteractiveElement + Styled> FocusableIconButtonExt for E {}
