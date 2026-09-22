//! Project context menu overlay (decoupled from Workspace).
//!
//! All displayed data is injected by the caller; the only write operation
//! (`toggle_pinned`) is surfaced as an event so the owning layer can persist it.

use velowork_ui::Cancel;
use velowork_ui::icon::AppIcon;
use velowork_ui::menu::{context_menu_panel, menu_item, menu_item_with_color, menu_separator};
use crate::theme::theme;
use velowork_i18n::i18n;
use gpui::prelude::*;
use gpui::*;

/// Event emitted by ContextMenu
pub enum ContextMenuEvent {
    Close,
    AddTerminal { project_id: String },
    RenameProject { project_id: String, project_name: String },
    RenameDirectory { project_id: String, project_path: String },
    DeleteProject { project_id: String },
    FocusParent { project_id: String },
    CopyPath { path: String },
    FocusProject { project_id: String },
    HideProject { project_id: String },
    /// Toggle the project's pinned state. Surfaced as an event (rather than a
    /// direct workspace write) so the owning layer can persist it.
    TogglePinned { project_id: String },
}

impl velowork_ui::overlay::CloseEvent for ContextMenuEvent {
    fn is_close(&self) -> bool { matches!(self, Self::Close) }
}

/// Project context menu component
pub struct ContextMenu {
    project_id: String,
    position: Point<Pixels>,
    project_name: String,
    project_path: String,
    is_pinned: bool,
    extras_exist: bool,
    is_hidden_in_window: bool,
    focus_handle: FocusHandle,
    initial_focus_done: bool,
}

impl ContextMenu {
    pub fn new(
        project_id: String,
        position: Point<Pixels>,
        project_name: String,
        project_path: String,
        is_pinned: bool,
        extras_exist: bool,
        is_hidden_in_window: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        Self {
            project_id,
            position,
            project_name,
            project_path,
            is_pinned,
            extras_exist,
            is_hidden_in_window,
            focus_handle,
            initial_focus_done: false,
        }
    }

    fn close(&self, cx: &mut Context<Self>) {
        cx.emit(ContextMenuEvent::Close);
    }

    fn add_terminal(&self, cx: &mut Context<Self>) {
        cx.emit(ContextMenuEvent::AddTerminal {
            project_id: self.project_id.clone(),
        });
    }

    fn rename_project(&self, project_name: String, cx: &mut Context<Self>) {
        cx.emit(ContextMenuEvent::RenameProject {
            project_id: self.project_id.clone(),
            project_name,
        });
    }

    fn rename_directory(&self, project_path: String, cx: &mut Context<Self>) {
        cx.emit(ContextMenuEvent::RenameDirectory {
            project_id: self.project_id.clone(),
            project_path,
        });
    }

    fn delete_project(&self, cx: &mut Context<Self>) {
        cx.emit(ContextMenuEvent::DeleteProject {
            project_id: self.project_id.clone(),
        });
    }

    fn toggle_pinned(&self, cx: &mut Context<Self>) {
        cx.emit(ContextMenuEvent::TogglePinned {
            project_id: self.project_id.clone(),
        });
    }

    fn copy_path(&self, path: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(path));
        cx.emit(ContextMenuEvent::Close);
    }

    fn focus_project(&self, cx: &mut Context<Self>) {
        cx.emit(ContextMenuEvent::FocusProject {
            project_id: self.project_id.clone(),
        });
    }

    fn hide_project(&self, cx: &mut Context<Self>) {
        cx.emit(ContextMenuEvent::HideProject {
            project_id: self.project_id.clone(),
        });
    }
}

impl EventEmitter<ContextMenuEvent> for ContextMenu {}

impl Render for ContextMenu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);

        // Focus on first render
        if !self.initial_focus_done {
            self.initial_focus_done = true;
            window.focus(&self.focus_handle, cx);
        }

        let position = self.position;
        let project_path = self.project_path.clone();
        let project_name = self.project_name.clone();
        let is_pinned = self.is_pinned;
        let extras_exist = self.extras_exist;
        let is_hidden_in_window = self.is_hidden_in_window;

        let hide_project_label = match (extras_exist, is_hidden_in_window) {
            (false, false) => i18n!(cx, "context_menu.hide_project"),
            (false, true) => i18n!(cx, "context_menu.show_project"),
            (true, false) => i18n!(cx, "context_menu.hide_from_window"),
            (true, true) => i18n!(cx, "context_menu.show_in_window"),
        };
        let hide_project_icon = if is_hidden_in_window {
            AppIcon::Eye
        } else {
            AppIcon::EyeOff
        };

        div()
            .track_focus(&self.focus_handle)
            .key_context("ContextMenu")
            .on_action(cx.listener(|this, _: &Cancel, _window, cx| {
                this.close(cx);
            }))
            .absolute()
            .inset_0()
            .occlude()
            .id("context-menu-backdrop")
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _window, cx| {
                this.close(cx);
            }))
            .on_mouse_down(MouseButton::Right, cx.listener(|this, _, _window, cx| {
                this.close(cx);
            }))
            .child(deferred(
                anchored()
                    .position(position)
                    .snap_to_window()
                    .child(
                        context_menu_panel("project-context-menu", &t, cx)
                        // Add Terminal option
                        .child(
                            menu_item("context-menu-add-terminal", AppIcon::Plus, i18n!(cx, "context_menu.add_terminal"), &t, cx)
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.add_terminal(cx);
                                })),
                        )
                        .child(menu_separator(&t))
                        // Copy Path
                        .child(
                            menu_item("context-menu-copy-path", AppIcon::Copy, i18n!(cx, "context_menu.copy_path"), &t, cx)
                                .on_click(cx.listener({
                                    let project_path = project_path.clone();
                                    move |this, _, _window, cx| {
                                        this.copy_path(project_path.clone(), cx);
                                    }
                                })),
                        )
                        // Focus Project
                        .child(
                            menu_item("context-menu-focus-project", AppIcon::Fullscreen, i18n!(cx, "context_menu.focus_project"), &t, cx)
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.focus_project(cx);
                                })),
                        )
                        // Pin / Unpin
                        .child(
                            menu_item(
                                "context-menu-toggle-pinned",
                                AppIcon::Bookmark,
                                if is_pinned { i18n!(cx, "context_menu.unpin") } else { i18n!(cx, "context_menu.pin") },
                                &t, cx,
                            )
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.toggle_pinned(cx);
                            })),
                        )
                        // Hide / Show Project
                        .child(
                            menu_item("context-menu-hide-project", hide_project_icon, hide_project_label, &t, cx)
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.hide_project(cx);
                                })),
                        )
                        .child(menu_separator(&t))
                        // Rename option
                        .child(
                            menu_item("context-menu-rename", AppIcon::Edit, i18n!(cx, "common.action.rename"), &t, cx)
                                .on_click(cx.listener({
                                    let project_name = project_name.clone();
                                    move |this, _, _window, cx| {
                                        this.rename_project(project_name.clone(), cx);
                                    }
                                })),
                        )
                        // Rename Directory option
                        .child(
                            menu_item("context-menu-rename-dir", AppIcon::Folder, i18n!(cx, "context_menu.rename_directory"), &t, cx)
                                .on_click(cx.listener({
                                    let project_path = project_path.clone();
                                    move |this, _, _window, cx| {
                                        this.rename_directory(project_path.clone(), cx);
                                    }
                                })),
                        )
                        // Delete option
                        .child(
                            menu_item_with_color("context-menu-delete", AppIcon::Trash, i18n!(cx, "common.action.delete"), t.error, t.error, &t, cx)
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.delete_project(cx);
                                })),
                        ),
                    ),
            ))
    }
}

crate::impl_focusable!(ContextMenu);
