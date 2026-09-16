//! Confirmation dialog (modal) managed via the centralized OverlayRegistry.
//!
//! Renders a centered modal with a title, message and Cancel/Confirm buttons.
//! The dialog registers itself with `OverlayRegistry` (ClosePolicy::ClickOutside)
//! so clicking the backdrop dismisses it exactly like every other overlay in the
//! app. All text uses the shared `ui_text` tokens so the font size stays
//! consistent with the rest of the UI.

use gpui::prelude::*;
use gpui::*;
use crate::h_flex;
use crate::overlay::modal_content;
use crate::scrollable::ScrollableElement;
use crate::overlay_registry::{ClosePolicy, OverlayInfo, OverlayRegistry};
use crate::design::semantic::SemanticPalette;
use crate::theme::theme;
use crate::tokens::{ui_text, ui_text_md, SPACE_MD};
use crate::button::button;
use std::sync::Arc;

/// Events emitted by [`ConfirmDialog`] so the owner can react to the outcome.
#[derive(Clone)]
pub enum ConfirmDialogEvent {
    /// User pressed the Confirm button (or its equivalent).
    Confirmed { checkbox_checked: bool },
    /// User cancelled: Cancel button, Escape, or a click outside the dialog.
    Cancelled,
}

/// Control that currently has keyboard focus in the confirmation dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmDialogControl {
    Cancel,
    Confirm,
    Checkbox,
}

/// Middle-truncates a string with `***` if its character count exceeds `max_chars`.
pub fn truncate_middle(s: &str, max_chars: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_chars {
        return s.to_string();
    }

    let ellipsis = "***";
    let ellipsis_len = 3;
    if max_chars <= ellipsis_len + 2 {
        return s.chars().take(max_chars).collect();
    }

    let keep = max_chars - ellipsis_len;
    let prefix_len = (keep + 2) / 2;
    let suffix_len = keep.saturating_sub(prefix_len);

    let prefix: String = s.chars().take(prefix_len).collect();
    let suffix: String = s.chars().skip(char_count - suffix_len).collect();

    format!("{}{}{}", prefix, ellipsis, suffix)
}

/// Formats a confirmation dialog message:
/// 1. Preserves existing quotes around the target object name (e.g. `「...」`, `“...”`, `"..."`).
/// 2. If no quotes exist, quotes ONLY the object name using `「...」` when matching prompt patterns.
/// 3. If the object name is short (<= 16 chars), keeps it entirely in full (no truncation).
/// 4. If the object name is long (> 16 chars), middle-truncates the name inside quotes with `***`.
/// 5. Never wraps the overall prompt sentence in outer quotes.
pub fn format_confirm_message(raw_msg: &str) -> String {
    let message = raw_msg.replace("\r\n", " ").replace('\n', " ");

    // Check if the message already contains quoted object names like 「...」, “...”, "...", etc.
    let quote_pairs = [
        ('「', '」'),
        ('“', '”'),
        ('『', '』'),
        ('"', '"'),
        ('\'', '\''),
        ('`', '`'),
    ];

    for (open, close) in quote_pairs {
        if let Some(start) = message.find(open) {
            let after_open = start + open.len_utf8();
            if let Some(end_rel) = message[after_open..].find(close) {
                let end = after_open + end_rel;
                let prefix = &message[..start];
                let target_name = &message[after_open..end];
                let suffix = &message[end + close.len_utf8()..];

                let formatted_target = if target_name.chars().count() > 16 {
                    truncate_middle(target_name, 13)
                } else {
                    target_name.to_string()
                };

                return format!("{}{}{}{}{}", prefix, open, formatted_target, close, suffix);
            }
        }
    }

    // No quotes found. Try extracting the target object name from common prompt patterns.
    if let Some((prefix, rest)) = message.split_once("确定要删除项目 ") {
        if let Some((target, suffix)) = rest.split_once(" 吗？").or_else(|| rest.split_once("吗？")) {
            let target_trim = target.trim();
            let formatted_target = if target_trim.chars().count() > 16 {
                truncate_middle(target_trim, 13)
            } else {
                target_trim.to_string()
            };
            return format!("{}确定要删除项目「{}」吗？{}", prefix, formatted_target, suffix);
        }
    } else if let Some((prefix, rest)) = message.split_once("确定要删除 ")
        && let Some((target, suffix)) = rest.split_once(" 吗？").or_else(|| rest.split_once("吗？"))
    {
        let target_trim = target.trim();
        let formatted_target = if target_trim.chars().count() > 16 {
            truncate_middle(target_trim, 13)
        } else {
            target_trim.to_string()
        };
        return format!("{}确定要删除「{}」吗？{}", prefix, formatted_target, suffix);
    }

    // Fallback: Return message as is without wrapping the whole sentence in quotes.
    message
}

/// Selected button in the confirmation dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmDialogButton {
    Cancel,
    Confirm,
}

pub struct ConfirmDialog {
    title: String,
    message: String,
    confirm_label: String,
    cancel_label: String,
    confirm_danger: bool,
    current_focus: ConfirmDialogControl,
    checkbox_label: Option<SharedString>,
    checkbox_checked: bool,
    previous_focus_handle: Option<FocusHandle>,
    overlay_registry: Option<WeakEntity<OverlayRegistry>>,
    overlay_id: SharedString,
    dismissed: bool,
    focus_handle: FocusHandle,
}

impl EventEmitter<ConfirmDialogEvent> for ConfirmDialog {}

impl ConfirmDialog {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cx: &mut Context<Self>,
        title: impl Into<String>,
        message: impl Into<String>,
        confirm_label: impl Into<String>,
        cancel_label: impl Into<String>,
        confirm_danger: bool,
        overlay_registry: Option<Entity<OverlayRegistry>>,
        overlay_id: impl Into<SharedString>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        let overlay_id: SharedString = overlay_id.into();
        let reg_entity = overlay_registry.or_else(|| OverlayRegistry::global(cx));
        let overlay_registry = reg_entity.as_ref().map(|e| e.downgrade());

        // Register with the centralized OverlayRegistry so a click on the
        // backdrop (outside the content box) dismisses the dialog, exactly like
        // context menus and other overlays.
        if let Some(reg) = overlay_registry.clone().and_then(|w| w.upgrade()) {
            let weak = cx.entity().downgrade();
            let close = Arc::new(move |window: &mut Window, cx: &mut App| {
                if let Some(entity) = weak.upgrade() {
                    entity.update(cx, |this, cx| {
                        this.dismiss(Some(window), cx);
                    });
                }
            });
            reg.update(cx, |r, _| {
                r.register(
                    OverlayInfo {
                        id: overlay_id.clone(),
                        bounds: Bounds::default(),
                        secondary_bounds: None,
                        close_policy: ClosePolicy::ClickOutside,
                        z_index: 2000,
                    },
                    close,
                )
            });
        }

        let raw_msg: String = message.into();
        let formatted_message = format_confirm_message(&raw_msg);

        Self {
            title: title.into(),
            message: formatted_message,
            confirm_label: confirm_label.into(),
            cancel_label: cancel_label.into(),
            confirm_danger,
            current_focus: ConfirmDialogControl::Cancel,
            checkbox_label: None,
            checkbox_checked: false,
            previous_focus_handle: None,
            overlay_registry,
            overlay_id,
            dismissed: false,
            focus_handle,
        }
    }

    /// Add an optional checkbox (e.g. "Don't ask again") to the confirmation dialog.
    pub fn checkbox(mut self, label: impl Into<SharedString>, default_checked: bool) -> Self {
        self.checkbox_label = Some(label.into());
        self.checkbox_checked = default_checked;
        self
    }

    /// Set the default focused button (defaults to `Cancel`).
    pub fn default_button(mut self, button: ConfirmDialogButton) -> Self {
        self.current_focus = match button {
            ConfirmDialogButton::Cancel => ConfirmDialogControl::Cancel,
            ConfirmDialogButton::Confirm => ConfirmDialogControl::Confirm,
        };
        self
    }

    /// Whether the checkbox was checked.
    pub fn is_checkbox_checked(&self) -> bool {
        self.checkbox_checked
    }

    /// Compute the next focused control when pressing Tab or arrow keys.
    pub fn next_control(&self, forward: bool) -> ConfirmDialogControl {
        let has_checkbox = self.checkbox_label.is_some();
        Self::compute_next_control(self.current_focus, forward, has_checkbox)
    }

    /// Compute next control purely based on current state.
    pub fn compute_next_control(
        current: ConfirmDialogControl,
        forward: bool,
        has_checkbox: bool,
    ) -> ConfirmDialogControl {
        match (current, forward, has_checkbox) {
            // Forward (Tab, Right, Down)
            (ConfirmDialogControl::Cancel, true, false) => ConfirmDialogControl::Confirm,
            (ConfirmDialogControl::Confirm, true, false) => ConfirmDialogControl::Cancel,

            (ConfirmDialogControl::Cancel, true, true) => ConfirmDialogControl::Confirm,
            (ConfirmDialogControl::Confirm, true, true) => ConfirmDialogControl::Checkbox,
            (ConfirmDialogControl::Checkbox, true, true) => ConfirmDialogControl::Cancel,

            // Backward (Shift+Tab, Left, Up)
            (ConfirmDialogControl::Cancel, false, false) => ConfirmDialogControl::Confirm,
            (ConfirmDialogControl::Confirm, false, false) => ConfirmDialogControl::Cancel,

            (ConfirmDialogControl::Cancel, false, true) => ConfirmDialogControl::Checkbox,
            (ConfirmDialogControl::Checkbox, false, true) => ConfirmDialogControl::Confirm,
            (ConfirmDialogControl::Confirm, false, true) => ConfirmDialogControl::Cancel,

            (ConfirmDialogControl::Checkbox, _, false) => ConfirmDialogControl::Cancel,
        }
    }

    /// Set trigger origin for origin-aware animation (no-op for compatibility).
    pub fn with_origin(self, _origin: Option<Point<Pixels>>, _cx: &mut Context<Self>) -> Self {
        self
    }

    /// Set the previous focus handle to restore when this dialog closes.
    pub fn previous_focus_handle(mut self, handle: Option<FocusHandle>) -> Self {
        self.previous_focus_handle = handle;
        self
    }

    /// Unregister from the OverlayRegistry. Safe to call outside of a registry
    /// `update` (i.e. from the dialog's own button / key handlers).
    fn unregister_from_registry(&self, cx: &mut App) {
        if let Some(reg) = self.overlay_registry.as_ref().and_then(|w| w.upgrade()) {
            reg.update(cx, |r, _| r.unregister(&self.overlay_id));
        }
    }

    fn restore_previous_focus(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        if let Some(window) = window
            && let Some(ref fh) = self.previous_focus_handle
        {
            window.focus(fh, cx);
        }
    }

    fn finish_close(&mut self, event: ConfirmDialogEvent, window: Option<&mut Window>, cx: &mut Context<Self>) {
        if !self.dismissed {
            self.dismissed = true;
            self.unregister_from_registry(cx);
            self.restore_previous_focus(window, cx);
            cx.emit(event);
        }
        cx.notify();
    }

    /// Dismiss without touching the registry immediately.
    fn dismiss(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        if let Some(w) = window {
            self.restore_previous_focus(Some(w), cx);
        }
        self.finish_close(ConfirmDialogEvent::Cancelled, None, cx);
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_close(
            ConfirmDialogEvent::Confirmed {
                checkbox_checked: self.checkbox_checked,
            },
            Some(window),
            cx,
        );
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss(Some(window), cx);
    }
}

impl Focusable for ConfirmDialog {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ConfirmDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);

        // Auto-focus on open and capture previous focus handle
        if !self.focus_handle.is_focused(window) {
            if self.previous_focus_handle.is_none()
                && let Some(focused) = window.focused(cx)
                && focused != self.focus_handle
            {
                self.previous_focus_handle = Some(focused);
            }
            window.focus(&self.focus_handle, cx);
        }

        // Keep the OverlayRegistry bounds in sync with the rendered content box
        // so click-outside dismissal is accurate.
        let registry_for_bounds = self.overlay_registry.clone();
        let id_for_bounds = self.overlay_id.clone();
        let bounds_setter = move |bounds: Bounds<Pixels>, _window: &mut Window, cx: &mut App| {
            if let Some(reg) = registry_for_bounds.as_ref().and_then(|w| w.upgrade()) {
                reg.update(cx, |r, _| r.set_bounds(&id_for_bounds, bounds));
            }
        };

        let confirm_label = self.confirm_label.clone();
        let cancel_label = self.cancel_label.clone();
        let confirm_danger = self.confirm_danger;

        modal_content("confirm-dialog-content", cx)
            .id("confirm-dialog-content")
            .w(px(420.0))
            .flex()
            .flex_col()
            .overflow_hidden()
            .track_focus(&self.focus_handle)
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        match event.keystroke.key.as_str() {
                            "escape" => {
                                cx.stop_propagation();
                                this.cancel(window, cx);
                            }
                            "tab" => {
                                cx.stop_propagation();
                                let forward = !event.keystroke.modifiers.shift;
                                this.current_focus = this.next_control(forward);
                                cx.notify();
                            }
                            "right" | "down" => {
                                cx.stop_propagation();
                                this.current_focus = this.next_control(true);
                                cx.notify();
                            }
                            "left" | "up" => {
                                cx.stop_propagation();
                                this.current_focus = this.next_control(false);
                                cx.notify();
                            }
                            "space" => {
                                if this.current_focus == ConfirmDialogControl::Checkbox {
                                    cx.stop_propagation();
                                    this.checkbox_checked = !this.checkbox_checked;
                                    cx.notify();
                                }
                            }
                            "enter" => {
                                cx.stop_propagation();
                                match this.current_focus {
                                    ConfirmDialogControl::Cancel => this.cancel(window, cx),
                                    ConfirmDialogControl::Confirm | ConfirmDialogControl::Checkbox => {
                                        this.confirm(window, cx);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }))
                    // Header: title
                    .child(
                        div()
                            .px(px(20.0))
                            .pt(px(20.0))
                            .text_size(ui_text(15.0, cx))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text_primary)
                            .child(self.title.clone()),
                    )
                    // Message area: compact natural vertical rhythm
                    .child(
                        div()
                            .id("confirm-dialog-body")
                            .w_full()
                            .px(px(20.0))
                            .pt(px(10.0))
                            .pb(if self.checkbox_label.is_some() { px(12.0) } else { px(20.0) })
                            .text_size(ui_text_md(cx))
                            .text_color(p.text_secondary)
                            .max_h(px(240.0))
                            .overflow_y_scrollbar()
                            .child(self.message.clone()),
                    )
                    // Optional Checkbox
                    .when_some(self.checkbox_label.clone(), |el, label| {
                        let entity = cx.entity().clone();
                        let is_checked = self.checkbox_checked;
                        let is_focused = self.current_focus == ConfirmDialogControl::Checkbox;
                        el.child(
                            div()
                                .id("confirm-dialog-checkbox-row")
                                .w_full()
                                .px(px(20.0))
                                .pb(px(16.0))
                                .child(
                                    crate::checkbox::Checkbox::new("confirm-dialog-checkbox")
                                        .label(label)
                                        .checked(is_checked)
                                        .focused(is_focused)
                                        .on_click(move |new_checked, _window, cx| {
                                            entity.update(cx, |this, cx| {
                                                this.checkbox_checked = *new_checked;
                                                this.current_focus = ConfirmDialogControl::Checkbox;
                                                cx.notify();
                                            });
                                        }),
                                ),
                        )
                    })
                    // Actions: right-aligned buttons at the bottom-right corner without dividing line
                    .child(
                        h_flex()
                            .w_full()
                            .gap(SPACE_MD)
                            .items_center()
                            .justify_end()
                            .px(px(20.0))
                            .pb(px(20.0))
                            .child(
                                button("confirm-dialog-cancel", cancel_label, &t)
                                    .selected(self.current_focus == ConfirmDialogControl::Cancel)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.cancel(window, cx);
                                    })),
                            )
                            .child({
                                let mut btn =
                                    button("confirm-dialog-confirm", confirm_label, &t)
                                        .selected(self.current_focus == ConfirmDialogControl::Confirm);
                                if confirm_danger {
                                    btn = btn.danger(true);
                                } else {
                                    btn = btn.primary();
                                }
                                btn.on_click(cx.listener(|this, _, window, cx| {
                                    this.confirm(window, cx);
                                }))
                            }),
                    )
                    .child(
                        canvas(bounds_setter, |_, _, _, _| {})
                            .absolute()
                            .inset_0(),
                    )
    }
}

#[cfg(test)]
mod tests {
    use super::{format_confirm_message, truncate_middle};

    #[test]
    fn test_truncate_middle() {
        let input = "查看指定目录下的文件的大小路径权限和其他相关信息";
        let truncated = truncate_middle(input, 13);
        assert_eq!(truncated, "查看指定目录***相关信息");
    }

    #[test]
    fn test_format_confirm_message_corner_bracket_short() {
        let input = "确定要删除「demo」吗？此操作无法撤销。";
        let formatted = format_confirm_message(input);
        assert_eq!(formatted, "确定要删除「demo」吗？此操作无法撤销。");
        assert!(!formatted.contains("***"));
    }

    #[test]
    fn test_format_confirm_message_corner_bracket_long() {
        let input = "确定要删除「查看指定目录下的文件的大小路径权限和其他相关信息」吗？此操作无法撤销。";
        let formatted = format_confirm_message(input);
        assert_eq!(
            formatted,
            "确定要删除「查看指定目录***相关信息」吗？此操作无法撤销。"
        );
    }

    #[test]
    fn test_format_confirm_message_unquoted_short() {
        let input = "确定要删除项目 demo 吗？";
        let formatted = format_confirm_message(input);
        assert_eq!(formatted, "确定要删除项目「demo」吗？");
        assert!(!formatted.contains("***"));
    }

    #[test]
    fn test_format_confirm_message_multi_item_no_extra_quotes() {
        let input = "确定要删除选中的 3 个快捷指令吗？此操作无法撤销。";
        let formatted = format_confirm_message(input);
        assert_eq!(formatted, "确定要删除选中的 3 个快捷指令吗？此操作无法撤销。");
    }

    #[test]
    fn test_confirm_dialog_control_navigation_without_checkbox() {
        use super::{ConfirmDialog, ConfirmDialogControl};

        // Forward cycle (Tab)
        let mut focus = ConfirmDialogControl::Cancel;
        focus = ConfirmDialog::compute_next_control(focus, true, false);
        assert_eq!(focus, ConfirmDialogControl::Confirm);

        focus = ConfirmDialog::compute_next_control(focus, true, false);
        assert_eq!(focus, ConfirmDialogControl::Cancel);

        // Backward cycle (Shift+Tab)
        focus = ConfirmDialog::compute_next_control(focus, false, false);
        assert_eq!(focus, ConfirmDialogControl::Confirm);
    }

    #[test]
    fn test_confirm_dialog_control_navigation_with_checkbox() {
        use super::{ConfirmDialog, ConfirmDialogControl};

        // Forward cycle from Confirm: Confirm -> Checkbox -> Cancel -> Confirm
        let mut focus = ConfirmDialogControl::Confirm;

        focus = ConfirmDialog::compute_next_control(focus, true, true);
        assert_eq!(focus, ConfirmDialogControl::Checkbox);

        focus = ConfirmDialog::compute_next_control(focus, true, true);
        assert_eq!(focus, ConfirmDialogControl::Cancel);

        focus = ConfirmDialog::compute_next_control(focus, true, true);
        assert_eq!(focus, ConfirmDialogControl::Confirm);

        // Backward cycle (Shift+Tab): Confirm -> Cancel -> Checkbox -> Confirm
        focus = ConfirmDialog::compute_next_control(focus, false, true);
        assert_eq!(focus, ConfirmDialogControl::Cancel);

        focus = ConfirmDialog::compute_next_control(focus, false, true);
        assert_eq!(focus, ConfirmDialogControl::Checkbox);

        focus = ConfirmDialog::compute_next_control(focus, false, true);
        assert_eq!(focus, ConfirmDialogControl::Confirm);
    }
}
