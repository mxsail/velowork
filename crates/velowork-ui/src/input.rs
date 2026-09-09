//! Input field components.

use crate::theme::ThemeColors;
use crate::tokens::*;
use gpui::*;
use crate::design::semantic::SemanticPalette;

pub use crate::simple_input::{
    InputChangedEvent, InputFocusedEvent, KeyHandled, KeyInterceptResult, SimpleInput,
    SimpleInputState, InputEvent,
};

pub type Input = SimpleInput;
pub type InputState = SimpleInputState;
pub type Textarea = SimpleInput;
pub type TextareaState = SimpleInputState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputMode {
    Text,
    Password,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputContentType {
    Text,
    Password,
}



/// Accent-colored focus halo painted around the element bounds via an
/// outset box shadow (following Ant Design focus ring specifications).
///
/// This produces a smooth, symmetric focus glow around the 1px physical border
/// without causing subpixel rounding misalignment or layout shifts.
pub fn focus_ring_shadows(t: &ThemeColors) -> Vec<BoxShadow> {
    let p = SemanticPalette::from_theme(t);
    vec![
        BoxShadow {
            color: Hsla { a: 0.35, ..p.border_active },
            offset: point(px(0.0), px(0.0)),
            blur_radius: px(3.0),
            spread_radius: px(1.0),
            inset: false,
        },
    ]
}

/// Fluent helpers to attach the global input focus ring to any bordered
/// input container. The ring is driven by GPUI's native focus-style path:
/// the container tracks the input's `FocusHandle` and the accent ring is
/// applied automatically whenever the input owns keyboard/IME focus.
pub trait InputFocusRingExt: InteractiveElement + Styled + Sized {
    /// Track `handle` and paint the accent focus ring while it is focused.
    fn focus_ring_on(self, handle: &FocusHandle, t: &ThemeColors) -> Self {
        let p = SemanticPalette::from_theme(t);
        let ring = focus_ring_shadows(t);
        self.track_focus(handle)
            .focus(move |s| s.border_color(p.border_active).shadow(ring))
    }

    /// Convenience wrapper resolving the handle from a `SimpleInputState`.
    fn input_focus_ring(self, input: &Entity<SimpleInputState>, t: &ThemeColors, cx: &App) -> Self {
        let handle = input.read(cx).focus_handle(cx);
        self.focus_ring_on(&handle, t)
    }
}

impl<E: InteractiveElement + Styled> InputFocusRingExt for E {}



/// Labeled input field container.
///
/// Delegates to [`form_item`] for consistent design system label typography,
/// text color, and spacing.
pub fn labeled_input(label: impl Into<SharedString>, t: &ThemeColors, cx: &App) -> Stateful<Div> {
    let lbl: SharedString = label.into();
    let id_str = format!("labeled-input-{}", lbl);
    crate::form::form_item(id_str).label(lbl).render(t, cx)
}

/// Search input area with ">" prefix prompt and query/placeholder display.
pub fn search_input_area(query: &str, placeholder: &str, t: &ThemeColors, cx: &App) -> Div {
    search_input_area_impl(query, placeholder, false, t, cx)
}

/// Search input area with optional text selection highlight.
pub fn search_input_area_selected(
    query: &str,
    placeholder: &str,
    selected: bool,
    t: &ThemeColors,
    cx: &App,
) -> Div {
    search_input_area_impl(query, placeholder, selected, t, cx)
}

fn search_input_area_impl(
    query: &str,
    placeholder: &str,
    selected: bool,
    t: &ThemeColors,
    cx: &App,
) -> Div {
    let p = SemanticPalette::from_theme(t);
    let query_element: AnyElement = if !query.is_empty() && selected {
        div()
            .flex_1()
            .text_size(ui_text_xl(cx))
            .child(
                div()
                    .bg(Hsla { a: 0.3, ..p.border_active })
                    .rounded(px(2.0))
                    .text_color(p.text_primary)
                    .child(query.to_string()),
            )
            .into_any_element()
    } else {
        div()
            .flex_1()
            .text_size(ui_text_xl(cx))
            .text_color(if query.is_empty() {
                p.text_muted
            } else {
                p.text_primary
            })
            .child(if query.is_empty() {
                placeholder.to_string()
            } else {
                query.to_string()
            })
            .into_any_element()
    };

    div()
        .px(SPACE_LG)
        .py(px(10.0))
        .flex()
        .items_center()
        .gap(SPACE_MD)
        .border_b_1()
        .border_color(p.border_subtle)
        .child(
            div()
                .text_size(ui_text_xl(cx))
                .text_color(p.text_muted)
                .child(">"),
        )
        .child(query_element)
}

#[cfg(test)]
mod tests {
    use super::focus_ring_shadows;
    use crate::theme::DARK_THEME;
    use gpui::px;

    #[std::prelude::v1::test]
    fn test_focus_ring_shadows_outset_glow() {
        let t = DARK_THEME.clone();
        let shadows = focus_ring_shadows(&t);
        assert_eq!(shadows.len(), 1);

        let ring = &shadows[0];
        assert!(!ring.inset, "Focus ring must be outset (inset: false) to prevent subpixel border conflicts");
        assert_eq!(ring.blur_radius, px(3.0));
        assert_eq!(ring.spread_radius, px(1.0));
        assert_eq!(ring.offset.x, px(0.0));
        assert_eq!(ring.offset.y, px(0.0));
        assert!((ring.color.a - 0.35).abs() < 0.001, "Focus ring alpha should be 0.35 for Ant Design halo consistency");
    }
}
