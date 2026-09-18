//! Focus Management for Terminal Panes
//!
//! This module provides unified focus management across the application.
//! It maintains a focus stack for restoration after modal dismissal or
//! fullscreen exit, and ensures consistent focus event propagation.
//!
//! FocusManager is the single source of truth for:
//! - Which terminal is focused (current_focus)
//! - Which project is zoomed/focused in the sidebar (focused_project_id)
//! - Whether a terminal is in fullscreen/zoom mode (Fullscreen context + terminal_id)

/// Identifies a focusable terminal in the workspace
#[derive(Clone, Debug, PartialEq)]
pub struct FocusTarget {
    pub project_id: String,
    pub layout_path: Vec<usize>,
    /// Terminal ID (set when entering fullscreen to track which terminal is zoomed)
    pub terminal_id: Option<String>,
}

impl FocusTarget {
    pub fn new(project_id: String, layout_path: Vec<usize>) -> Self {
        Self {
            project_id,
            layout_path,
            terminal_id: None,
        }
    }

    pub fn with_terminal(project_id: String, layout_path: Vec<usize>, terminal_id: String) -> Self {
        Self {
            project_id,
            layout_path,
            terminal_id: Some(terminal_id),
        }
    }
}

/// Focus context type for distinguishing different focus scenarios
#[derive(Clone, Debug, PartialEq)]
pub enum FocusContext {
    /// Normal terminal focus
    Terminal,
    /// Fullscreen mode is active
    Fullscreen,
    /// Modal dialog is open (search, rename, etc.)
    Modal,
}

/// Categorical owner of input focus.
///
/// This is the application-level analogue of GPUI's physical `FocusHandle`:
/// `FocusManager` is the single authority that decides *which category* of
/// component is allowed to own keyboard input at any moment. Components must
/// NOT call `window.focus(...)` on their own initiative to steal focus from
/// another category — they register intent here (via `request_focus`,
/// `push_layer`, `pop_layer`) and the manager gates re-claiming.
///
/// This is what prevents the terminal's frequent timer-driven re-renders
/// from hijacking focus away from foreign inputs (AI side panel, popup
/// text fields, sidebar inputs, ...). The terminal only re-claims focus
/// when this layer says a terminal (or nothing yet) owns input AND no
/// foreign input currently holds the physical focus.
///
/// Variants mirror the high-level input owners in the app:
/// - `None`: nothing currently owns input focus (safe for a terminal to claim).
/// - `Terminal`: a terminal pane owns input.
/// - `Dialog`: a modal/overlay (command palette, settings, file picker, …) owns input.
/// - `Search`: a search box (terminal find, file/content search) owns input.
/// - `Editor`: a text editor / file viewer owns input.
/// - `SessionTree`: the sidebar session tree owns input.
/// - `Settings`: the settings panel owns input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusLayer {
    /// Nothing currently owns input focus.
    None,
    /// A terminal pane owns input.
    Terminal,
    /// A modal/overlay (command palette, settings, file picker, …) owns input.
    Dialog,
    /// A search box (terminal find, file/content search) owns input.
    Search,
    /// A text editor / file viewer owns input.
    Editor,
    /// The sidebar session tree owns input.
    SessionTree,
    /// The settings panel owns input.
    Settings,
}

/// Entry in the focus stack for restoration
#[derive(Clone, Debug)]
struct FocusStackEntry {
    target: Option<FocusTarget>,
    context: FocusContext,
    /// Saved focused_project_id at the time of push
    focused_project_id: Option<String>,
}

/// Manages focus state and focus stack for the application.
///
/// The FocusManager is the single source of truth for:
/// - Current terminal focus target
/// - Project zoom/focus state (focused_project_id)
/// - Fullscreen terminal state
/// - Focus stack for restoration after modal/fullscreen exit
#[derive(Clone, Debug)]
pub struct FocusManager {
    /// Currently focused terminal (if any)
    current_focus: Option<FocusTarget>,
    /// Focus stack for restoration (most recent on top)
    focus_stack: Vec<FocusStackEntry>,
    /// Current focus context
    context: FocusContext,
    /// Current focus *layer* — the category authorized to own input focus.
    /// This is the single source of truth consulted by `terminal_may_claim_focus`
    /// (via `terminal_layer_allows_claim`) and by components deciding whether
    /// they are the active input target. See `FocusLayer`.
    layer: FocusLayer,
    /// Stack of prior focus layers, for push/pop (e.g. Dialog over Terminal).
    layer_stack: Vec<FocusLayer>,
    /// Which project is "zoomed" in the sidebar (only that project's column is visible)
    focused_project_id: Option<String>,
    /// Global active project ID (persisted across restarts, updated when switching projects)
    active_project_id: Option<String>,
    /// When true, focusing a parent project shows only that project (not its worktree children)
    focus_project_individual: bool,
    /// Saved terminal focus from before project zoom, restored when returning to overview
    pre_zoom_focus: Option<FocusTarget>,
    /// Maximum stack depth to prevent memory issues
    max_stack_depth: usize,
}

impl Default for FocusManager {
    fn default() -> Self {
        Self::new()
    }
}

impl FocusManager {
    pub fn new() -> Self {
        Self {
            current_focus: None,
            focus_stack: Vec::new(),
            context: FocusContext::Terminal,
            layer: FocusLayer::None,
            layer_stack: Vec::new(),
            focused_project_id: None,
            active_project_id: None,
            focus_project_individual: false,
            pre_zoom_focus: None,
            max_stack_depth: 10,
        }
    }

    /// Get the current focus as FocusedTerminalState for backward compatibility.
    ///
    pub fn current_focus(&self) -> Option<&FocusTarget> {
        self.current_focus.as_ref()
    }

    /// This is the primary method for checking which terminal is focused.
    /// Returns None if no terminal is focused.
    pub fn focused_terminal_state(&self) -> Option<crate::state::FocusedTerminalState> {
        self.current_focus.as_ref().map(|target| {
            crate::state::FocusedTerminalState {
                project_id: target.project_id.clone(),
                layout_path: target.layout_path.clone(),
            }
        })
    }

    /// Returns the focused terminal state, falling back to the most recent target in the focus stack if currently in modal/cleared state.
    pub fn last_focused_terminal_state(&self) -> Option<crate::state::FocusedTerminalState> {
        if let Some(state) = self.focused_terminal_state() {
            return Some(state);
        }
        for entry in self.focus_stack.iter().rev() {
            if let Some(ref target) = entry.target {
                return Some(crate::state::FocusedTerminalState {
                    project_id: target.project_id.clone(),
                    layout_path: target.layout_path.clone(),
                });
            }
        }
        None
    }

    /// Get the current focus context
    #[allow(dead_code)]
    pub fn context(&self) -> &FocusContext {
        &self.context
    }

    /// Check if a specific terminal is currently focused
    pub fn is_focused(&self, project_id: &str, layout_path: &[usize]) -> bool {
        self.current_focus.as_ref().is_some_and(|f| {
            f.project_id == project_id && f.layout_path == layout_path
        })
    }

    // --- Focus layer (unified input ownership) ---

    /// The focus layer currently authorized to own input focus.
    ///
    /// This is the high-level category (see `FocusLayer`) that `FocusManager`
    /// uses as the single source of truth for *who may own input focus*.
    pub fn current_layer(&self) -> FocusLayer {
        self.layer
    }

    /// Authoritatively set the active focus layer.
    ///
    /// Used for direct, non-stacked focus transitions such as clicking a
    /// terminal (`request_focus(FocusLayer::Terminal)`) or an editor gaining
    /// focus (`request_focus(FocusLayer::Editor)`).
    pub fn request_focus(&mut self, layer: FocusLayer) {
        self.layer = layer;
    }

    /// Push a focus layer, saving the current one so it can be restored by
    /// `pop_layer`. Used by the modal manager when a dialog opens over
    /// whatever currently owns input focus.
    pub fn push_layer(&mut self, layer: FocusLayer) {
        self.layer_stack.push(self.layer);
        self.layer = layer;
    }

    /// Pop the most recent focus layer, restoring the previously saved one.
    /// Returns the layer that was active before the pop.
    pub fn pop_layer(&mut self) -> FocusLayer {
        if let Some(prev) = self.layer_stack.pop() {
            self.layer = prev;
        }
        self.layer
    }

    /// Whether the terminal layer (or an empty focus state) is currently
    /// authorized to own input focus. When this is `false` (Dialog, Search,
    /// Editor, Settings, SessionTree), a terminal pane must NOT reclaim focus.
    ///
    /// This is the first half of the unified focus decision; the second half
    /// (physical-focus check) lives in `terminal_may_claim_focus` on the
    /// terminal side, since `PaneMap` lives in the terminal crate.
    pub fn terminal_layer_allows_claim(&self) -> bool {
        matches!(self.layer, FocusLayer::Terminal | FocusLayer::None)
    }

    // --- Focused project ID (project zoom) ---

    /// Get the global active project ID.
    pub fn active_project_id(&self) -> Option<&String> {
        self.active_project_id
            .as_ref()
            .or(self.focused_project_id.as_ref())
    }

    /// Set the global active project ID.
    pub fn set_active_project_id(&mut self, id: Option<String>) {
        if let Some(ref pid) = id {
            self.active_project_id = Some(pid.clone());
        }
    }

    /// Get the currently focused/zoomed project ID
    pub fn focused_project_id(&self) -> Option<&String> {
        self.focused_project_id.as_ref()
    }

    /// Set the focused/zoomed project ID
    ///
    /// When zooming into a project (Some), saves current terminal focus for later restoration.
    /// When returning to overview (None), restores the previously saved terminal focus.
    pub fn set_focused_project_id(&mut self, id: Option<String>) {
        self.apply_zoom_focus_save_restore(&id);
        if let Some(ref pid) = id {
            self.active_project_id = Some(pid.clone());
        }
        self.focused_project_id = id;
        self.focus_project_individual = false;
    }

    /// Set the focused project ID with individual mode (show only this project, not worktree children)
    ///
    /// When zooming into a project (Some), saves current terminal focus for later restoration.
    /// When returning to overview (None), restores the previously saved terminal focus.
    pub fn set_focused_project_id_individual(&mut self, id: Option<String>) {
        self.apply_zoom_focus_save_restore(&id);
        if let Some(ref pid) = id {
            self.active_project_id = Some(pid.clone());
        }
        self.focused_project_id = id;
        self.focus_project_individual = true;
    }

    /// Save/restore terminal focus around project zoom transitions.
    ///
    /// Entering zoom (None→Some): saves current focus.
    /// Exiting zoom (Some→None): restores saved focus.
    /// Switching zoom (Some→Some): no change to saved focus.
    fn apply_zoom_focus_save_restore(&mut self, new_id: &Option<String>) {
        match (&self.focused_project_id, new_id) {
            (None, Some(_)) => {
                // Entering zoom — save current terminal focus
                self.pre_zoom_focus = self.current_focus.clone();
            }
            (Some(_), None) => {
                // Returning to overview — restore saved terminal focus
                if let Some(saved) = self.pre_zoom_focus.take() {
                    self.current_focus = Some(saved);
                }
            }
            _ => {}
        }
    }

    /// Whether focus is in individual mode (don't expand worktree children)
    pub fn is_focus_individual(&self) -> bool {
        self.focus_project_individual
    }

    // --- Fullscreen state queries ---

    /// Get fullscreen state as (project_id, terminal_id) if in fullscreen
    pub fn fullscreen_state(&self) -> Option<(&str, &str)> {
        if self.context != FocusContext::Fullscreen {
            return None;
        }
        self.current_focus.as_ref().and_then(|f| {
            f.terminal_id.as_deref().map(|tid| (f.project_id.as_str(), tid))
        })
    }

    /// Check if a specific terminal is currently fullscreened
    pub fn is_terminal_fullscreened(&self, project_id: &str, terminal_id: &str) -> bool {
        self.fullscreen_state()
            .is_some_and(|(pid, tid)| pid == project_id && tid == terminal_id)
    }

    /// Check if any terminal is in fullscreen mode
    pub fn has_fullscreen(&self) -> bool {
        self.context == FocusContext::Fullscreen
            && self.current_focus.as_ref().is_some_and(|f| f.terminal_id.is_some())
    }

    /// Get the project ID of the fullscreened terminal (if any)
    pub fn fullscreen_project_id(&self) -> Option<&str> {
        self.fullscreen_state().map(|(pid, _)| pid)
    }

    // --- Focus actions ---

    /// Focus a terminal pane.
    ///
    /// This is the primary method for focusing a terminal. It:
    /// - Updates the current focus target
    /// - Does NOT push to stack (direct user action)
    pub fn focus_terminal(&mut self, project_id: String, layout_path: Vec<usize>) {
        if self.context == FocusContext::Fullscreen {
            // Preserve fullscreen state — only update layout_path if same project
            if let Some(ref mut focus) = self.current_focus {
                focus.layout_path = layout_path;
            }
            return;
        }
        self.current_focus = Some(FocusTarget::new(project_id, layout_path));
        self.context = FocusContext::Terminal;
        self.layer = FocusLayer::Terminal;
    }

    /// Enter fullscreen mode, saving current focus for restoration.
    ///
    /// When entering fullscreen, the current focus and focused_project_id are
    /// pushed to the stack so they can be restored when fullscreen exits.
    /// If already in fullscreen, the target is swapped in place — switching
    /// terminals via the zoom header arrows must not grow the stack, otherwise
    /// each switch would require another exit click to undo.
    pub fn enter_fullscreen(&mut self, project_id: String, layout_path: Vec<usize>, terminal_id: String) {
        if self.context == FocusContext::Fullscreen {
            self.current_focus = Some(FocusTarget::with_terminal(project_id.clone(), layout_path, terminal_id));
            self.focused_project_id = Some(project_id);
            return;
        }

        // Save current state to stack (target may be None if nothing was focused)
        self.push_focus(self.current_focus.clone(), self.context.clone(), self.focused_project_id.clone());

        // Set fullscreen as current focus
        self.current_focus = Some(FocusTarget::with_terminal(project_id.clone(), layout_path, terminal_id));
        self.context = FocusContext::Fullscreen;

        // Also zoom to the project
        self.focused_project_id = Some(project_id);
    }

    /// Exit fullscreen mode, restoring previous focus and project zoom.
    ///
    /// Returns the target that should be focused after exiting fullscreen.
    pub fn exit_fullscreen(&mut self) -> Option<FocusTarget> {
        if self.context != FocusContext::Fullscreen {
            return None;
        }

        // Pop and restore previous focus + focused_project_id
        if let Some(entry) = self.pop_focus() {
            self.current_focus = entry.target.clone();
            self.context = entry.context;
            self.focused_project_id = entry.focused_project_id;
            entry.target
        } else {
            // No saved focus, clear current
            self.current_focus = None;
            self.context = FocusContext::Terminal;
            self.focused_project_id = None;
            None
        }
    }

    /// Clear fullscreen without restoring the saved focused_project_id.
    ///
    /// Used by set_focused_project() which overrides the project zoom itself.
    /// This avoids exit_fullscreen() restoring an old project_id that would
    /// immediately be overwritten.
    pub fn clear_fullscreen_without_restore(&mut self) {
        if self.context != FocusContext::Fullscreen {
            return;
        }

        // Pop the stack entry but discard it (don't restore focused_project_id)
        let _ = self.pop_focus();
        self.context = FocusContext::Terminal;
        // Don't clear current_focus - the caller (set_focused_project) will set new focus
    }

    /// Enter modal context (search, rename, etc.), saving current focus.
    ///
    /// Modal contexts temporarily take focus away from terminals.
    /// The previous focus is saved for restoration when the modal closes.
    /// Note: focused_project_id is NOT saved — modals don't change it,
    /// and actions dispatched from modals (e.g. command palette) may
    /// intentionally modify it.
    pub fn enter_modal(&mut self) {
        self.push_layer(FocusLayer::Dialog);
        self.push_focus(self.current_focus.clone(), self.context.clone(), None);
        self.context = FocusContext::Modal;
    }

    /// Exit modal context, restoring previous focus.
    ///
    /// Returns the target that should be focused after exiting the modal.
    /// Only restores current_focus and context — focused_project_id is left as-is.
    pub fn exit_modal(&mut self) -> Option<FocusTarget> {
        if self.context != FocusContext::Modal {
            return self.current_focus.clone();
        }

        // Restore the focus layer that was active before the dialog opened.
        self.pop_layer();

        if let Some(entry) = self.pop_focus() {
            self.current_focus = entry.target.clone();
            self.context = entry.context;
            // Don't restore focused_project_id — leave whatever is current
            entry.target
        } else {
            self.context = FocusContext::Terminal;
            self.current_focus.clone()
        }
    }

    /// Clear current focus without affecting the stack.
    ///
    /// Used when focus should be removed but not restored later
    /// (e.g., terminal closed).
    pub fn clear_focus(&mut self) {
        self.current_focus = None;
        self.context = FocusContext::Terminal;
        self.layer = FocusLayer::None;
    }

    /// Scrub any references to project IDs that no longer exist in
    /// `valid_project_ids`. Called by the Velowork workspace observer after
    /// a delete so every window's focus manager — not just the one that
    /// invoked the delete — drops stale references. Without this, an
    /// extra that was zoomed on a project deleted via main (or vice
    /// versa) keeps a ghost focus_project_id and renders a missing
    /// project's column.
    ///
    /// Returns `true` if anything was cleared (caller can decide whether
    /// to notify).
    pub fn clear_stale_focus<F>(&mut self, project_exists: F) -> bool
    where
        F: Fn(&str) -> bool + Copy,
    {
        let mut changed = false;
        if let Some(id) = self.focused_project_id.as_deref()
            && !project_exists(id)
        {
            self.focused_project_id = None;
            changed = true;
        }
        if let Some(ref target) = self.current_focus
            && !project_exists(&target.project_id)
        {
            self.current_focus = None;
            self.context = FocusContext::Terminal;
            self.layer = FocusLayer::None;
            changed = true;
        }
        if let Some(ref target) = self.pre_zoom_focus
            && !project_exists(&target.project_id)
        {
            self.pre_zoom_focus = None;
            changed = true;
        }
        let before = self.focus_stack.len();
        self.focus_stack.retain(|entry| {
            let target_alive = entry
                .target
                .as_ref()
                .is_none_or(|t| project_exists(&t.project_id));
            let project_alive = entry
                .focused_project_id
                .as_deref()
                .is_none_or(project_exists);
            target_alive && project_alive
        });
        if self.focus_stack.len() != before {
            changed = true;
        }
        changed
    }

    /// Clear all focus state: current focus, focused_project_id, active_project_id, and stack.
    ///
    /// Used when switching workspaces to reset everything.
    pub fn clear_all(&mut self) {
        self.current_focus = None;
        self.context = FocusContext::Terminal;
        self.layer = FocusLayer::None;
        self.layer_stack.clear();
        self.focused_project_id = None;
        self.active_project_id = None;
        self.pre_zoom_focus = None;
        self.focus_stack.clear();
    }

    /// Realign focus and active project against a given list of projects (e.g. after workspace restore or reload).
    ///
    /// If `preferred_focused_id` exists in `projects`, it is selected. Otherwise falls back to
    /// the first project in `projects`. If `projects` is empty, focus remains cleared.
    pub fn realign_with_projects(&mut self, projects: &[crate::state::ProjectData], preferred_focused_id: Option<&str>) {
        let target_pid = preferred_focused_id
            .filter(|id| projects.iter().any(|p| p.id == *id))
            .map(|id| id.to_string())
            .or_else(|| projects.first().map(|p| p.id.clone()));

        if let Some(pid) = target_pid {
            self.focused_project_id = Some(pid.clone());
            self.active_project_id = Some(pid);
        } else {
            self.focused_project_id = None;
            self.active_project_id = None;
        }
    }

    /// Push a focus entry onto the stack.
    fn push_focus(&mut self, target: Option<FocusTarget>, context: FocusContext, focused_project_id: Option<String>) {
        // Enforce max stack depth
        while self.focus_stack.len() >= self.max_stack_depth {
            self.focus_stack.remove(0);
        }

        self.focus_stack.push(FocusStackEntry { target, context, focused_project_id });
    }

    /// Pop the most recent focus entry from the stack.
    fn pop_focus(&mut self) -> Option<FocusStackEntry> {
        self.focus_stack.pop()
    }

    /// Check if we're in modal context
    pub fn is_modal(&self) -> bool {
        self.context == FocusContext::Modal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_exit_fullscreen_restores_state() {
        let mut fm = FocusManager::new();
        fm.focus_terminal("proj1".to_string(), vec![0]);
        fm.set_focused_project_id(None);

        fm.enter_fullscreen("proj1".to_string(), vec![0], "term1".to_string());
        assert!(fm.has_fullscreen());
        assert_eq!(fm.fullscreen_project_id(), Some("proj1"));
        assert!(fm.is_terminal_fullscreened("proj1", "term1"));
        assert_eq!(fm.focused_project_id(), Some(&"proj1".to_string()));

        let restored = fm.exit_fullscreen();
        assert!(!fm.has_fullscreen());
        assert!(restored.is_some());
        let target = restored.unwrap();
        assert_eq!(target.project_id, "proj1");
        assert_eq!(target.layout_path, vec![0]);
        // focused_project_id restored to None
        assert_eq!(fm.focused_project_id(), None);
    }

    #[test]
    fn enter_exit_modal_restores_focus() {
        let mut fm = FocusManager::new();
        fm.focus_terminal("proj1".to_string(), vec![0]);

        fm.enter_modal();
        assert!(fm.is_modal());
        // Current focus is preserved for visual indicator
        assert!(fm.focused_terminal_state().is_some());

        let restored = fm.exit_modal();
        assert!(!fm.is_modal());
        assert_eq!(*fm.context(), FocusContext::Terminal);
        let target = restored.unwrap();
        assert_eq!(target.project_id, "proj1");
    }

    #[test]
    fn stack_depth_limit_enforced() {
        let mut fm = FocusManager::new();
        // Push more than max_stack_depth (10) entries via repeated modal entries
        for _ in 0..15 {
            fm.enter_modal();
        }
        assert!(fm.focus_stack.len() <= fm.max_stack_depth);
    }

    #[test]
    fn switching_fullscreen_target_does_not_grow_stack() {
        let mut fm = FocusManager::new();
        fm.focus_terminal("proj1".to_string(), vec![0]);

        fm.enter_fullscreen("proj1".to_string(), vec![0], "term1".to_string());
        let stack_after_first = fm.focus_stack.len();

        // Simulate the zoom header next/prev arrows switching the fullscreened terminal.
        fm.enter_fullscreen("proj1".to_string(), vec![1], "term2".to_string());
        fm.enter_fullscreen("proj1".to_string(), vec![2], "term3".to_string());

        assert_eq!(fm.focus_stack.len(), stack_after_first);
        assert!(fm.is_terminal_fullscreened("proj1", "term3"));

        // A single exit must fully leave fullscreen, not unwind through the switches.
        fm.exit_fullscreen();
        assert!(!fm.has_fullscreen());
    }

    #[test]
    fn clear_all_resets_everything() {
        let mut fm = FocusManager::new();
        fm.focus_terminal("proj1".to_string(), vec![0]);
        fm.set_focused_project_id(Some("proj1".to_string()));
        fm.enter_fullscreen("proj1".to_string(), vec![0], "term1".to_string());

        fm.clear_all();
        assert!(fm.focused_terminal_state().is_none());
        assert_eq!(fm.focused_project_id(), None);
        assert_eq!(fm.active_project_id(), None);
        assert!(!fm.has_fullscreen());
        assert!(fm.focus_stack.is_empty());
        assert_eq!(*fm.context(), FocusContext::Terminal);
    }

    #[test]
    fn test_realign_with_projects() {
        use crate::state::ProjectData;
        let mut fm = FocusManager::new();
        let p1 = ProjectData { id: "p1".to_string(), name: "P1".to_string(), ..Default::default() };
        let p2 = ProjectData { id: "p2".to_string(), name: "P2".to_string(), ..Default::default() };
        let projects = vec![p1, p2];

        // Preferred is p2
        fm.realign_with_projects(&projects, Some("p2"));
        assert_eq!(fm.focused_project_id(), Some(&"p2".to_string()));
        assert_eq!(fm.active_project_id(), Some(&"p2".to_string()));

        // Preferred is invalid (nonexistent), fallback to first (p1)
        fm.realign_with_projects(&projects, Some("unknown"));
        assert_eq!(fm.focused_project_id(), Some(&"p1".to_string()));
        assert_eq!(fm.active_project_id(), Some(&"p1".to_string()));

        // Empty projects list clears everything
        fm.realign_with_projects(&[], None);
        assert_eq!(fm.focused_project_id(), None);
        assert_eq!(fm.active_project_id(), None);
    }

    #[test]
    fn focus_terminal_preserves_fullscreen() {
        let mut fm = FocusManager::new();
        fm.enter_fullscreen("proj1".to_string(), vec![0], "term1".to_string());
        assert!(fm.has_fullscreen());
        assert!(fm.is_terminal_fullscreened("proj1", "term1"));

        // Clicking a terminal while fullscreened should NOT exit fullscreen
        fm.focus_terminal("proj1".to_string(), vec![0]);
        assert!(fm.has_fullscreen());
        assert!(fm.is_terminal_fullscreened("proj1", "term1"));
        assert_eq!(fm.fullscreen_state(), Some(("proj1", "term1")));
    }

    #[test]
    fn exit_fullscreen_when_not_fullscreen_returns_none() {
        let mut fm = FocusManager::new();
        fm.focus_terminal("proj1".to_string(), vec![0]);
        let result = fm.exit_fullscreen();
        assert!(result.is_none());
    }

    #[test]
    fn zoom_to_project_saves_and_restores_focus() {
        let mut fm = FocusManager::new();
        // Focus a terminal in proj1 (on overview)
        fm.focus_terminal("proj1".to_string(), vec![0, 1]);

        // Zoom into proj2
        fm.set_focused_project_id(Some("proj2".to_string()));
        // Simulate focusing a terminal in proj2
        fm.focus_terminal("proj2".to_string(), vec![0]);
        assert_eq!(fm.focused_terminal_state().unwrap().project_id, "proj2");

        // Return to overview
        fm.set_focused_project_id(None);
        // Focus should be restored to proj1's terminal
        let state = fm.focused_terminal_state().unwrap();
        assert_eq!(state.project_id, "proj1");
        assert_eq!(state.layout_path, vec![0, 1]);
    }

    #[test]
    fn zoom_switch_between_projects_preserves_original_focus() {
        let mut fm = FocusManager::new();
        fm.focus_terminal("proj1".to_string(), vec![0]);

        // Zoom into proj2
        fm.set_focused_project_id(Some("proj2".to_string()));
        fm.focus_terminal("proj2".to_string(), vec![0]);

        // Switch zoom to proj3 (Some→Some, should NOT overwrite saved focus)
        fm.set_focused_project_id(Some("proj3".to_string()));
        fm.focus_terminal("proj3".to_string(), vec![0]);

        // Return to overview
        fm.set_focused_project_id(None);
        let state = fm.focused_terminal_state().unwrap();
        assert_eq!(state.project_id, "proj1");
    }

    #[test]
    fn zoom_individual_also_saves_and_restores_focus() {
        let mut fm = FocusManager::new();
        fm.focus_terminal("proj1".to_string(), vec![0]);

        // Zoom individual into proj2
        fm.set_focused_project_id_individual(Some("proj2".to_string()));
        fm.focus_terminal("proj2".to_string(), vec![0]);

        // Return to overview
        fm.set_focused_project_id(None);
        let state = fm.focused_terminal_state().unwrap();
        assert_eq!(state.project_id, "proj1");
    }

    /// Slice 03 of the multi-window plan moves `FocusManager` from a single
    /// field on `Workspace` to one-per-`WindowView`. This pins the contract
    /// the new ownership relies on: two independently-constructed instances
    /// are mutually isolated -- no static, shared, or interior-mutable state
    /// links them. Push/pop on every state-bearing surface (terminal focus,
    /// project zoom, fullscreen stack, modal stack) on one instance never
    /// leaks into the other's observable surface. A regression that
    /// introduced any shared state (e.g. a global focus counter, a shared
    /// stack arena) would surface here before the per-window refactor lands.
    #[test]
    fn two_instances_are_independent() {
        let mut a = FocusManager::new();
        let mut b = FocusManager::new();

        // Mutate A across every state-bearing surface.
        a.focus_terminal("p1".to_string(), vec![0]);
        a.set_focused_project_id(Some("p1".to_string()));
        a.enter_fullscreen("p1".to_string(), vec![0], "t1".to_string());
        a.enter_modal();

        // B is untouched on every observable surface.
        assert!(b.focused_terminal_state().is_none());
        assert_eq!(b.focused_project_id(), None);
        assert!(!b.has_fullscreen());
        assert!(!b.is_modal());
        assert_eq!(*b.context(), FocusContext::Terminal);

        // Unwind A's stack fully and clear it.
        let _ = a.exit_modal();
        let _ = a.exit_fullscreen();
        a.set_focused_project_id(None);
        a.clear_focus();

        // Mutate B; A must not silently flip back into a populated state.
        b.focus_terminal("p2".to_string(), vec![1, 2]);
        b.enter_fullscreen("p2".to_string(), vec![1, 2], "t2".to_string());

        // A is still cleared on every surface.
        assert!(a.focused_terminal_state().is_none());
        assert_eq!(a.focused_project_id(), None);
        assert!(!a.has_fullscreen());
        assert!(!a.is_modal());
        assert_eq!(*a.context(), FocusContext::Terminal);

        // B carries its own populated state, untainted by A's prior history.
        assert!(b.has_fullscreen());
        assert!(b.is_terminal_fullscreened("p2", "t2"));
        assert_eq!(b.focused_project_id(), Some(&"p2".to_string()));
    }
}
