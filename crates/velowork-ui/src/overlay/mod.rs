//! Overlay helpers for Velowork UI components.

mod animated_modal;
mod legacy;
mod modal;

pub use animated_modal::*;
pub use legacy::*;
pub use modal::{
    chrome_button, modal_body, modal_frame, ActionKind, BadgeTone, DetachAffordance, ModalAction,
    ModalActions, ModalBadge, ModalFooter, ModalHeader, OverlayAction, WindowCornerRadius,
    fullscreen_overlay, fullscreen_panel, window_drag_spacer, detached_needs_controls,
    window_min_max_controls, modal_backdrop, modal_content, modal_header,
};

pub use crate::overlay_registry::{ClosePolicy, OverlayId, OverlayInfo, OverlayRegistry};

/// Unified result of a modal / overlay interaction.
#[derive(Clone, Debug, PartialEq)]
pub enum ModalResult<T = ()> {
    /// The primary action succeeded, carrying its payload.
    Confirm(T),
    /// The user cancelled (Cancel button / Escape on a form).
    Cancel,
    /// The user dismissed via ✕ / click-outside (no intent to confirm).
    Close,
    /// The overlay auto-dismissed (e.g. a toast timeout).
    Timeout,
}
