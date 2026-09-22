//! Right-panel quick-command tree context menu ("快捷指令" right-click).

use gpui::*;
use std::sync::Arc;
use velowork_i18n::i18n;
use velowork_ui::icon::AppIcon;
use velowork_ui::menu::{ContextMenu, PopupMenu, PopupMenuItem};
use velowork_ui::overlay::OverlayRegistry;
use velowork_workspace::quick_commands::QuickCommandNode;

/// What the context menu was opened on.
pub enum QuickCommandMenuTarget {
    /// Empty area of the tree (right panel background).
    Root,
    /// A specific node (folder or command).
    Node(QuickCommandNode),
}

/// Request passed when showing the quick-command context menu.
pub struct QuickCommandMenuRequest {
    pub position: Point<Pixels>,
    pub target: QuickCommandMenuTarget,
    /// Snapshot of the current multi-selection.
    pub selected_ids: Vec<String>,
}

#[derive(Clone)]
pub enum QuickCommandContextMenuEvent {
    Close,
    NewCommand { parent_id: Option<String> },
    NewFolder { parent_id: Option<String> },
    Edit { node: QuickCommandNode },
    Rename { node: QuickCommandNode },
    Duplicate { command_id: String },
    Delete { ids: Vec<String> },
}

pub fn open_quick_command_context_menu(
    request: QuickCommandMenuRequest,
    overlay_registry: Option<Entity<OverlayRegistry>>,
    on_event: impl Fn(QuickCommandContextMenuEvent, &mut App) + Send + Sync + 'static,
    window: &mut Window,
    cx: &mut App,
) -> Entity<PopupMenu> {
    let on_event = Arc::new(on_event);
    let mut items = Vec::new();

    let (parent_id, is_command, node) = match &request.target {
        QuickCommandMenuTarget::Root => (None, false, None),
        QuickCommandMenuTarget::Node(n) => (
            if n.is_folder() {
                Some(n.id().to_string())
            } else {
                None
            },
            !n.is_folder(),
            Some(n.clone()),
        ),
    };
    let delete_id = node.as_ref().map(|n| n.id().to_string());

    let pid1 = parent_id.clone();
    let ev1 = on_event.clone();
    let label_new_cmd = i18n!(cx, "quick_commands.new_command");
    items.push(
        PopupMenuItem::item("qc-menu-new-cmd", label_new_cmd, move |_, cx| {
            ev1(QuickCommandContextMenuEvent::NewCommand { parent_id: pid1.clone() }, cx);
        })
        .icon(AppIcon::Plus),
    );

    let pid2 = parent_id.clone();
    let ev2 = on_event.clone();
    let label_new_folder = i18n!(cx, "workspace.folder.create");
    items.push(
        PopupMenuItem::item("qc-menu-new-folder", label_new_folder, move |_, cx| {
            ev2(QuickCommandContextMenuEvent::NewFolder { parent_id: pid2.clone() }, cx);
        })
        .icon(AppIcon::NewFolder),
    );

    if is_command {
        items.push(PopupMenuItem::separator());

        let n1 = node.clone();
        let ev3 = on_event.clone();
        let label_rename = i18n!(cx, "common.action.rename");
        items.push(
            PopupMenuItem::item("qc-menu-rename", label_rename, move |_, cx| {
                if let Some(node) = n1.clone() {
                    ev3(QuickCommandContextMenuEvent::Rename { node }, cx);
                }
            })
            .icon(AppIcon::Edit),
        );

        let n2 = node.clone();
        let ev4 = on_event.clone();
        let label_edit = i18n!(cx, "common.action.edit");
        items.push(
            PopupMenuItem::item("qc-menu-edit", label_edit, move |_, cx| {
                if let Some(node) = n2.clone() {
                    ev4(QuickCommandContextMenuEvent::Edit { node }, cx);
                }
            })
            .icon(AppIcon::Edit),
        );

        let n3 = node.clone();
        let ev5 = on_event.clone();
        let label_dup = i18n!(cx, "common.action.duplicate");
        items.push(
            PopupMenuItem::item("qc-menu-duplicate", label_dup, move |_, cx| {
                if let Some(node) = n3.clone() {
                    ev5(
                        QuickCommandContextMenuEvent::Duplicate {
                            command_id: node.id().to_string(),
                        },
                        cx,
                    );
                }
            })
            .icon(AppIcon::Copy),
        );

        items.push(PopupMenuItem::separator());

        let node_id = delete_id.clone();
        let selected = request.selected_ids.clone();
        let ev6 = on_event.clone();
        let label_del = i18n!(cx, "common.action.delete");
        let t = velowork_ui::theme::theme(cx);
        items.push(
            PopupMenuItem::item("qc-menu-delete", label_del, move |_, cx| {
                let ids = match node_id.clone() {
                    Some(nid) if !selected.is_empty() && selected.contains(&nid) => {
                        selected.clone()
                    }
                    Some(nid) => vec![nid],
                    None => Vec::new(),
                };
                ev6(QuickCommandContextMenuEvent::Delete { ids }, cx);
            })
            .icon(AppIcon::Trash)
            .text_color(t.error),
        );
    } else if node.is_some() {
        items.push(PopupMenuItem::separator());

        let n1 = node.clone();
        let ev7 = on_event.clone();
        let label_rename = i18n!(cx, "common.action.rename");
        items.push(
            PopupMenuItem::item("qc-menu-rename-folder", label_rename, move |_, cx| {
                if let Some(node) = n1.clone() {
                    ev7(QuickCommandContextMenuEvent::Rename { node }, cx);
                }
            })
            .icon(AppIcon::Edit),
        );

        items.push(PopupMenuItem::separator());

        let node_id = delete_id.clone();
        let selected = request.selected_ids.clone();
        let ev8 = on_event.clone();
        let label_del = i18n!(cx, "common.action.delete");
        let t = velowork_ui::theme::theme(cx);
        items.push(
            PopupMenuItem::item("qc-menu-delete-folder", label_del, move |_, cx| {
                let ids = match node_id.clone() {
                    Some(nid) if !selected.is_empty() && selected.contains(&nid) => {
                        selected.clone()
                    }
                    Some(nid) => vec![nid],
                    None => Vec::new(),
                };
                ev8(QuickCommandContextMenuEvent::Delete { ids }, cx);
            })
            .icon(AppIcon::Trash)
            .text_color(t.error),
        );
    }

    let ev_on_close = on_event;
    let on_close: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync> = Arc::new(move |_, cx| {
        ev_on_close(QuickCommandContextMenuEvent::Close, cx);
    });

    ContextMenu::open(
        request.position,
        items,
        overlay_registry,
        Some(on_close),
        window,
        cx,
    )
}
