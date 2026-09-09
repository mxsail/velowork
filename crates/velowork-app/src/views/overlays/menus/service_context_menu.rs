//! Service tree right-click context menu.

use gpui::*;
use std::sync::Arc;
use velowork_i18n::i18n;
use velowork_state::{ServiceNode, ServiceOp};
use velowork_ui::icon::AppIcon;
use velowork_ui::menu::{ContextMenu, PopupMenu, PopupMenuItem};
use velowork_ui::overlay::OverlayRegistry;
use velowork_workspace::services::service_parent_id_of;
use velowork_workspace::stores::GlobalServiceStore;

/// What the context menu was opened on.
pub enum ServiceMenuTarget {
    /// Empty area of the tree (background).
    Root,
    /// A specific node (folder or service).
    Node(ServiceNode),
}

/// Request passed when showing the service context menu.
pub struct ServiceMenuRequest {
    pub position: Point<Pixels>,
    pub target: ServiceMenuTarget,
    /// Snapshot of the current multi-selection.
    pub selected_ids: Vec<String>,
    /// Runtime status if known.
    pub status: Option<velowork_state::ServiceStatus>,
    /// Whether to only show operational commands (Start, Stop, Restart) without management actions.
    pub ops_only: bool,
}

#[derive(Clone)]
pub enum ServiceContextMenuEvent {
    Close,
    Op { node: ServiceNode, op: ServiceOp },
    SendToTerminal { node: ServiceNode },
    NewService { parent_id: Option<String> },
    NewFolder { parent_id: Option<String> },
    Edit { node: ServiceNode },
    Copy { node: ServiceNode },
    Rename { node: ServiceNode },
    Delete { ids: Vec<String> },
}

pub fn open_service_context_menu(
    request: ServiceMenuRequest,
    overlay_registry: Option<Entity<OverlayRegistry>>,
    on_event: impl Fn(ServiceContextMenuEvent, &mut App) + Send + Sync + 'static,
    window: &mut Window,
    cx: &mut App,
) -> Entity<PopupMenu> {
    let on_event = Arc::new(on_event);
    let mut items = Vec::new();

    // 计算“新建”操作的默认父目录：
    // - 右键空白处 → 根目录（None）
    // - 右键目录行 → 该目录
    // - 右键服务行 → 该服务所属目录
    let compute_parent = |n: &ServiceNode, cx: &App| -> Option<String> {
        if n.is_folder() {
            Some(n.id().to_string())
        } else {
            let tree = cx
                .try_global::<GlobalServiceStore>()
                .map(|s| s.0.read(cx).nodes().to_vec())
                .unwrap_or_default();
            service_parent_id_of(&tree, &n.id()).flatten()
        }
    };
    let (parent_id, is_service, node) = match &request.target {
        ServiceMenuTarget::Root => (None, false, None),
        ServiceMenuTarget::Node(n) => (compute_parent(n, cx), !n.is_folder(), Some(n.clone())),
    };
    let delete_id = node.as_ref().map(|n| n.id().to_string());
    let selected = request.selected_ids.clone();

    // If target is a service node, show operational management commands first.
    // Dynamic display based on status:
    // - Running: only Stop & Restart
    // - Stopped: only Start
    // - NotChecked / Unknown: all available commands
    if let Some(ref n) = node {
        if is_service {
            let def = n.def();
            let start_cmd = def
                .and_then(|d| d.effective_start_command())
                .filter(|c| !c.is_empty());
            let stop_cmd = def
                .and_then(|d| d.effective_stop_command())
                .filter(|c| !c.is_empty());
            let restart_cmd = def
                .and_then(|d| d.effective_restart_command())
                .filter(|c| !c.is_empty());

            let status = request.status.unwrap_or_else(|| {
                if let Some(d) = def {
                    if let Some(engine) = cx.try_global::<velowork_terminal::GlobalServiceMonitorEngine>() {
                        engine
                            .0
                            .read(cx)
                            .runtime_for(&d.id)
                            .map(|r| r.status)
                            .unwrap_or(velowork_state::ServiceStatus::NotChecked)
                    } else {
                        velowork_state::ServiceStatus::NotChecked
                    }
                } else {
                    velowork_state::ServiceStatus::NotChecked
                }
            });

            let show_start = start_cmd.is_some() && status != velowork_state::ServiceStatus::Running;
            let show_stop = stop_cmd.is_some() && status != velowork_state::ServiceStatus::Stopped;
            let show_restart = restart_cmd.is_some() && status != velowork_state::ServiceStatus::Stopped;

            let terminal_hint: SharedString = i18n!(cx, "service_monitor.send_to_terminal").into();
            let mut has_op_items = false;

            if show_start {
                has_op_items = true;
                let n_start = n.clone();
                let ev_start = on_event.clone();
                let label_start = i18n!(cx, "service_monitor.start");
                items.push(
                    PopupMenuItem::item("service-menu-start", label_start, move |_, cx| {
                        ev_start(
                            ServiceContextMenuEvent::Op {
                                node: n_start.clone(),
                                op: ServiceOp::Start,
                            },
                            cx,
                        );
                    })
                    .icon(AppIcon::Play)
                    .shortcut(terminal_hint.clone()),
                );
            }

            if show_stop {
                has_op_items = true;
                let n_stop = n.clone();
                let ev_stop = on_event.clone();
                let label_stop = i18n!(cx, "service_monitor.stop");
                items.push(
                    PopupMenuItem::item("service-menu-stop", label_stop, move |_, cx| {
                        ev_stop(
                            ServiceContextMenuEvent::Op {
                                node: n_stop.clone(),
                                op: ServiceOp::Stop,
                            },
                            cx,
                        );
                    })
                    .icon(AppIcon::Stop)
                    .shortcut(terminal_hint.clone()),
                );
            }

            if show_restart {
                has_op_items = true;
                let n_restart = n.clone();
                let ev_restart = on_event.clone();
                let label_restart = i18n!(cx, "service_monitor.restart");
                items.push(
                    PopupMenuItem::item("service-menu-restart", label_restart, move |_, cx| {
                        ev_restart(
                            ServiceContextMenuEvent::Op {
                                node: n_restart.clone(),
                                op: ServiceOp::Restart,
                            },
                            cx,
                        );
                    })
                    .icon(AppIcon::Refresh)
                    .shortcut(terminal_hint.clone()),
                );
            }

            if has_op_items && !request.ops_only {
                items.push(PopupMenuItem::separator());
            }
        }
    }

    if request.ops_only {
        let ev_on_close = on_event;
        let on_close: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync> = Arc::new(move |_, cx| {
            ev_on_close(ServiceContextMenuEvent::Close, cx);
        });

        return ContextMenu::open(
            request.position,
            items,
            overlay_registry,
            Some(on_close),
            window,
            cx,
        );
    }

    let pid1 = parent_id.clone();
    let ev1 = on_event.clone();
    let label_new = i18n!(cx, "service_monitor.add");
    items.push(
        PopupMenuItem::item("service-menu-new", label_new, move |_, cx| {
            ev1(ServiceContextMenuEvent::NewService { parent_id: pid1.clone() }, cx);
        })
        .icon(AppIcon::Plus),
    );

    let pid2 = parent_id.clone();
    let ev2 = on_event.clone();
    let label_new_folder = i18n!(cx, "service_monitor.new_folder");
    items.push(
        PopupMenuItem::item("service-menu-new-folder", label_new_folder, move |_, cx| {
            ev2(ServiceContextMenuEvent::NewFolder { parent_id: pid2.clone() }, cx);
        })
        .icon(AppIcon::NewFolder),
    );

    if let Some(n) = node {
        items.push(PopupMenuItem::separator());

        let node_rename = n.clone();
        let ev3 = on_event.clone();
        let label_rename = i18n!(cx, "common.rename");
        items.push(
            PopupMenuItem::item("service-menu-rename", label_rename, move |_, cx| {
                ev3(ServiceContextMenuEvent::Rename { node: node_rename.clone() }, cx);
            })
            .icon(AppIcon::Edit),
        );

        if is_service {
            let node_edit = n.clone();
            let ev4 = on_event.clone();
            let label_edit = i18n!(cx, "common.edit");
            items.push(
                PopupMenuItem::item("service-menu-edit", label_edit, move |_, cx| {
                    ev4(ServiceContextMenuEvent::Edit { node: node_edit.clone() }, cx);
                })
                .icon(AppIcon::Edit),
            );

            let node_copy = n.clone();
            let ev5 = on_event.clone();
            let label_copy = i18n!(cx, "common.copy");
            items.push(
                PopupMenuItem::item("service-menu-copy", label_copy, move |_, cx| {
                    ev5(ServiceContextMenuEvent::Copy { node: node_copy.clone() }, cx);
                })
                .icon(AppIcon::Copy),
            );
        }

        items.push(PopupMenuItem::separator());

        let node_id = delete_id.clone();
        let ev6 = on_event.clone();
        let label_del = i18n!(cx, "common.delete");
        let t = velowork_ui::theme::theme(cx);
        items.push(
            PopupMenuItem::item("service-menu-delete", label_del, move |_, cx| {
                let ids = match node_id.clone() {
                    Some(nid) if !selected.is_empty() && selected.contains(&nid) => {
                        selected.clone()
                    }
                    Some(nid) => vec![nid],
                    None => Vec::new(),
                };
                ev6(ServiceContextMenuEvent::Delete { ids }, cx);
            })
            .icon(AppIcon::Trash)
            .text_color(t.error),
        );
    }

    let ev_on_close = on_event;
    let on_close: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync> = Arc::new(move |_, cx| {
        ev_on_close(ServiceContextMenuEvent::Close, cx);
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
