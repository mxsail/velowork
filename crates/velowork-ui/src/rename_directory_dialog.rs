//! Dialog for renaming a project's directory on disk (decoupled from Workspace).
//!
//! The on-disk rename is performed here (pure `std`); the resulting new path and
//! name are surfaced via [`RenameDirectoryDialogEvent::Renamed`] so the owning
//! layer can update persisted state.

use crate::design::semantic::SemanticPalette;
use crate::dialog_actions::dialog_actions;
use crate::focus_group::{FocusGroup, FocusGroupExt};
use crate::icon::AppIcon;
use crate::Cancel;
use crate::h_flex;
use crate::input::{Input, InputState};
use crate::overlay::modal_content;
use crate::theme::theme;
use crate::tokens::{ui_text_md, ui_text_ms, ui_text_xl, SPACE_LG, SPACE_MD, SPACE_XL, RADIUS_STD, ICON_STD};
use velowork_i18n::i18n;
use crate::behavior::{HoverBehavior, StatefulElementBehaviorExt};
use gpui::prelude::*;
use gpui::*;
use std::path::Path;

/// Events emitted by the rename directory dialog
#[derive(Clone)]
pub enum RenameDirectoryDialogEvent {
    /// Dialog closed (cancelled)
    Close,
    /// Directory was successfully renamed on disk
    Renamed { project_id: String, new_path: String, new_name: String },
}

impl crate::overlay::CloseEvent for RenameDirectoryDialogEvent {
    fn is_close(&self) -> bool { matches!(self, Self::Close | Self::Renamed { .. }) }
}

impl EventEmitter<RenameDirectoryDialogEvent> for RenameDirectoryDialog {}

/// Dialog for renaming a project's directory on disk.
pub struct RenameDirectoryDialog {
    project_id: String,
    project_path: String,
    current_name: String,
    name_input: Option<Entity<InputState>>,
    error_message: Option<String>,
    focus_handle: FocusHandle,
    cancel_focus: FocusHandle,
    confirm_focus: FocusHandle,
    initialized: bool,
}

impl RenameDirectoryDialog {
    pub fn new(
        project_id: String,
        project_path: String,
        cx: &mut Context<Self>,
    ) -> Self {
        let current_name = Path::new(&project_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        Self {
            project_id,
            project_path,
            current_name,
            name_input: None,
            error_message: None,
            focus_handle: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            confirm_focus: cx.focus_handle(),
            initialized: false,
        }
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        cx.emit(RenameDirectoryDialogEvent::Close);
    }

    fn confirm(&mut self, cx: &mut Context<Self>) {
        let new_name = self
            .name_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string())
            .unwrap_or_default()
            .trim()
            .to_string();

        if new_name.is_empty() {
            self.error_message = Some("Directory name cannot be empty".to_string());
            cx.notify();
            return;
        }

        if new_name.contains('/') || new_name.contains('\\') {
            self.error_message = Some("Directory name cannot contain path separators".to_string());
            cx.notify();
            return;
        }

        let old_path = Path::new(&self.project_path);
        let current_name = old_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        if new_name == current_name {
            self.error_message = Some("Name is the same as current directory".to_string());
            cx.notify();
            return;
        }

        let new_path = match old_path.parent() {
            Some(parent) => parent.join(&new_name),
            None => {
                self.error_message = Some("Cannot determine parent directory".to_string());
                cx.notify();
                return;
            }
        };

        if new_path.exists() {
            self.error_message = Some(format!("'{}' already exists", new_name));
            cx.notify();
            return;
        }

        if let Err(e) = std::fs::rename(&self.project_path, &new_path) {
            self.error_message = Some(format!("Failed to rename: {}", e));
            cx.notify();
            return;
        }

        let new_path_str = new_path.to_string_lossy().to_string();
        let project_id = self.project_id.clone();
        let new_name_clone = new_name.clone();
        cx.emit(RenameDirectoryDialogEvent::Renamed {
            project_id,
            new_path: new_path_str,
            new_name: new_name_clone,
        });
    }
}

crate::impl_focusable!(RenameDirectoryDialog);

impl Render for RenameDirectoryDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);
        let focus_handle = self.focus_handle.clone();

        let name_input = self.name_input.get_or_insert_with(|| {
            let current_name = self.current_name.clone();
            cx.new(|cx| {
                InputState::new(cx)
                    .placeholder(i18n!(cx, "dialog.directory_name_placeholder"))
                    .default_value(current_name)
                    .pass_enter(true)
            })
        });

        if !self.initialized {
            self.initialized = true;
            name_input.update(cx, |input, cx| {
                input.focus(window, cx);
            });
        }

        let name_input_entity = name_input.clone();
        let error_msg = self.error_message.clone();
        let path_display = self.project_path.clone();

        let focus_group = FocusGroup::new();
        focus_group.add(name_input_entity.read(cx).focus_handle(cx));
        focus_group.add(self.cancel_focus.clone());
        focus_group.add(self.confirm_focus.clone());

        modal_content("rename-dir-dialog", cx)
            .w(px(420.0))
            .track_focus(&focus_handle)
            .key_context("RenameDirectoryDialog")
            .tab_cycle(&focus_group)
            .on_action(cx.listener(|this, _: &Cancel, _, cx| {
                this.close(cx);
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key.as_str() == "enter" {
                    this.confirm(cx);
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .child(
                        div()
                            .px(SPACE_XL)
                            .py(SPACE_LG)
                            .flex()
                            .items_center()
                            .justify_between()
                            .border_b_1()
                            .border_color(p.border_subtle)
                            .child(
                                h_flex()
                                    .gap(SPACE_MD)
                                    .child(
                                        AppIcon::Folder
                                            .size(px(16.0))
                                            .text_color(p.border_active),
                                    )
                                    .child(
                                        div()
                                            .text_size(ui_text_xl(cx))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(p.text_primary)
                                            .child(i18n!(cx, "dialog.rename_directory")),
                                    ),
                            )
                            .child(
                                div()
                                    .id("close-rename-dir-btn")
                                    .cursor_pointer()
                                    .w(px(24.0))
                                    .h(px(24.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(RADIUS_STD)
                                    .stateful_behavior(HoverBehavior { hover_bg: rgb(t.bg_hover).into(), ..Default::default() })
                                    .child(
                                        AppIcon::Close
                                            .size(ICON_STD)
                                            .text_color(p.text_secondary),
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.close(cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .px(SPACE_XL)
                            .py(SPACE_LG)
                            .flex()
                            .flex_col()
                            .gap(SPACE_MD)
                            .child(
                                div()
                                    .text_size(ui_text_ms(cx))
                                    .text_color(p.text_muted)
                                    .overflow_x_hidden()
                                    .whitespace_nowrap()
                                    .child(path_display),
                            )
                            .child(Input::new(&name_input_entity)),
                    )
                    .when_some(error_msg, |d, msg| {
                        d.child(
                            div()
                                .px(SPACE_XL)
                                .py(SPACE_MD)
                                .bg(rgba(0xff00001a))
                                .text_size(ui_text_md(cx))
                                .text_color(p.status_error)
                                .child(msg),
                        )
                    })
                    .child(
                        h_flex()
                            .h(px(52.0))
                            .flex_shrink_0()
                            .px(SPACE_LG)
                            .items_center()
                            .justify_end()
                            .border_t_1()
                            .border_color(p.border_subtle)
                            .child(dialog_actions(
                                &i18n!(cx, "common.cancel"),
                                cx.listener(|this, _, _, cx| this.close(cx)),
                                &i18n!(cx, "common.rename"),
                                cx.listener(|this, _, _, cx| this.confirm(cx)),
                                &self.cancel_focus,
                                &self.confirm_focus,
                                &t,
                            )),
                    )
    }
}
