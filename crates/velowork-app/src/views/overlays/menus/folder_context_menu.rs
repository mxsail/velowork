//! Folder context menu overlay (decoupled from Workspace).

use velowork_ui::Cancel;
use velowork_ui::icon::AppIcon;
use velowork_ui::menu::{context_menu_panel, menu_item, menu_item_with_color, menu_separator};
use crate::theme::theme;
use velowork_i18n::i18n;
use gpui::prelude::*;
use gpui::*;

/// Event emitted by FolderContextMenu
pub enum FolderContextMenuEvent {
    Close,
    RenameFolder { folder_id: String, folder_name: String },
    DeleteFolder { folder_id: String },
    FilterToFolder { folder_id: String },
}

impl velowork_ui::overlay::CloseEvent for FolderContextMenuEvent {
    fn is_close(&self) -> bool { matches!(self, Self::Close) }
}

/// Folder context menu component
pub struct FolderContextMenu {
    folder_id: String,
    folder_name: String,
    position: Point<Pixels>,
    project_count: usize,
    is_active_filter: bool,
    focus_handle: FocusHandle,
    initial_focus_done: bool,
}

impl FolderContextMenu {
    pub fn new(
        folder_id: String,
        folder_name: String,
        position: Point<Pixels>,
        project_count: usize,
        is_active_filter: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        Self {
            folder_id,
            folder_name,
            position,
            project_count,
            is_active_filter,
            focus_handle,
            initial_focus_done: false,
        }
    }

    fn close(&self, cx: &mut Context<Self>) {
        cx.emit(FolderContextMenuEvent::Close);
    }

    fn rename_folder(&self, cx: &mut Context<Self>) {
        cx.emit(FolderContextMenuEvent::RenameFolder {
            folder_id: self.folder_id.clone(),
            folder_name: self.folder_name.clone(),
        });
    }

    fn delete_folder(&self, cx: &mut Context<Self>) {
        cx.emit(FolderContextMenuEvent::DeleteFolder {
            folder_id: self.folder_id.clone(),
        });
    }

    fn toggle_folder_filter(&self, cx: &mut Context<Self>) {
        cx.emit(FolderContextMenuEvent::FilterToFolder {
            folder_id: self.folder_id.clone(),
        });
    }
}

impl EventEmitter<FolderContextMenuEvent> for FolderContextMenu {}

impl Render for FolderContextMenu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);

        // Focus on first render
        if !self.initial_focus_done {
            self.initial_focus_done = true;
            window.focus(&self.focus_handle, cx);
        }

        let position = self.position;
        let project_count = self.project_count;
        let is_active_filter = self.is_active_filter;

        div()
            .track_focus(&self.focus_handle)
            .key_context("FolderContextMenu")
            .on_action(cx.listener(|this, _: &Cancel, _window, cx| {
                this.close(cx);
            }))
            .absolute()
            .inset_0()
            .id("folder-context-menu-backdrop")
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
                        context_menu_panel("folder-context-menu", &t, cx)
                        // Filter option
                        .child(
                            menu_item(
                                "folder-ctx-filter",
                                if is_active_filter { AppIcon::EyeOff } else { AppIcon::Eye },
                                if is_active_filter { i18n!(cx, "folder_context_menu.show_all") } else { i18n!(cx, "folder_context_menu.show_only") },
                                &t, cx,
                            )
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.toggle_folder_filter(cx);
                            })),
                        )
                        // Rename option
                        .child(
                            menu_item("folder-ctx-rename", AppIcon::Edit, i18n!(cx, "common.rename"), &t, cx)
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.rename_folder(cx);
                                })),
                        )
                        // Separator
                        .child(menu_separator(&t))
                        // Delete option
                        .child(
                            menu_item_with_color(
                                "folder-ctx-delete",
                                AppIcon::Trash,
                                if project_count > 0 {
                                    let template = i18n!(cx, "folder_context_menu.delete_with_count");
                                    template.replace("{count}", &project_count.to_string())
                                } else {
                                    i18n!(cx, "folder_context_menu.delete")
                                },
                                t.error,
                                t.error,
                                &t, cx,
                            )
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.delete_folder(cx);
                            })),
                        ),
                    ),
            ))
    }
}

crate::impl_focusable!(FolderContextMenu);
