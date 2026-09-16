//! Unified action dispatch — routes terminal actions to local execution.
//!
//! The `ActionDispatcher` enum encapsulates the dispatch decision. Callers simply
//! call `dispatcher.dispatch(action, cx)` without any conditionals.
//!
//! Remote (SSH) projects are dispatched locally: their terminal backend speaks
//! SSH (russh) directly, so no separate remote-control path is needed here.

use crate::terminal::backend::TerminalBackend;
use crate::views::overlays::overlay_manager::OverlayManager;
use crate::views::window::TerminalsRegistry;
use crate::workspace::actions::execute::execute_action;
use crate::workspace::focus::FocusManager;
use crate::workspace::state::{WindowId, Workspace};

use velowork_core::api::ActionRequest;

use gpui::{AppContext, Entity};
use std::sync::Arc;

/// Build an ActionDispatcher for the given project.
///
/// `window_id` carries the originating `WindowView`'s window id so per-window
/// state mutations triggered by local UI actions (e.g. hide/show via the
/// sidebar context menu routed through `SetProjectShowInOverview`) land on
/// the right window's slot.
/// Threads the workspace, focus manager, terminals, and
/// cx as distinct dependencies; a context struct would obscure more than help.
#[allow(clippy::too_many_arguments)]
pub fn dispatcher_for_project(
    project_id: &str,
    window_id: WindowId,
    workspace: &Entity<Workspace>,
    focus_manager: &Entity<FocusManager>,
    backend: &Option<Arc<dyn TerminalBackend>>,
    terminals: &TerminalsRegistry,
    overlay_manager: Option<Entity<OverlayManager>>,
    cx: &gpui::App,
) -> Option<ActionDispatcher> {
    let ws = workspace.read(cx);
    let _project = ws.project(project_id)?;
    let backend = backend.as_ref()?;
    Some(ActionDispatcher::Local {
        workspace: workspace.clone(),
        focus_manager: focus_manager.clone(),
        backend: backend.clone(),
        terminals: terminals.clone(),
        window_id,
        overlay_manager,
    })
}

/// Routes terminal actions to local execution.
///
/// Passed through the view hierarchy (ProjectColumn → LayoutContainer → TerminalPane)
/// so all action handlers dispatch through this without knowing if the project is
/// local or remote.
#[derive(Clone)]
pub enum ActionDispatcher {
    /// Local project — execute actions directly in the workspace.
    Local {
        workspace: Entity<Workspace>,
        focus_manager: Entity<FocusManager>,
        backend: Arc<dyn TerminalBackend>,
        terminals: TerminalsRegistry,
        /// Originating window's id (PRD cri 13). Per-window state mutations
        /// inside `execute_action` (e.g. `SetProjectShowInOverview`) target
        /// this slot.
        window_id: WindowId,
        overlay_manager: Option<Entity<OverlayManager>>,
    },
}

impl ActionDispatcher {
    #[allow(dead_code)]
    pub fn is_remote(&self) -> bool {
        false
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_close_single_terminal(
        workspace: &Entity<Workspace>,
        focus_manager: &Entity<FocusManager>,
        backend: &Arc<dyn TerminalBackend>,
        terminals: &TerminalsRegistry,
        window_id: WindowId,
        project_id: String,
        terminal_id: String,
        cx: &mut gpui::App,
    ) {
        let backend = backend.clone();
        let terminals = terminals.clone();
        let focus_manager = focus_manager.clone();
        let workspace = workspace.clone();
        focus_manager.update(cx, |fm, cx| {
            workspace.update(cx, |ws, cx| {
                if crate::soft_close::begin(
                    ws, fm, &backend, &terminals, &project_id, &terminal_id, cx,
                ) {
                    return;
                }
                execute_action(
                    ActionRequest::CloseTerminal { project_id, terminal_id },
                    ws, window_id, fm, &*backend, &terminals, cx,
                );
            });
            cx.notify();
        });
    }

    /// Dispatch a standard action (split, close, create terminal, etc.).
    pub fn dispatch(&self, action: ActionRequest, cx: &mut gpui::App) {
        match self {
            Self::Local {
                workspace,
                focus_manager,
                backend,
                terminals,
                window_id,
                overlay_manager,
            } => {
                if let ActionRequest::CloseTerminal { project_id, terminal_id } = &action {
                    let confirm = crate::settings::settings(cx).confirm_close_tab;
                    if confirm {
                        if let Some(om) = overlay_manager {
                            let pid = project_id.clone();
                            let tid = terminal_id.clone();
                            let workspace = workspace.clone();
                            let focus_manager = focus_manager.clone();
                            let backend = backend.clone();
                            let terminals = terminals.clone();
                            let window_id = *window_id;
                            om.update(cx, |om, cx| {
                                om.request_terminal_close_confirm(pid.clone(), tid.clone(), move |cx| {
                                    Self::execute_close_single_terminal(
                                        &workspace,
                                        &focus_manager,
                                        &backend,
                                        &terminals,
                                        window_id,
                                        pid,
                                        tid,
                                        cx,
                                    );
                                }, cx);
                            });
                            return;
                        }
                    }
                    Self::execute_close_single_terminal(
                        workspace,
                        focus_manager,
                        backend,
                        terminals,
                        *window_id,
                        project_id.clone(),
                        terminal_id.clone(),
                        cx,
                    );
                    return;
                }

                let backend = backend.clone();
                let terminals = terminals.clone();
                let focus_manager = focus_manager.clone();
                let window_id = *window_id;
                focus_manager.update(cx, |fm, cx| {
                    workspace.update(cx, |ws, cx| {
                        if let ActionRequest::CloseTerminals { project_id, terminal_ids } = &action {
                            // Optimistically close each terminal (eject now,
                            // decide kill-vs-undo off-thread); whatever isn't
                            // handled here (feature off / not in layout)
                            // hard-closes in a single batched action.
                            let mut remaining = Vec::new();
                            for terminal_id in terminal_ids {
                                if !crate::soft_close::begin(
                                    ws, fm, &backend, &terminals, project_id, terminal_id, cx,
                                ) {
                                    remaining.push(terminal_id.clone());
                                }
                            }
                            if remaining.is_empty() {
                                return;
                            }
                            execute_action(
                                ActionRequest::CloseTerminals {
                                    project_id: project_id.clone(),
                                    terminal_ids: remaining,
                                },
                                ws, window_id, fm, &*backend, &terminals, cx,
                            );
                            return;
                        }
                        execute_action(action, ws, window_id, fm, &*backend, &terminals, cx);
                    });
                    cx.notify();
                });
            }
        }
    }

    /// Split a terminal.
    ///
    /// This only modifies the layout — the UI will lazily spawn the PTY with the
    /// correct shell. Going through `execute_action` would eagerly call
    /// `spawn_uninitialized_terminals` with `None` shell, ignoring the project /
    /// global default shell (e.g. WSL).
    pub fn split_terminal(
        &self,
        project_id: &str,
        layout_path: &[usize],
        direction: crate::workspace::state::SplitDirection,
        cx: &mut impl AppContext,
    ) {
        match self {
            Self::Local { workspace, focus_manager, .. } => {
                let pid = project_id.to_string();
                let lp = layout_path.to_vec();
                let focus_manager = focus_manager.clone();
                focus_manager.update(cx, |fm, cx| {
                    workspace.update(cx, |ws, cx| {
                        ws.split_terminal(fm, &pid, &lp, direction, cx);
                    });
                    cx.notify();
                });
            }
        }
    }

    /// Add a tab.
    pub fn add_tab(
        &self,
        project_id: &str,
        layout_path: &[usize],
        in_group: bool,
        cx: &mut impl AppContext,
    ) {
        match self {
            Self::Local { workspace, focus_manager, .. } => {
                let pid = project_id.to_string();
                let lp = layout_path.to_vec();
                let focus_manager = focus_manager.clone();
                focus_manager.update(cx, |fm, cx| {
                    workspace.update(cx, |ws, cx| {
                        if in_group {
                            ws.add_tab_to_group(fm, &pid, &lp, cx);
                        } else {
                            ws.add_tab(fm, &pid, &lp, cx);
                        }
                    });
                    cx.notify();
                });
            }
        }
    }

    pub fn add_tab_with_shell(
        &self,
        project_id: &str,
        layout_path: &[usize],
        shell_type: velowork_core::shell::ShellType,
        in_group: bool,
        cx: &mut impl AppContext,
    ) {
        match self {
            Self::Local { workspace, focus_manager, .. } => {
                let pid = project_id.to_string();
                let lp = layout_path.to_vec();
                let focus_manager = focus_manager.clone();
                focus_manager.update(cx, |fm, cx| {
                    workspace.update(cx, |ws, cx| {
                        if in_group {
                            ws.add_tab_to_group_with_shell(fm, &pid, &lp, shell_type, cx);
                        } else {
                            ws.add_tab_with_shell(fm, &pid, &lp, shell_type, cx);
                        }
                    });
                    cx.notify();
                });
            }
        }
    }
}

impl ActionDispatcher {
    /// Upload a pasted clipboard image. No-op for local projects.
    pub fn upload_remote_paste_image(
        &self,
        _terminal_id: &str,
        _mime: &str,
        _bytes: Vec<u8>,
        _cx: &mut impl AppContext,
    ) {
    }
}

impl velowork_views_terminal::ActionDispatch for ActionDispatcher {
    fn dispatch(&self, action: ActionRequest, cx: &mut gpui::App) {
        self.dispatch(action, cx);
    }

    fn is_remote(&self) -> bool {
        self.is_remote()
    }

    fn split_terminal(
        &self,
        project_id: &str,
        layout_path: &[usize],
        direction: crate::workspace::state::SplitDirection,
        cx: &mut gpui::App,
    ) {
        self.split_terminal(project_id, layout_path, direction, cx);
    }

    fn add_tab(
        &self,
        project_id: &str,
        layout_path: &[usize],
        in_group: bool,
        cx: &mut gpui::App,
    ) {
        self.add_tab(project_id, layout_path, in_group, cx);
    }

    fn add_tab_with_shell(
        &self,
        project_id: &str,
        layout_path: &[usize],
        shell_type: velowork_core::shell::ShellType,
        in_group: bool,
        cx: &mut gpui::App,
    ) {
        self.add_tab_with_shell(project_id, layout_path, shell_type, in_group, cx);
    }

    fn upload_remote_paste_image(
        &self,
        terminal_id: &str,
        mime: &str,
        bytes: Vec<u8>,
        cx: &mut gpui::App,
    ) {
        self.upload_remote_paste_image(terminal_id, mime, bytes, cx);
    }
}
