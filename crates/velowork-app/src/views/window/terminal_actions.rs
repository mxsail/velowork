use crate::settings::settings;
use crate::terminal::terminal::{Terminal, TerminalSize};
use crate::views::panels::toast::ToastManager;
use crate::workspace::actions::execute::spawn_uninitialized_terminals;
use gpui::*;
use std::sync::Arc;

use super::WindowView;

impl WindowView {
    /// Spawn terminals for all layout slots in a project that have terminal_id: None
    /// Used after creating a worktree project to immediately populate terminals
    pub(super) fn spawn_terminals_for_project(&mut self, project_id: String, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let terminals = self.terminals.clone();
        self.workspace.update(cx, |ws, cx| {
            spawn_uninitialized_terminals(ws, &project_id, &*backend, &terminals, cx);
        });
        self.sync_project_columns(cx);
    }

    /// Switch terminal shell - kills old terminal and creates new one with the new shell.
    /// Used when user selects a different shell from the shell selector overlay.
    pub(super) fn switch_terminal_shell(
        &mut self,
        project_id: &str,
        old_terminal_id: &str,
        shell_type: crate::terminal::shell_config::ShellType,
        cx: &mut Context<Self>,
    ) {
        // Get project path and terminal's layout path
        let (project_path, layout_path) = {
            let ws = self.workspace.read(cx);
            let project = match ws.project(project_id) {
                Some(p) => p,
                None => {
                    log::error!("[window:terminal] switch_terminal_shell: Project not found | project_id={}", project_id);
                    return;
                }
            };
            let layout_path = match project.layout.as_ref().and_then(|l| l.find_terminal_path(old_terminal_id)) {
                Some(p) => p,
                None => {
                    log::error!("[window:terminal] switch_terminal_shell: Terminal not found | project_id={} terminal_id={}", project_id, old_terminal_id);
                    return;
                }
            };
            (project.path.clone(), layout_path)
        };

        // Get current shell to check if it's actually changing
        let current_shell = self.workspace.read(cx).get_terminal_shell(project_id, &layout_path);
        if current_shell.as_ref() == Some(&shell_type) {
            log::debug!("[window:terminal] switch_terminal_shell: Shell type unchanged, skipping | shell={:?}", shell_type);
            return;
        }

        // Kill the old terminal
        self.backend.kill(old_terminal_id);
        self.terminals.lock().remove(old_terminal_id);

        // Update shell type in workspace state
        self.workspace.update(cx, |ws, cx| {
            ws.set_terminal_shell(project_id, &layout_path, shell_type.clone(), cx);
        });

        // Determine the actual shell to use (resolve Default → project default → global default)
        let actual_shell = shell_type.resolve_default(
            self.workspace.read(cx).project(project_id).and_then(|p| p.default_shell.as_ref()),
            &settings(cx).default_shell,
        );

        // Create new terminal with the new shell
        match self.backend.create_terminal(&project_path, Some(&actual_shell)) {
            Ok(new_terminal_id) => {
                log::info!("[window:terminal] switch_terminal_shell: Switched shell | shell={:?} new_terminal_id={}", actual_shell, new_terminal_id);

                // Update terminal_id in workspace state
                self.workspace.update(cx, |ws, cx| {
                    ws.set_terminal_id(project_id, &layout_path, new_terminal_id.clone(), cx);
                });

                // Create terminal wrapper and register it
                let size = TerminalSize::default();
                let terminal = Arc::new(Terminal::new(
                    new_terminal_id.clone(),
                    size,
                    self.backend.transport(),
                    project_path.clone(),
                ));
                self.terminals.lock().insert(new_terminal_id, terminal);
            }
            Err(e) => {
                log::error!("[window:terminal] switch_terminal_shell: Failed to create terminal with new shell | error: {:#}", e);
                ToastManager::error(format!("Failed to create terminal: {}", e), cx);
            }
        }
    }
}
