use gpui::{FocusHandle, InteractiveElement, MouseButton, Styled};

/// Implement the `Focusable` trait for a type that has a `focus_handle` field.
///
/// This macro generates a standard `Focusable` implementation that returns
/// a clone of the `focus_handle` field.
///
/// # Example
///
/// ```rust,ignore
/// pub struct MyView {
///     focus_handle: FocusHandle,
/// }
///
/// velowork_ui::impl_focusable!(MyView);
/// ```
#[macro_export]
macro_rules! impl_focusable {
    ($type:ty) => {
        impl gpui::Focusable for $type {
            fn focus_handle(&self, _cx: &gpui::App) -> gpui::FocusHandle {
                self.focus_handle.clone()
            }
        }
    };
}

/// Extension trait for scope background surface elements.
///
/// Clicking a blank surface node initiates a natural GPUI Focus Transition
/// to the scope root (`scope_handle`), gracefully triggering `FocusOutEvent`
/// on any previously active child input.
pub trait FocusSurfaceExt: InteractiveElement + Styled + Sized {
    /// Transfers focus to `scope_handle` when clicking directly on this background surface.
    fn focus_scope_on_click(self, scope_handle: &FocusHandle) -> Self {
        let h = scope_handle.clone();
        self.on_mouse_down(MouseButton::Left, move |_, window, cx| {
            window.focus(&h, cx);
        })
    }
}

impl<E: InteractiveElement + Styled> FocusSurfaceExt for E {}
