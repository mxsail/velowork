//! Selectable list item component for overlay lists.

use crate::theme::{surface_bg_t, ThemeColors};
use crate::tokens::{RADIUS_STD, SPACE_LG, SPACE_MD};
use gpui::*;
use crate::behavior::{HoverBehavior, SelectedBehavior, StatefulElementBehaviorExt};

/// A list item row with selection highlight and hover state.
///
/// Used in command palette, project switcher, file search, theme selector, etc.
/// Returns a Stateful<Div> ready for `.on_mouse_down()`, `.child()`, etc.
pub fn selectable_list_item(
    id: impl Into<ElementId>,
    is_selected: bool,
    t: &ThemeColors,
) -> Stateful<Div> {
    div()
        .id(id)
        .w_full()
        .cursor_pointer()
        .flex()
        .items_center()
        .px(SPACE_LG)
        .py(SPACE_MD)
        .rounded(RADIUS_STD)
        .stateful_behavior(SelectedBehavior {
            selected: is_selected,
            bg: surface_bg_t(t.bg_hover, t),
            fg: None,
        })
        .stateful_behavior(HoverBehavior {
            hover_bg: surface_bg_t(t.bg_hover, t),
            ..Default::default()
        })
}
