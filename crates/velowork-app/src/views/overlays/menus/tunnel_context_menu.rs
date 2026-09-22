//! Tunnel tree right-click context menu.

use gpui::*;
use std::sync::Arc;
use velowork_i18n::i18n;
use velowork_state::TunnelNode;
use velowork_ui::icon::AppIcon;
use velowork_ui::menu::{ContextMenu, PopupMenu, PopupMenuItem};
use velowork_ui::overlay::OverlayRegistry;

/// What the context menu was opened on.
pub enum TunnelMenuTarget {
    /// Empty area of the tree (background).
    Root,
    /// A specific node (folder or tunnel).
    Node(TunnelNode),
}

/// Request passed when showing the tunnel context menu.
pub struct TunnelMenuRequest {
    pub position: Point<Pixels>,
    pub target: TunnelMenuTarget,
    /// Snapshot of the current multi-selection.
    pub selected_ids: Vec<String>,
}

#[derive(Clone)]
pub enum TunnelContextMenuEvent {
    Close,
    NewTunnel { parent_id: Option<String> },
    NewFolder { parent_id: Option<String> },
    Edit { node: TunnelNode },
    Copy { node: TunnelNode },
    Rename { node: TunnelNode },
    Delete { ids: Vec<String> },
}

pub fn open_tunnel_context_menu(
    request: TunnelMenuRequest,
    overlay_registry: Option<Entity<OverlayRegistry>>,
    on_event: impl Fn(TunnelContextMenuEvent, &mut App) + Send + Sync + 'static,
    window: &mut Window,
    cx: &mut App,
) -> Entity<PopupMenu> {
    let on_event = Arc::new(on_event);
    let mut items = Vec::new();

    let (parent_id, is_tunnel, node) = match &request.target {
        TunnelMenuTarget::Root => (None, false, None),
        TunnelMenuTarget::Node(n) => (
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
    let label_new = i18n!(cx, "tunnel.add");
    items.push(
        PopupMenuItem::item("tunnel-menu-new", label_new, move |_, cx| {
            ev1(TunnelContextMenuEvent::NewTunnel { parent_id: pid1.clone() }, cx);
        })
        .icon(AppIcon::Plus),
    );

    let pid2 = parent_id.clone();
    let ev2 = on_event.clone();
    let label_new_folder = i18n!(cx, "workspace.folder.create");
    items.push(
        PopupMenuItem::item("tunnel-menu-new-folder", label_new_folder, move |_, cx| {
            ev2(TunnelContextMenuEvent::NewFolder { parent_id: pid2.clone() }, cx);
        })
        .icon(AppIcon::NewFolder),
    );

    if let Some(n) = node {
        items.push(PopupMenuItem::separator());

        let node_rename = n.clone();
        let ev3 = on_event.clone();
        let label_rename = i18n!(cx, "common.action.rename");
        items.push(
            PopupMenuItem::item("tunnel-menu-rename", label_rename, move |_, cx| {
                ev3(TunnelContextMenuEvent::Rename { node: node_rename.clone() }, cx);
            })
            .icon(AppIcon::Edit),
        );

        if is_tunnel {
            let node_edit = n.clone();
            let ev4 = on_event.clone();
            let label_edit = i18n!(cx, "common.action.edit");
            items.push(
                PopupMenuItem::item("tunnel-menu-edit", label_edit, move |_, cx| {
                    ev4(TunnelContextMenuEvent::Edit { node: node_edit.clone() }, cx);
                })
                .icon(AppIcon::Edit),
            );

            let node_copy = n.clone();
            let ev5 = on_event.clone();
            let label_copy = i18n!(cx, "common.action.copy");
            items.push(
                PopupMenuItem::item("tunnel-menu-copy", label_copy, move |_, cx| {
                    ev5(TunnelContextMenuEvent::Copy { node: node_copy.clone() }, cx);
                })
                .icon(AppIcon::Copy),
            );
        }

        items.push(PopupMenuItem::separator());

        let node_id = delete_id.clone();
        let selected = request.selected_ids.clone();
        let ev6 = on_event.clone();
        let label_del = i18n!(cx, "common.action.delete");
        let t = velowork_ui::theme::theme(cx);
        items.push(
            PopupMenuItem::item("tunnel-menu-delete", label_del, move |_, cx| {
                let ids = match node_id.clone() {
                    Some(nid) if !selected.is_empty() && selected.contains(&nid) => {
                        selected.clone()
                    }
                    Some(nid) => vec![nid],
                    None => Vec::new(),
                };
                ev6(TunnelContextMenuEvent::Delete { ids }, cx);
            })
            .icon(AppIcon::Trash)
            .text_color(t.error),
        );
    }

    let ev_on_close = on_event;
    let on_close: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync> = Arc::new(move |_, cx| {
        ev_on_close(TunnelContextMenuEvent::Close, cx);
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
