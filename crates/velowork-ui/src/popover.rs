//! Reusable popover components for anchored floating panels.
//!
//! Provides anchored positioning and styled panel container.
//! Uses GPUI's `deferred(anchored().position().snap_to_window())` pattern.

use crate::theme::{surface_bg_t, ThemeColors};
use crate::tokens::{RADIUS_MD, SPACE_MD};
use gpui::*;

/// Styled popover panel container with standard look: bg, border, rounded corners, shadow.
///
/// Stops mouse-down and scroll-wheel propagation to prevent interaction with elements underneath.
pub fn popover_panel(id: impl Into<SharedString>, t: &ThemeColors) -> Stateful<Div> {
    div()
        .id(ElementId::Name(id.into()))
        .occlude()
        .bg(surface_bg_t(t.bg_secondary, t))
        .border_1()
        .border_color(rgb(t.border))
        .rounded(RADIUS_MD)
        .shadow_lg()
        .p(SPACE_MD)
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        .on_scroll_wheel(|_, _, cx| {
            cx.stop_propagation();
        })
}
