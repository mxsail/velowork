//! Dialog action button row (Cancel + Confirm) with unified visual hierarchy and focus glow.

use crate::button::{button, button_primary, Button};
use crate::theme::ThemeColors;
use crate::tokens::SPACE_MD;
use gpui::*;

/// Cancel + Confirm button row for dialogs.
///
/// Right-aligned with standard gap (`SPACE_MD`). Returns a Div.
/// - Cancel button: Secondary style (`button`), focusable via `cancel_focus`.
/// - Confirm button: Primary style (`button_primary`), focusable via `confirm_focus`.
///
/// `cancel_focus` / `confirm_focus` are persistent [`FocusHandle`]s owned by
/// the calling view so the buttons stay keyboard-focusable (Tab) across
/// re-renders and respond to `Enter` / `Space`.
pub fn dialog_actions<F1, F2>(
    cancel_label: impl Into<SharedString>,
    on_cancel: F1,
    confirm_label: impl Into<SharedString>,
    on_confirm: F2,
    cancel_focus: &FocusHandle,
    confirm_focus: &FocusHandle,
    t: &ThemeColors,
) -> Div
where
    F1: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    F2: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    div()
        .flex()
        .gap(SPACE_MD)
        .items_center()
        .justify_end()
        .child(
            button("dialog-cancel-btn", cancel_label, t)
                .focus_handle(cancel_focus)
                .on_click(on_cancel),
        )
        .child(
            button_primary("dialog-confirm-btn", confirm_label, t)
                .focus_handle(confirm_focus)
                .on_click(on_confirm),
        )
}

/// Extended dialog actions row with configurable confirm button state (primary, danger, loading, disabled).
#[allow(clippy::too_many_arguments)]
pub fn dialog_actions_extended<F1, F2>(
    cancel_label: impl Into<SharedString>,
    on_cancel: F1,
    cancel_focus: &FocusHandle,
    confirm_label: impl Into<SharedString>,
    on_confirm: F2,
    confirm_focus: &FocusHandle,
    confirm_primary: bool,
    confirm_danger: bool,
    confirm_loading: bool,
    confirm_disabled: bool,
    t: &ThemeColors,
) -> Div
where
    F1: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    F2: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    let mut confirm_btn = if confirm_primary {
        button_primary("dialog-confirm-btn", confirm_label, t)
    } else {
        button("dialog-confirm-btn", confirm_label, t)
    };

    if confirm_danger {
        confirm_btn = confirm_btn.danger(true);
    }
    if confirm_loading {
        confirm_btn = confirm_btn.loading(true);
    }
    if confirm_disabled {
        confirm_btn = confirm_btn.disabled(true);
    }

    confirm_btn = confirm_btn.focus_handle(confirm_focus).on_click(on_confirm);

    div()
        .flex()
        .gap(SPACE_MD)
        .items_center()
        .justify_end()
        .child(
            button("dialog-cancel-btn", cancel_label, t)
                .focus_handle(cancel_focus)
                .on_click(on_cancel),
        )
        .child(confirm_btn)
}

/// Creates a keyboard-focusable secondary button with unified focus ring and Enter/Space support.
pub fn focusable_action_button<F>(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    on_click: F,
    focus_handle: &FocusHandle,
    t: &ThemeColors,
) -> Button
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    button(id, label, t)
        .focus_handle(focus_handle)
        .on_click(on_click)
}

/// Creates a keyboard-focusable primary button with unified focus ring and Enter/Space support.
pub fn focusable_action_button_primary<F>(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    on_click: F,
    focus_handle: &FocusHandle,
    t: &ThemeColors,
) -> Button
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    button_primary(id, label, t)
        .focus_handle(focus_handle)
        .on_click(on_click)
}

