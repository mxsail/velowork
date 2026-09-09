//! Zoom (fullscreen) state and rendering for terminal panes.

use crate::ActionDispatch;
use gpui::*;
use velowork_core::api::ActionRequest;

use super::TerminalPane;

impl<D: ActionDispatch + Send + Sync> TerminalPane<D> {
    pub(super) fn is_zoomed(&self, cx: &Context<Self>) -> bool {
        let fm = self.focus_manager.read(cx);
        self.terminal_id.as_ref().is_some_and(|tid| {
            fm.is_terminal_fullscreened(&self.project_id, tid)
        })
    }

    fn get_project_terminals(&self, cx: &Context<Self>) -> Vec<String> {
        let ws = self.workspace.read(cx);
        ws.project(&self.project_id)
            .and_then(|p| p.layout.as_ref())
            .map(|l| l.collect_terminal_ids())
            .unwrap_or_default()
    }

    pub(super) fn handle_zoom_next_terminal(&mut self, cx: &mut Context<Self>) {
        if !self.is_zoomed(cx) { return; }
        let terminals = self.get_project_terminals(cx);
        if terminals.len() <= 1 { return; }
        if let Some(ref current_id) = self.terminal_id
            && let Some(idx) = terminals.iter().position(|id| id == current_id) {
                let next_idx = (idx + 1) % terminals.len();
                let next_id = terminals[next_idx].clone();
                if let Some(ref dispatcher) = self.action_dispatcher {
                    dispatcher.dispatch(ActionRequest::SetFullscreen {
                        project_id: self.project_id.clone(),
                        terminal_id: Some(next_id),
                        window: None,
                    }, cx);
                }
            }
    }

    pub(super) fn handle_zoom_prev_terminal(&mut self, cx: &mut Context<Self>) {
        if !self.is_zoomed(cx) { return; }
        let terminals = self.get_project_terminals(cx);
        if terminals.len() <= 1 { return; }
        if let Some(ref current_id) = self.terminal_id
            && let Some(idx) = terminals.iter().position(|id| id == current_id) {
                let prev_idx = if idx == 0 { terminals.len() - 1 } else { idx - 1 };
                let prev_id = terminals[prev_idx].clone();
                if let Some(ref dispatcher) = self.action_dispatcher {
                    dispatcher.dispatch(ActionRequest::SetFullscreen {
                        project_id: self.project_id.clone(),
                        terminal_id: Some(prev_id),
                        window: None,
                    }, cx);
                }
            }
    }

}
