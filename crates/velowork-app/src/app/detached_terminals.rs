use crate::views::overlays::detached_terminal::DetachedTerminalView;
use crate::views::overlays::detached_overlay::{DetachedHost, DetachedHostCloseEvent};
use crate::workspace::state::Workspace;
use gpui::*;
use crate::app::detached_overlays::{open_detached_overlay, DetachedOverlayOptions};
use std::collections::HashSet;

use super::Velowork;

impl Velowork {
    pub(super) fn handle_detached_terminals_changed(
        &mut self,
        workspace: Entity<Workspace>,
        cx: &mut Context<Self>,
    ) {
        let ws = workspace.read(cx);
        let current_detached: HashSet<String> = ws
            .collect_all_detached_terminals()
            .into_iter()
            .map(|(terminal_id, _, _)| terminal_id)
            .collect();

        let new_ids: Vec<_> = current_detached
            .iter()
            .filter(|id| !self.opened_detached_windows.contains(*id))
            .cloned()
            .collect();

        self.opened_detached_windows = current_detached;

        for terminal_id in new_ids {
            self.open_detached_window(&terminal_id, cx);
        }
    }

    fn open_detached_window(&self, terminal_id: &str, cx: &mut Context<Self>) {
        let workspace = self.workspace.clone();
        let transport: std::sync::Arc<dyn crate::terminal::terminal::TerminalTransport> = self.pty_manager.clone();
        let terminals = self.terminals.clone();
        let terminal_id_owned = terminal_id.to_string();

        let terminal_name = {
            let ws = workspace.read(cx);
            let mut resolved_name = None;
            for project in ws.projects() {
                if let Some(layout) = &project.layout
                    && let Some(path) = layout.find_terminal_path(terminal_id)
                    && let Some(node) = layout.get_at_path(&path)
                    && let velowork_workspace::state::LayoutNode::Terminal { shell_type, .. } = node
                {
                    if let Some(custom_name) = project.terminal_names.get(terminal_id) {
                        resolved_name = Some(custom_name.clone());
                        break;
                    }
                    if let Some(session_name) = velowork_views_terminal::overlays::detached_terminal::resolve_ssh_connection_name(shell_type, cx) {
                        resolved_name = Some(session_name);
                        break;
                    }
                    if !shell_type.is_remote() {
                        resolved_name = Some(shell_type.local_shell_name());
                        break;
                    }
                }
            }
            resolved_name.unwrap_or_else(|| terminal_id.chars().take(8).collect::<String>())
        };

        let ws_for_close = workspace.clone();
        let term_id_for_close = terminal_id_owned.clone();

        let _ = open_detached_overlay::<DetachedHost<DetachedTerminalView>, DetachedHostCloseEvent>(
            format!("{} - Detached", terminal_name),
            move |_window, _registry, cx| {
                let inner = cx.new(|cx| {
                    let mut v = DetachedTerminalView::new(
                        workspace.clone(),
                        terminal_id_owned.clone(),
                        transport.clone(),
                        terminals.clone(),
                        cx,
                    );
                    v.set_overlay_registry(_registry);
                    v
                });
                cx.new(|_cx| DetachedHost { inner })
            },
            DetachedOverlayOptions {
                size: size(px(800.0), px(600.0)),
                min_size: Size {
                    width: px(300.0),
                    height: px(200.0),
                },
                on_close: Some(std::sync::Arc::new(move |_window, cx| {
                    ws_for_close.update(cx, |ws, cx| {
                        ws.attach_terminal(&term_id_for_close, cx);
                    });
                })),
                hide_titlebar: true,
            },
            cx,
        );
    }
}
