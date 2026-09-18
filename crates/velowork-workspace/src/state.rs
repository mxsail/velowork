//! Workspace GPUI entity — coordinator over persistent data and transient
//! per-session state.
//!
//! Data types (`WorkspaceData`, `ProjectData`, `LayoutNode`, etc.) live in
//! `velowork-state` / `velowork-layout` and are re-exported here so existing
//! `crate::state::*` imports keep working.

use velowork_core::theme::FolderColor;
use crate::access_history::ProjectAccessHistory;
use crate::focus::FocusManager;
use crate::lifecycle::ProjectLifecycleTracker;
use crate::visibility::compute_visible_projects;
use gpui::*;
use std::collections::HashMap;

pub use velowork_layout::{LayoutNode, SplitDirection};
pub use velowork_state::{
    DropZone, FocusedTerminalState, FolderData, ProjectData, ProjectLayoutMode, WindowBounds,
    WindowId, WindowState, WorkspaceData,
};

/// Global workspace wrapper for app-wide access (used by quit handler)
#[derive(Clone)]
pub struct GlobalWorkspace(pub Entity<Workspace>);

impl Global for GlobalWorkspace {}

/// GPUI Entity for workspace state.
///
/// Composes focused helper types by ownership. `Workspace` itself is a
/// coordinator — it does not own the raw transient HashSets/HashMaps directly.
///
/// Per slice 03 of the multi-window plan, `FocusManager` is no longer a field
/// here; each `WindowView` owns its own. Action methods that touch focus state
/// take `focus_manager: &mut FocusManager` as a parameter so the focus
/// mutation stays scoped to the window driving the action.
pub struct Workspace {
    pub data: WorkspaceData,
    /// Transient project lifecycle state (creating / closing / removing).
    pub lifecycle: ProjectLifecycleTracker,
    /// Per-project last-access timestamps, for "recently used" sorting.
    pub access_history: ProjectAccessHistory,
    /// Monotonic counter incremented only on persistent data mutations.
    /// The auto-save observer compares this to skip saves for UI-only changes.
    data_version: u64,
    /// Monotonic counter incremented when all workspace data is replaced.
    data_replacement_epoch: u64,
    /// Terminal IDs queued for killing by the app layer (drained by Velowork observer).
    pending_terminal_kills: Vec<String>,
    /// Terminals closed with the grace-period "soft close": removed from the
    /// layout but their PTY is kept alive until the grace timer fires (or the
    /// user undoes / force-closes). Holds the snapshots needed to restore.
    pub(crate) pending_closes: Vec<PendingClose>,
    /// Terminals just brought back by an undo whose PTY might still be racing an
    /// in-flight exit event — see [`RestoredClose`].
    pub(crate) restored_closes: Vec<RestoredClose>,
}

/// A terminal that was soft-closed and is waiting out its grace period.
///
/// The PTY is still alive in the registry; only the layout entry was removed.
/// `pre_close_layout` / `post_close_layout` snapshot the owning project's tree
/// before and right after the close so undo can either restore the exact prior
/// tree (when nothing else changed) or fall back to re-appending the pane.
#[derive(Clone, Debug)]
pub struct PendingClose {
    pub terminal_id: String,
    pub project_id: String,
    pub toast_id: String,
    pub pre_close_layout: Option<LayoutNode>,
    pub post_close_layout: Option<LayoutNode>,
}

/// A terminal brought back by `undo_soft_close` whose PTY may still be racing an
/// in-flight exit event.
///
/// The `alive` check the undo path uses is registry-based, and the registry only
/// drops a terminal once the app *processes* its exit event — so a shell that has
/// already exited can still read as "alive", letting undo restore a doomed pane.
/// If that exit then lands, `reap_restored_close` tears the now-dead pane back out
/// of the layout (it can't be reconnected) instead of leaving it to linger — or to
/// silently respawn a fresh shell on the next render.
#[derive(Clone, Debug)]
pub struct RestoredClose {
    pub terminal_id: String,
    pub project_id: String,
}

impl Workspace {
    pub fn new(data: WorkspaceData) -> Self {
        Self {
            data,
            lifecycle: ProjectLifecycleTracker::new(),
            access_history: ProjectAccessHistory::new(),
            data_version: 0,
            data_replacement_epoch: 0,
            pending_terminal_kills: Vec::new(),
            pending_closes: Vec::new(),
            restored_closes: Vec::new(),
        }
    }

    /// Current data version (incremented on persistent data mutations)
    pub fn data_version(&self) -> u64 {
        self.data_version
    }

    /// Current wholesale data replacement epoch.
    pub fn data_replacement_epoch(&self) -> u64 {
        self.data_replacement_epoch
    }

    /// Read-only access to persistent workspace data.
    pub fn data(&self) -> &WorkspaceData {
        &self.data
    }

    /// Notify that persistent data changed. Bumps version, calls cx.notify(),
    /// and refreshes all windows to bypass `.cached()` view wrappers.
    /// Use this instead of cx.notify() when mutating `self.data`.
    pub fn notify_data(&mut self, cx: &mut Context<Self>) {
        self.data_version += 1;
        cx.notify();
        cx.refresh_windows();
    }

    /// Replace workspace data wholesale (e.g. from disk reload).
    /// Does NOT bump data_version — the data came from disk, not a user edit.
    pub fn replace_data(&mut self, focus_manager: &mut FocusManager, data: WorkspaceData, cx: &mut Context<Self>) {
        self.data = data;
        self.data_replacement_epoch += 1;
        // Snapshots in pending_closes refer to the old data — drop them so an
        // undo can't restore into a wholesale-replaced workspace. The
        // restore-race breadcrumbs refer to the old layout too.
        self.pending_closes.clear();
        self.restored_closes.clear();
        focus_manager.clear_all();
        focus_manager.realign_with_projects(
            &self.data.projects,
            self.data.main_window.focused_project_id.as_deref(),
        );
        cx.notify();
        cx.refresh_windows();
    }

    /// Record that a project was accessed (for sorting by recency)
    pub fn touch_project(&mut self, project_id: &str) {
        self.access_history.touch(project_id);
    }

    /// Mark an SSH session as connected (delegates to `ConnectionStore`).
    pub fn mark_ssh_session_connected(&mut self, session_id: &str, cx: &mut Context<Self>) {
        let store = cx.global::<crate::stores::GlobalConnectionStore>().0.clone();
        store.update(cx, |c, cx| c.mark_connected(session_id, cx));
    }

    /// Mark an SSH session as disconnected (delegates to `ConnectionStore`).
    pub fn mark_ssh_session_disconnected(&mut self, session_id: &str, cx: &mut Context<Self>) {
        let store = cx.global::<crate::stores::GlobalConnectionStore>().0.clone();
        store.update(cx, |c, cx| c.mark_disconnected(session_id, cx));
    }

    /// Check if an SSH session is currently connected (reads `ConnectionStore`).
    pub fn is_ssh_session_connected(&self, session_id: &str, cx: &App) -> bool {
        cx.global::<crate::stores::GlobalConnectionStore>().0.read(cx).is_connected(session_id)
    }

    /// Record meaningful activity for a project: stamp `last_activity_at` with
    /// the current unix-millis and persist it (drives the activity-sorted
    /// sidebar view). Called on focus, a finished command (OSC 133 ;D), and a
    /// bell/notification from one of the project's terminals — deliberately NOT
    /// on raw terminal output, since output volume is not "activity". A no-op
    /// for an unknown project id. Uses `notify_data` so the change is persisted
    /// (debounced) and the sidebar re-renders to reorder.
    pub fn bump_activity(&mut self, project_id: &str, cx: &mut Context<Self>) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        if let Some(project) = self.project_mut(project_id) {
            project.last_activity_at = Some(now);
            self.notify_data(cx);
        }
    }

    /// Get projects sorted by last access time (most recent first)
    pub fn projects_by_recency(&self) -> Vec<&ProjectData> {
        let mut projects: Vec<&ProjectData> = self.data.projects.iter().collect();
        projects.sort_by(|a, b| self.access_history.cmp_by_recency(&a.id, &b.id));
        projects
    }

    /// Current folder filter for the targeted window's viewport.
    ///
    /// Routes through `data.window(window_id)` (the lookup pair on
    /// `WorkspaceData`): `WindowId::Main` always returns the main slot,
    /// `WindowId::Extra(uuid)` walks `extra_windows`. Unknown extra ids
    /// (a paint racing a close) yield `None` -- the same default used when
    /// the targeted window has no folder_filter set. Mirrors the silent
    /// no-op shape of the window-scoped setters.
    pub fn active_folder_filter(&self, window_id: WindowId) -> Option<&String> {
        self.data
            .window(window_id)
            .and_then(|w| w.folder_filter.as_ref())
    }

    /// Set the folder filter on the targeted window.
    ///
    /// Delegates to `data.set_folder_filter`, which writes to the targeted
    /// window's `WindowState::folder_filter`. Unknown extra ids are a silent
    /// no-op (the targeted window was just closed).
    ///
    /// Bumps `data_version` because folder_filter is persisted -- the
    /// auto-save observer must trigger.
    pub fn set_folder_filter(
        &mut self,
        window_id: WindowId,
        folder_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.data.set_folder_filter(window_id, folder_id);
        self.notify_data(cx);
    }

    /// Toggle a project's hidden state in the targeted window.
    ///
    /// Delegates to `data.toggle_hidden`, which inserts the project id into
    /// the targeted window's `hidden_project_ids` if absent and removes it if
    /// present. Unknown extra ids are a silent no-op (the targeted window
    /// was just closed).
    ///
    /// Bumps `data_version` because hidden state is persisted -- the
    /// auto-save observer must trigger.
    pub fn toggle_hidden(
        &mut self,
        window_id: WindowId,
        project_id: &str,
        cx: &mut Context<Self>,
    ) {
        self.data.toggle_hidden(window_id, project_id);
        self.notify_data(cx);
    }

    /// Set a single project's column width on the targeted window.
    ///
    /// Delegates to `data.set_project_width`, which writes the
    /// (project_id, width) pair into the targeted window's
    /// `project_widths` map, overwriting any prior value. Unknown extra
    /// ids are a silent no-op (the targeted window was just closed).
    ///
    /// Bumps `data_version` because project widths are persisted -- the
    /// auto-save observer must trigger.
    pub fn set_project_width(
        &mut self,
        window_id: WindowId,
        project_id: &str,
        width: f32,
        cx: &mut Context<Self>,
    ) {
        self.data.set_project_width(window_id, project_id, width);
        self.notify_data(cx);
    }

    /// Set a folder's collapsed state on the targeted window.
    ///
    /// Delegates to `data.set_folder_collapsed`, which inserts
    /// `(folder_id, true)` into the targeted window's `folder_collapsed`
    /// when `collapsed=true`, or removes any existing entry when
    /// `collapsed=false` (the "absence == expanded" runtime convention).
    /// Unknown extra ids are a silent no-op (the targeted window was just
    /// closed).
    ///
    /// Bumps `data_version` because folder-collapsed state is persisted --
    /// the auto-save observer must trigger.
    pub fn set_folder_collapsed(
        &mut self,
        window_id: WindowId,
        folder_id: &str,
        collapsed: bool,
        cx: &mut Context<Self>,
    ) {
        self.data.set_folder_collapsed(window_id, folder_id, collapsed);
        self.notify_data(cx);
    }

    /// Set the OS window bounds on the targeted window.
    ///
    /// Delegates to `data.set_os_bounds`, which writes the
    /// `Option<WindowBounds>` into the targeted window's `os_bounds` slot.
    /// `Some(bounds)` records the latest OS-reported origin/size so the next
    /// launch can restore the window in the same place; `None` clears the
    /// slot (the next launch falls back to the OS default / cascade-offset).
    /// Unknown extra ids are a silent no-op (the targeted window was just
    /// closed -- a debounced bounds-observer firing after a close lands on
    /// a no-op rather than panicking).
    ///
    /// Bumps `data_version` because os_bounds is persisted -- the auto-save
    /// observer must trigger.
    pub fn set_os_bounds(
        &mut self,
        window_id: WindowId,
        bounds: Option<WindowBounds>,
        cx: &mut Context<Self>,
    ) {
        self.data.set_os_bounds(window_id, bounds);
        self.notify_data(cx);
    }

    /// Set dock open/closed state for the targeted window and position.
    pub fn set_dock_open(
        &mut self,
        window_id: WindowId,
        pos: velowork_core::types::DockPosition,
        open: bool,
        cx: &mut Context<Self>,
    ) {
        self.data.set_dock_open(window_id, pos, open);
        self.notify_data(cx);
    }

    /// Set dock size for the targeted window and position.
    pub fn set_dock_size(
        &mut self,
        window_id: WindowId,
        pos: velowork_core::types::DockPosition,
        size: f32,
        cx: &mut Context<Self>,
    ) {
        self.data.set_dock_size(window_id, pos, size);
        self.notify_data(cx);
    }

    pub fn set_sidebar_open(
        &mut self,
        window_id: WindowId,
        open: bool,
        cx: &mut Context<Self>,
    ) {
        self.set_dock_open(window_id, velowork_core::types::DockPosition::Left, open, cx);
    }

    pub fn set_right_sidebar_open(
        &mut self,
        window_id: WindowId,
        open: bool,
        cx: &mut Context<Self>,
    ) {
        self.set_dock_open(window_id, velowork_core::types::DockPosition::Right, open, cx);
    }

    pub fn set_right_sidebar_width(
        &mut self,
        window_id: WindowId,
        width: f32,
        cx: &mut Context<Self>,
    ) {
        self.set_dock_size(window_id, velowork_core::types::DockPosition::Right, width, cx);
    }

    /// Persist the focused/zoomed project ID for the targeted window.
    /// Called from `set_focused_project` so the value survives app restarts.
    pub fn persist_focused_project_id(
        &mut self,
        window_id: WindowId,
        project_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.data.set_focused_project_id(window_id, project_id);
        self.notify_data(cx);
    }

    /// Read the project-grid orientation for the targeted window. Falls back
    /// to the default (`Columns`) for an unknown window id.
    pub fn project_layout_mode(&self, window_id: WindowId) -> ProjectLayoutMode {
        self.data
            .window(window_id)
            .map(|w| w.project_layout)
            .unwrap_or_default()
    }

    /// Flip the targeted window's project grid between columns and rows, and
    /// transpose every terminal split inside the projects shown in that window.
    ///
    /// Flipping the grid axis without transposing the panes inside would leave
    /// each project's internal splits running the "wrong" way relative to the
    /// new grid orientation; transposing keeps the whole window's layout
    /// visually consistent through the switch. Only projects visible in *this*
    /// window are touched (using the window's persistent visibility, ignoring
    /// transient focus narrowing), so a project shown in another window keeps
    /// its own pane orientation.
    ///
    /// Percentages in `project_widths` are axis-agnostic, so relative grid
    /// sizing is preserved across the flip. Persisted via `notify_data`.
    pub fn toggle_project_layout_mode(&mut self, window_id: WindowId, cx: &mut Context<Self>) {
        let Some(window_state) = self.data.window(window_id) else {
            return;
        };

        // Collect the IDs of projects shown in this window before mutating, so
        // the borrow of `window_state` is released before we mutate layouts.
        let visible_ids: Vec<String> =
            compute_visible_projects(&self.data, None, false, window_state)
                .into_iter()
                .map(|p| p.id.clone())
                .collect();

        for project in &mut self.data.projects {
            if visible_ids.iter().any(|id| id == &project.id)
                && let Some(layout) = project.layout.as_mut()
            {
                layout.transpose();
            }
        }

        if let Some(w) = self.data.window_mut(window_id) {
            w.project_layout = w.project_layout.toggled();
        }
        self.notify_data(cx);
    }

    /// Flip the sidebar project sort mode (manual ↔ activity) for a window.
    /// Persisted via `notify_data`.
    pub fn toggle_project_sort_mode(&mut self, window_id: WindowId, cx: &mut Context<Self>) {
        if self.data.toggle_project_sort_mode(window_id).is_some() {
            self.notify_data(cx);
        }
    }

    /// Flip the "needs attention" section opt-in for a window's manual view.
    /// Persisted via `notify_data`.
    pub fn toggle_show_attention_section(&mut self, window_id: WindowId, cx: &mut Context<Self>) {
        if self.data.toggle_show_attention_section(window_id).is_some() {
            self.notify_data(cx);
        }
    }

    /// Toggle whether a project is pinned to the top of the activity-sorted
    /// view. No-op for an unknown project id. Persisted via `notify_data`.
    pub fn toggle_project_pinned(&mut self, project_id: &str, cx: &mut Context<Self>) {
        if let Some(project) = self.project_mut(project_id) {
            project.pinned = !project.pinned;
            self.notify_data(cx);
        }
    }

    /// Spawn a fresh extra window onto `extra_windows` and return its id.
    ///
    /// Delegates to `data.spawn_extra_window`, which appends a new
    /// `WindowState` whose `hidden_project_ids` snapshots every current
    /// project ID (so the spawned window's grid is empty at first render --
    /// the user curates it via the per-window "Show in this window" sidebar
    /// action). The returned `WindowId::Extra(uuid)` is the handle the
    /// observer in `src/app/extras.rs` uses to look the corresponding
    /// `Entity<WindowView>` up in `Velowork::extra_windows`.
    ///
    /// `spawning_bounds` carries the live OS bounds of the window that
    /// triggered the spawn (read by the action handler from
    /// `gpui::Window::window_bounds()`). When `Some`, the data layer
    /// seeds the new entry's `os_bounds` with origin shifted by `+30,+30`
    /// (the cascade-offset rule); the observer then passes that
    /// `os_bounds` straight into `cx.open_window`'s `window_bounds` so
    /// the OS positions the new window cascade-offset from its parent.
    /// When `None`, `os_bounds` stays `None` and the OS picks a default
    /// position.
    ///
    /// Bumps `data_version` because the new entry is persisted -- the
    /// auto-save observer must trigger so a freshly-spawned extra survives
    /// a quit-during-spawn race.
    pub fn spawn_extra_window(
        &mut self,
        spawning_bounds: Option<WindowBounds>,
        cx: &mut Context<Self>,
    ) -> WindowId {
        let id = self.data.spawn_extra_window(spawning_bounds);
        self.notify_data(cx);
        id
    }

    /// Drop the extra window entry from `extra_windows`.
    ///
    /// Slice 07 cri 3 lifecycle counterpart to `spawn_extra_window` —
    /// the close-flow in `src/app/extras.rs::open_extra_window`'s
    /// `on_window_should_close` hook calls this when the user closes an
    /// extra OS window so the entry stops being persisted (PRD user
    /// story 22). Delegates to `data.close_extra_window`, which retains
    /// every entry whose `state.id != uuid`.
    ///
    /// `WindowId::Main` is a silent no-op at the data layer (main is
    /// the always-present slot). `WindowId::Extra(uuid)` for an unknown
    /// extra (double-close race) is also a silent no-op.
    ///
    /// Bumps `data_version` because removing an entry shrinks the
    /// persisted state — the auto-save observer must trigger so the
    /// next launch (slice 07 cri 6) does not see the closed extra
    /// reappear.
    pub fn close_extra_window(&mut self, id: WindowId, cx: &mut Context<Self>) {
        self.data.close_extra_window(id);
        self.notify_data(cx);
    }

    // === ProjectLifecycleTracker conveniences ===

    pub fn is_creating_project(&self, project_id: &str) -> bool {
        self.lifecycle.is_creating(project_id)
    }

    pub fn mark_creating_project(&mut self, project_id: &str) {
        self.lifecycle.mark_creating(project_id);
    }

    pub fn finish_creating_project(&mut self, project_id: &str) {
        self.lifecycle.finish_creating(project_id);
    }

    pub fn finish_closing_project(&mut self, project_id: &str) {
        self.lifecycle.finish_closing(project_id);
    }

    // === Terminal kill queue ===

    pub fn queue_terminal_kills(&mut self, ids: impl IntoIterator<Item = String>) {
        self.pending_terminal_kills.extend(ids);
    }

    pub fn drain_pending_terminal_kills(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_terminal_kills)
    }

    /// Update the saved service terminal IDs for a project.
    /// Called by the ServiceManager observer to persist terminal IDs across restarts.
    pub fn sync_service_terminals(&mut self, project_id: &str, terminals: HashMap<String, String>, cx: &mut Context<Self>) {
        if let Some(project) = self.project_mut(project_id)
            && project.service_terminals != terminals {
                project.service_terminals = terminals;
                self.notify_data(cx);
            }
    }

    /// Find the project that owns a terminal by scanning project layouts.
    /// Returns a reference to the `ProjectData` if found.
    pub fn find_project_for_terminal(&self, terminal_id: &str) -> Option<&ProjectData> {
        self.data.projects.iter().find(|p| {
            p.layout.as_ref().is_some_and(|l| l.find_terminal_path(terminal_id).is_some())
        })
    }

    /// Check if a project is currently being closed (hook running or removal in progress).
    pub fn is_project_closing(&self, project_id: &str) -> bool {
        self.lifecycle.is_closing(project_id)
    }

    pub fn projects(&self) -> &[ProjectData] {
        &self.data.projects
    }

    /// Get visible projects in order, expanding folders into their contained projects.
    /// When a folder filter is active, only projects from that folder are shown
    /// (top-level projects are hidden). Focused project override still takes priority.
    ///
    /// Per slice 03 of the multi-window plan, callers pass the focused
    /// project id and individual-mode flag from their per-window
    /// `FocusManager` -- visibility is now scoped to the calling window.
    pub fn visible_projects(
        &self,
        window_id: WindowId,
        focused_project_id: Option<&String>,
        focus_individual: bool,
    ) -> Vec<&ProjectData> {
        // Source folder filter / hidden set / widths / collapse from the
        // calling window's persisted WindowState. Fall back to main_window
        // if the targeted extra has been dropped between caller-resolve and
        // read (drop-race safety).
        let window_state = self.data.window(window_id).unwrap_or(&self.data.main_window);
        compute_visible_projects(
            &self.data,
            focused_project_id,
            focus_individual,
            window_state,
        )
    }

    /// Union of project IDs visible in *any* window (main + extras).
    ///
    /// Uses each window's persistent visibility (folder filter + hidden set)
    /// but deliberately *not* the transient fullscreen-focus narrowing
    /// (`focus_individual`), so the set stays stable as focus moves around.
    /// Used by the git-status watcher to scope expensive `gh` PR/CI polling to
    /// projects the user can actually see somewhere.
    pub fn all_visible_project_ids(&self) -> std::collections::HashSet<String> {
        let mut ids = std::collections::HashSet::new();
        for window in
            std::iter::once(&self.data.main_window).chain(self.data.extra_windows.iter())
        {
            for p in compute_visible_projects(&self.data, None, false, window) {
                ids.insert(p.id.clone());
            }
        }
        ids
    }

    /// Get a project by ID
    pub fn project(&self, id: &str) -> Option<&ProjectData> {
        self.data.projects.iter().find(|p| p.id == id)
    }

    /// Get the effective folder color for a project.
    pub fn effective_folder_color(&self, project: &ProjectData) -> FolderColor {
        project.folder_color
    }

    /// Get a mutable project by ID
    pub(crate) fn project_mut(&mut self, id: &str) -> Option<&mut ProjectData> {
        self.data.projects.iter_mut().find(|p| p.id == id)
    }

    /// Get a folder by ID
    pub fn folder(&self, id: &str) -> Option<&FolderData> {
        self.data.folders.iter().find(|f| f.id == id)
    }

    /// Get a mutable folder by ID
    pub(crate) fn folder_mut(&mut self, id: &str) -> Option<&mut FolderData> {
        self.data.folders.iter_mut().find(|f| f.id == id)
    }

    /// Check if an ID in project_order refers to a folder
    #[allow(dead_code)]
    pub fn is_folder(&self, id: &str) -> bool {
        self.data.folders.iter().any(|f| f.id == id)
    }

    /// Find which folder (if any) contains a given project
    pub fn folder_for_project(&self, project_id: &str) -> Option<&FolderData> {
        self.data.folders.iter().find(|f| f.project_ids.contains(&project_id.to_string()))
    }

    /// Collect all detached terminals across all projects by traversing layout trees.
    /// Returns (terminal_id, project_id, layout_path) tuples.
    pub fn collect_all_detached_terminals(&self) -> Vec<(String, String, Vec<usize>)> {
        let mut result = Vec::new();
        for project in &self.data.projects {
            if let Some(ref layout) = project.layout {
                for (terminal_id, layout_path, _) in layout.collect_detached_terminals() {
                    result.push((terminal_id, project.id.clone(), layout_path));
                }
            }
        }
        result
    }

    /// Notify UI without bumping data_version (for remote state changes that shouldn't trigger auto-save).
    pub fn notify_ui_only(&mut self, cx: &mut Context<Self>) {
        cx.notify();
    }

    /// Helper to mutate a layout node at a path, with automatic notify.
    /// Returns true if the mutation was applied.
    pub fn with_layout_node<F>(&mut self, project_id: &str, path: &[usize], cx: &mut Context<Self>, f: F) -> bool
    where
        F: FnOnce(&mut LayoutNode) -> bool,
    {
        if let Some(project) = self.project_mut(project_id)
            && let Some(ref mut layout) = project.layout
                && let Some(node) = layout.get_at_path_mut(path)
                    && f(node) {
                        self.notify_data(cx);
                        return true;
                    }
        false
    }

    /// Helper to mutate a project, with automatic notify.
    /// Returns true if the mutation was applied.
    pub fn with_project<F>(&mut self, project_id: &str, cx: &mut Context<Self>, f: F) -> bool
    where
        F: FnOnce(&mut ProjectData) -> bool,
    {
        if let Some(project) = self.project_mut(project_id)
            && f(project) {
                self.notify_data(cx);
                return true;
            }
        false
    }
}



#[cfg(test)]
mod workspace_tests {
    use crate::state::{
        FolderData, LayoutNode, ProjectData, SplitDirection, WindowId, WindowState, Workspace,
        WorkspaceData,
    };
    use velowork_terminal::shell_config::ShellType;
    use velowork_core::theme::FolderColor;
    use std::collections::HashMap;

    fn make_project(id: &str) -> ProjectData {
        ProjectData {
            id: id.to_string(),
            name: format!("Project {}", id),
            path: "/tmp/test".to_string(),
            layout: Some(LayoutNode::Terminal {
                terminal_id: Some(format!("term_{}", id)),
                minimized: false,
                detached: false,
                shell_type: ShellType::Default,
                zoom_level: 1.0,
            }),
            terminal_names: HashMap::new(),
            hidden_terminals: HashMap::new(),
            folder_color: FolderColor::default(),
            is_remote: false,
            connection_id: None,
            service_terminals: HashMap::new(),
            default_shell: None,
            pinned: false,
            last_activity_at: None,
            ..Default::default()
        }
    }

    fn make_workspace_data(projects: Vec<ProjectData>, order: Vec<&str>) -> WorkspaceData {
        // Per-window viewport model: hidden state lives on
        // `main_window.hidden_project_ids` and is populated explicitly by
        // tests that exercise hidden-project behavior. The legacy
        // `ProjectData.show_in_overview` shortcut has been removed.
        WorkspaceData {
            version: 1,
            projects,
            project_order: order.into_iter().map(String::from).collect(),
            service_panel_heights: HashMap::new(),
            folders: Vec::new(),
            main_window: WindowState::default(),
            extra_windows: Vec::new(),
        }
    }

    #[test]
    fn test_visible_projects_filters_hidden() {
        let mut data = make_workspace_data(
            vec![make_project("p1"), make_project("p2"), make_project("p3")],
            vec!["p1", "p2", "p3"],
        );
        data.main_window.hidden_project_ids.insert("p2".to_string());
        let ws = Workspace::new(data);

        let visible = ws.visible_projects(WindowId::Main, None, false);
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].id, "p1");
        assert_eq!(visible[1].id, "p3");
    }

    #[test]
    fn test_visible_projects_with_focused_project() {
        let mut data = make_workspace_data(
            vec![make_project("p1"), make_project("p2"), make_project("p3")],
            vec!["p1", "p2", "p3"],
        );
        data.main_window.hidden_project_ids.insert("p3".to_string());
        let ws = Workspace::new(data);

        let mut fm = crate::focus::FocusManager::new();
        fm.set_focused_project_id(Some("p3".to_string()));

        let visible = ws.visible_projects(WindowId::Main, fm.focused_project_id(), fm.is_focus_individual());
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].id, "p3");
    }

    #[test]
    fn test_visible_projects_with_folder() {
        let mut data = make_workspace_data(
            vec![make_project("p1"), make_project("p2")],
            vec!["f1"],
        );
        data.folders = vec![FolderData {
            id: "f1".to_string(),
            name: "Folder".to_string(),
            project_ids: vec!["p1".to_string(), "p2".to_string()],
            folder_color: FolderColor::default(),
        }];

        let ws = Workspace::new(data);

        let visible = ws.visible_projects(WindowId::Main, None, false);
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].id, "p1");
        assert_eq!(visible[1].id, "p2");
    }

    #[test]
    fn test_projects_by_recency() {
        let data = make_workspace_data(
            vec![make_project("p1"), make_project("p2"), make_project("p3")],
            vec!["p1", "p2", "p3"],
        );
        let mut ws = Workspace::new(data);

        ws.touch_project("p3");
        ws.touch_project("p1");

        let recency = ws.projects_by_recency();
        assert_eq!(recency[0].id, "p1");
        assert_eq!(recency[1].id, "p3");
        assert_eq!(recency[2].id, "p2");
    }

    #[test]
    fn test_collect_all_detached_terminals() {
        let mut project = make_project("p1");
        project.layout = Some(LayoutNode::Split {
            direction: SplitDirection::Horizontal,
            sizes: vec![50.0, 50.0],
            children: vec![
                LayoutNode::Terminal {
                    terminal_id: Some("t1".to_string()),
                    minimized: false,
                    detached: true,
                    shell_type: ShellType::Default,
                    zoom_level: 1.0,
                },
                LayoutNode::Terminal {
                    terminal_id: Some("t2".to_string()),
                    minimized: false,
                    detached: false,
                    shell_type: ShellType::Default,
                    zoom_level: 1.0,
                },
            ],
        });
        let data = make_workspace_data(vec![project], vec!["p1"]);
        let ws = Workspace::new(data);

        let detached = ws.collect_all_detached_terminals();
        assert_eq!(detached.len(), 1);
        assert_eq!(detached[0].0, "t1");
        assert_eq!(detached[0].1, "p1");
        assert_eq!(detached[0].2, vec![0]);
    }

    #[test]
    fn test_folder_for_project() {
        let mut data = make_workspace_data(
            vec![make_project("p1"), make_project("p2")],
            vec!["f1", "p2"],
        );
        data.folders = vec![FolderData {
            id: "f1".to_string(),
            name: "Folder".to_string(),
            project_ids: vec!["p1".to_string()],
            folder_color: FolderColor::default(),
        }];
        let ws = Workspace::new(data);

        assert_eq!(ws.folder_for_project("p1").unwrap().id, "f1");
        assert!(ws.folder_for_project("p2").is_none());
    }

    #[test]
    fn test_visible_projects_with_folder_filter() {
        let mut data = make_workspace_data(
            vec![
                make_project("p1"), make_project("p2"),
                make_project("p3"), make_project("p4"),
                make_project("p5"),
            ],
            vec!["f1", "f2", "p5"],
        );
        data.folders = vec![
            FolderData {
                id: "f1".to_string(),
                name: "Folder 1".to_string(),
                project_ids: vec!["p1".to_string(), "p2".to_string()],
                    folder_color: FolderColor::default(),
            },
            FolderData {
                id: "f2".to_string(),
                name: "Folder 2".to_string(),
                project_ids: vec!["p3".to_string(), "p4".to_string()],
                    folder_color: FolderColor::default(),
            },
        ];

        let mut ws = Workspace::new(data);

        assert_eq!(ws.visible_projects(WindowId::Main, None, false).len(), 5);

        ws.data.main_window.folder_filter = Some("f1".to_string());
        let visible = ws.visible_projects(WindowId::Main, None, false);
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].id, "p1");
        assert_eq!(visible[1].id, "p2");

        ws.data.main_window.folder_filter = Some("f2".to_string());
        let visible = ws.visible_projects(WindowId::Main, None, false);
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].id, "p3");
        assert_eq!(visible[1].id, "p4");
    }

    #[test]
    fn test_folder_filter_hides_top_level_projects() {
        let mut data = make_workspace_data(
            vec![
                make_project("p1"), make_project("p2"),
                make_project("p3"),
            ],
            vec!["f1", "p3"],
        );
        data.folders = vec![FolderData {
            id: "f1".to_string(),
            name: "Folder".to_string(),
            project_ids: vec!["p1".to_string(), "p2".to_string()],
            folder_color: FolderColor::default(),
        }];

        let mut ws = Workspace::new(data);
        ws.data.main_window.folder_filter = Some("f1".to_string());

        let visible = ws.visible_projects(WindowId::Main, None, false);
        assert_eq!(visible.len(), 2);
        assert!(visible.iter().all(|p| p.id != "p3"));
    }


    #[test]
    fn test_folder_filter_with_focus_override() {
        let mut data = make_workspace_data(
            vec![
                make_project("p1"), make_project("p2"),
                make_project("p3"),
            ],
            vec!["f1", "p3"],
        );
        data.folders = vec![FolderData {
            id: "f1".to_string(),
            name: "Folder".to_string(),
            project_ids: vec!["p1".to_string(), "p2".to_string()],
            folder_color: FolderColor::default(),
        }];

        let mut ws = Workspace::new(data);
        ws.data.main_window.folder_filter = Some("f1".to_string());

        let mut fm = crate::focus::FocusManager::new();
        fm.set_focused_project_id(Some("p3".to_string()));

        let visible = ws.visible_projects(WindowId::Main, fm.focused_project_id(), fm.is_focus_individual());
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].id, "p3");
    }

    #[test]
    fn visible_projects_reads_folder_filter_from_main_window() {
        // visible_projects must source the folder filter from
        // `data.main_window.folder_filter` (the persisted, per-window
        // viewport model). A regression that re-introduces a transient
        // override on the entity would see None and return all 3 projects
        // instead of just f1's 2.
        let mut data = make_workspace_data(
            vec![make_project("p1"), make_project("p2"), make_project("p3")],
            vec!["f1", "p3"],
        );
        data.folders = vec![FolderData {
            id: "f1".to_string(),
            name: "Folder".to_string(),
            project_ids: vec!["p1".to_string(), "p2".to_string()],
            folder_color: FolderColor::default(),
        }];
        data.main_window.folder_filter = Some("f1".to_string());
        let ws = Workspace::new(data);

        let visible = ws.visible_projects(WindowId::Main, None, false);
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].id, "p1");
        assert_eq!(visible[1].id, "p2");
    }
}

#[cfg(test)]
mod gpui_tests {
    use gpui::AppContext as _;
    use crate::state::{LayoutNode, ProjectData, WindowBounds, WindowId, WindowState, Workspace, WorkspaceData};
    use velowork_terminal::shell_config::ShellType;
    use velowork_core::theme::FolderColor;
    use std::collections::HashMap;

    fn make_project(id: &str) -> ProjectData {
        ProjectData {
            id: id.to_string(),
            name: format!("Project {}", id),
            path: "/tmp/test".to_string(),
            layout: Some(LayoutNode::Terminal {
                terminal_id: Some(format!("term_{}", id)),
                minimized: false,
                detached: false,
                shell_type: ShellType::Default,
                zoom_level: 1.0,
            }),
            terminal_names: HashMap::new(),
            hidden_terminals: HashMap::new(),
            folder_color: FolderColor::default(),
            is_remote: false,
            connection_id: None,
            service_terminals: HashMap::new(),
            default_shell: None,
            pinned: false,
            last_activity_at: None,
            ..Default::default()
        }
    }

    fn make_workspace_data(projects: Vec<ProjectData>, order: Vec<&str>) -> WorkspaceData {
        // Per-window viewport model: hidden state lives on
        // `main_window.hidden_project_ids` and is set explicitly by tests
        // that exercise hidden-project behavior.
        WorkspaceData {
            version: 1,
            projects,
            project_order: order.into_iter().map(String::from).collect(),
            service_panel_heights: HashMap::new(),
            folders: vec![],
            main_window: WindowState::default(),
            extra_windows: Vec::new(),
        }
    }

    #[gpui::test]
    fn test_with_layout_node_applies_mutation(cx: &mut gpui::TestAppContext) {
        let data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        let workspace = cx.new(|_cx| Workspace::new(data));

        let result = workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.with_layout_node("p1", &[], cx, |node| {
                if let LayoutNode::Terminal { minimized, .. } = node {
                    *minimized = true;
                    true
                } else {
                    false
                }
            })
        });
        assert!(result);

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            let layout = ws.project("p1").unwrap().layout.as_ref().unwrap();
            match layout {
                LayoutNode::Terminal { minimized, .. } => assert!(*minimized),
                _ => panic!("Expected terminal"),
            }
            assert_eq!(ws.data_version(), 1);
        });
    }

    #[gpui::test]
    fn test_with_layout_node_invalid_path_returns_false(cx: &mut gpui::TestAppContext) {
        let data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        let workspace = cx.new(|_cx| Workspace::new(data));

        let result = workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.with_layout_node("p1", &[99], cx, |_node| true)
        });
        assert!(!result);

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            assert_eq!(ws.data_version(), 0);
        });
    }

    #[gpui::test]
    fn test_with_layout_node_invalid_project_returns_false(cx: &mut gpui::TestAppContext) {
        let data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        let workspace = cx.new(|_cx| Workspace::new(data));

        let result = workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.with_layout_node("nonexistent", &[], cx, |_node| true)
        });
        assert!(!result);

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            assert_eq!(ws.data_version(), 0);
        });
    }


    #[gpui::test]
    fn test_replace_data_realigns_focus(cx: &mut gpui::TestAppContext) {
        use crate::focus::FocusManager;

        let data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        let workspace = cx.new(|_cx| Workspace::new(data));
        let mut fm = FocusManager::new();

        fm.set_focused_project_id(Some("p1".to_string()));
        assert!(fm.focused_project_id().is_some());

        let new_data = make_workspace_data(vec![make_project("p2")], vec!["p2"]);
        workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.replace_data(&mut fm, new_data, cx);
        });

        assert_eq!(fm.focused_project_id(), Some(&"p2".to_string()));
        assert_eq!(fm.active_project_id(), Some(&"p2".to_string()));
        workspace.read_with(cx, |ws: &Workspace, _cx| {
            assert_eq!(ws.data().projects.len(), 1);
            assert_eq!(ws.data().projects[0].id, "p2");
        });
    }

    #[gpui::test]
    fn test_visible_projects_gpui(cx: &mut gpui::TestAppContext) {
        let p1 = make_project("p1");
        let p2 = make_project("p2");
        let p3 = make_project("p3");
        let mut data = make_workspace_data(vec![p1, p2, p3], vec!["p1", "p2", "p3"]);
        data.main_window.hidden_project_ids.insert("p1".to_string());
        data.main_window.hidden_project_ids.insert("p3".to_string());
        let workspace = cx.new(|_cx| Workspace::new(data));

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            let visible = ws.visible_projects(WindowId::Main, None, false);
            assert_eq!(visible.len(), 1);
            assert_eq!(visible[0].id, "p2");
        });

        workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.toggle_project_overview_visibility(&mut crate::focus::FocusManager::new(), WindowId::Main, "p1", cx);
        });

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            let visible = ws.visible_projects(WindowId::Main, None, false);
            assert_eq!(visible.len(), 2);
            assert_eq!(visible[0].id, "p1");
            assert_eq!(visible[1].id, "p2");
        });
    }

    fn make_remote_project(id: &str, conn_id: &str) -> ProjectData {
        let mut p = make_project(id);
        p.is_remote = true;
        p.connection_id = Some(conn_id.to_string());
        p
    }

    #[gpui::test]
    fn test_visible_projects_includes_remote_in_folders(cx: &mut gpui::TestAppContext) {
        use crate::state::FolderData;

        let local = make_project("local1");
        let remote1 = make_remote_project("remote:conn1:p1", "conn1");
        let remote2 = make_remote_project("remote:conn1:p2", "conn1");

        let mut data = make_workspace_data(
            vec![local, remote1, remote2],
            vec!["local1", "remote:conn1:folder1"],
        );
        data.main_window.hidden_project_ids.insert("remote:conn1:p2".to_string());
        data.folders.push(FolderData {
            id: "remote:conn1:folder1".to_string(),
            name: "Server 1".to_string(),
            project_ids: vec!["remote:conn1:p1".to_string(), "remote:conn1:p2".to_string()],
            folder_color: FolderColor::default(),
        });

        let workspace = cx.new(|_cx| Workspace::new(data));

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            let visible = ws.visible_projects(WindowId::Main, None, false);
            assert_eq!(visible.len(), 2);
            assert_eq!(visible[0].id, "local1");
            assert_eq!(visible[1].id, "remote:conn1:p1");
        });
    }

    #[gpui::test]
    fn set_folder_filter_main_writes_to_data(cx: &mut gpui::TestAppContext) {
        // Window-scoped entity setter: WindowId::Main writes to
        // data.main_window.folder_filter (the persisted source of truth).
        // data_version bumps because folder_filter is persisted -- the
        // auto-save observer must trigger.
        let data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        let workspace = cx.new(|_cx| Workspace::new(data));

        workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.set_folder_filter(WindowId::Main, Some("f1".to_string()), cx);
        });

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            assert_eq!(ws.data().main_window.folder_filter.as_deref(), Some("f1"));
            assert_eq!(ws.active_folder_filter(WindowId::Main).map(|s| s.as_str()), Some("f1"));
            assert_eq!(ws.data_version(), 1);
        });
    }

    #[gpui::test]
    fn set_folder_filter_main_clears_with_none(cx: &mut gpui::TestAppContext) {
        // Passing None must clear the data-layer filter. Without this,
        // callers wanting to exit folder-filter mode (e.g. ClearFocus) would
        // have no API path -- the setter would be write-only.
        let data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        let workspace = cx.new(|_cx| Workspace::new(data));

        workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.set_folder_filter(WindowId::Main, Some("f1".to_string()), cx);
            ws.set_folder_filter(WindowId::Main, None, cx);
        });

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            assert!(ws.data().main_window.folder_filter.is_none());
            assert!(ws.active_folder_filter(WindowId::Main).is_none());
        });
    }

    #[gpui::test]
    fn set_folder_filter_extra_writes_only_to_targeted_window(cx: &mut gpui::TestAppContext) {
        // Targeting an extra window writes to that extra's WindowState only.
        // The main window's filter is untouched. Defends against a regression
        // that ignores the WindowId and writes to main, or scatters the write
        // across all windows.
        let mut data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        let extra = WindowState::default();
        let extra_id = extra.id;
        data.extra_windows.push(extra);
        let workspace = cx.new(|_cx| Workspace::new(data));

        workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.set_folder_filter(WindowId::Extra(extra_id), Some("f1".to_string()), cx);
        });

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            let extra_w = ws.data().window(WindowId::Extra(extra_id)).unwrap();
            assert_eq!(extra_w.folder_filter.as_deref(), Some("f1"));
            assert!(ws.data().main_window.folder_filter.is_none());
            assert!(ws.active_folder_filter(WindowId::Main).is_none());
        });
    }

    #[gpui::test]
    fn set_folder_filter_unknown_extra_is_silent_noop(cx: &mut gpui::TestAppContext) {
        // The "targeted window was just closed" race: the entity setter
        // delegates to data.set_folder_filter, which silently no-ops on a
        // missing extra id. Pin the contract so a future refactor that swaps
        // the data layer to a panicking variant fails here loudly.
        let data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        let workspace = cx.new(|_cx| Workspace::new(data));

        let unknown = uuid::Uuid::new_v4();
        workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.set_folder_filter(WindowId::Extra(unknown), Some("f1".to_string()), cx);
        });

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            assert!(ws.data().main_window.folder_filter.is_none());
            assert!(ws.active_folder_filter(WindowId::Main).is_none());
            assert!(ws.data().extra_windows.is_empty());
        });
    }

    #[gpui::test]
    fn toggle_hidden_main_inserts_when_absent(cx: &mut gpui::TestAppContext) {
        // Window-scoped entity setter: WindowId::Main + previously-visible
        // project lands the project's id in main_window.hidden_project_ids.
        // data_version bumps because hidden state is persisted -- the
        // auto-save observer must trigger.
        let data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        let workspace = cx.new(|_cx| Workspace::new(data));

        workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.toggle_hidden(WindowId::Main, "p1", cx);
        });

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            assert!(ws.data().main_window.hidden_project_ids.contains("p1"));
            assert_eq!(ws.data_version(), 1);
        });
    }

    #[gpui::test]
    fn toggle_hidden_main_removes_when_present(cx: &mut gpui::TestAppContext) {
        // The "Show Project" leg: a previously-hidden project becomes visible
        // again after toggling. Pinned separately from the insert leg because
        // a future refactor that always-inserts would leave projects stuck
        // hidden after the user clicks "Show Project".
        let mut data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        data.main_window.hidden_project_ids.insert("p1".to_string());
        let workspace = cx.new(|_cx| Workspace::new(data));

        workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.toggle_hidden(WindowId::Main, "p1", cx);
        });

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            assert!(!ws.data().main_window.hidden_project_ids.contains("p1"));
            assert_eq!(ws.data_version(), 1);
        });
    }

    #[gpui::test]
    fn toggle_hidden_extra_writes_only_to_targeted_window(cx: &mut gpui::TestAppContext) {
        // Targeting an extra window writes to that extra's WindowState only.
        // Main and the sibling extra are untouched. Defends against a
        // regression that ignores the WindowId, scatters the write across
        // every window, or always writes to main.
        let mut data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        let extra_a = WindowState::default();
        let extra_a_id = extra_a.id;
        let extra_b = WindowState::default();
        let extra_b_id = extra_b.id;
        data.extra_windows.push(extra_a);
        data.extra_windows.push(extra_b);
        let workspace = cx.new(|_cx| Workspace::new(data));

        workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.toggle_hidden(WindowId::Extra(extra_a_id), "p1", cx);
        });

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            let a = ws.data().window(WindowId::Extra(extra_a_id)).unwrap();
            let b = ws.data().window(WindowId::Extra(extra_b_id)).unwrap();
            assert!(a.hidden_project_ids.contains("p1"));
            assert!(!b.hidden_project_ids.contains("p1"));
            assert!(!ws.data().main_window.hidden_project_ids.contains("p1"));
        });
    }

    #[gpui::test]
    fn toggle_hidden_unknown_extra_is_silent_noop(cx: &mut gpui::TestAppContext) {
        // The "targeted window was just closed" race: the entity setter
        // delegates to data.toggle_hidden, which silently no-ops on a
        // missing extra id. Pin the contract so a future refactor that swaps
        // the data layer to a panicking variant fails here loudly.
        let mut data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        let extra = WindowState::default();
        let extra_id = extra.id;
        data.extra_windows.push(extra);
        let workspace = cx.new(|_cx| Workspace::new(data));

        let unknown = uuid::Uuid::new_v4();
        workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.toggle_hidden(WindowId::Extra(unknown), "p1", cx);
        });

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            assert!(ws.data().main_window.hidden_project_ids.is_empty());
            let kept = ws.data().window(WindowId::Extra(extra_id)).unwrap();
            assert!(kept.hidden_project_ids.is_empty());
        });
    }

    #[gpui::test]
    fn set_project_width_main_writes_to_data(cx: &mut gpui::TestAppContext) {
        // Window-scoped entity setter: WindowId::Main writes the
        // (project_id, width) pair into data.main_window.project_widths
        // (the persisted source of truth). data_version bumps because
        // project widths are persisted -- the auto-save observer must
        // trigger.
        let data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        let workspace = cx.new(|_cx| Workspace::new(data));

        workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.set_project_width(WindowId::Main, "p1", 0.42, cx);
        });

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            assert_eq!(ws.data().main_window.project_widths.get("p1").copied(), Some(0.42));
            assert_eq!(ws.data_version(), 1);
        });
    }

    #[gpui::test]
    fn toggle_project_layout_mode_flips_and_persists(cx: &mut gpui::TestAppContext) {
        // Per-window orientation defaults to Columns, flips to Rows on the
        // first toggle and back on the second. Each flip bumps data_version
        // so the auto-save observer persists the new orientation.
        use crate::state::ProjectLayoutMode;
        let data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        let workspace = cx.new(|_cx| Workspace::new(data));

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            assert_eq!(ws.project_layout_mode(WindowId::Main), ProjectLayoutMode::Columns);
        });

        workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.toggle_project_layout_mode(WindowId::Main, cx);
        });
        workspace.read_with(cx, |ws: &Workspace, _cx| {
            assert_eq!(ws.project_layout_mode(WindowId::Main), ProjectLayoutMode::Rows);
            assert_eq!(ws.data_version(), 1);
        });

        workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.toggle_project_layout_mode(WindowId::Main, cx);
        });
        workspace.read_with(cx, |ws: &Workspace, _cx| {
            assert_eq!(ws.project_layout_mode(WindowId::Main), ProjectLayoutMode::Columns);
            assert_eq!(ws.data_version(), 2);
        });
    }

    #[gpui::test]
    fn toggle_project_layout_mode_transposes_visible_project_splits(cx: &mut gpui::TestAppContext) {
        // Flipping the grid orientation must also deeply transpose every split
        // inside the window's visible projects: horizontal <-> vertical, nested
        // and inside tab groups, while leaving sizes and tab structure intact.
        use crate::state::SplitDirection;

        let nested = LayoutNode::Split {
            direction: SplitDirection::Horizontal,
            sizes: vec![0.6, 0.4],
            children: vec![
                LayoutNode::new_terminal(),
                LayoutNode::Split {
                    direction: SplitDirection::Vertical,
                    sizes: vec![0.5, 0.5],
                    children: vec![LayoutNode::new_terminal(), LayoutNode::new_terminal()],
                },
            ],
        };
        let mut p1 = make_project("p1");
        p1.layout = Some(nested);

        let data = make_workspace_data(vec![p1], vec!["p1"]);
        let workspace = cx.new(|_cx| Workspace::new(data));

        workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.toggle_project_layout_mode(WindowId::Main, cx);
        });

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            let layout = ws.project("p1").unwrap().layout.as_ref().unwrap();
            let LayoutNode::Split { direction, sizes, children } = layout else {
                panic!("expected outer split");
            };
            assert_eq!(*direction, SplitDirection::Vertical, "outer flipped");
            assert_eq!(sizes, &vec![0.6, 0.4], "sizes preserved");
            let LayoutNode::Split { direction: inner, .. } = &children[1] else {
                panic!("expected nested split");
            };
            assert_eq!(*inner, SplitDirection::Horizontal, "nested flipped");
        });
    }

    #[gpui::test]
    fn toggle_project_layout_mode_leaves_hidden_projects_untouched(cx: &mut gpui::TestAppContext) {
        // A project hidden in this window is not part of its grid, so toggling
        // the window orientation must not transpose that project's panes (it
        // may be shown — at its own orientation — in another window).
        use crate::state::SplitDirection;

        let mut hidden = make_project("hidden");
        hidden.layout = Some(LayoutNode::Split {
            direction: SplitDirection::Horizontal,
            sizes: vec![0.5, 0.5],
            children: vec![LayoutNode::new_terminal(), LayoutNode::new_terminal()],
        });
        let mut data = make_workspace_data(vec![make_project("p1"), hidden], vec!["p1", "hidden"]);
        data.main_window.hidden_project_ids.insert("hidden".to_string());
        let workspace = cx.new(|_cx| Workspace::new(data));

        workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.toggle_project_layout_mode(WindowId::Main, cx);
        });

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            let layout = ws.project("hidden").unwrap().layout.as_ref().unwrap();
            let LayoutNode::Split { direction, .. } = layout else {
                panic!("expected split");
            };
            assert_eq!(*direction, SplitDirection::Horizontal, "hidden project untouched");
        });
    }

    #[gpui::test]
    fn set_project_width_main_overwrites_existing_value(cx: &mut gpui::TestAppContext) {
        // Re-setting a width for the same project must replace the prior
        // value, not silently keep the first write. Without this, every
        // column-resize after the first would be a silent no-op (the user
        // would see the column "snap back" once they tried to resize the
        // same column twice). Pinned via two consecutive sets.
        let data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        let workspace = cx.new(|_cx| Workspace::new(data));

        workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.set_project_width(WindowId::Main, "p1", 0.25, cx);
            ws.set_project_width(WindowId::Main, "p1", 0.75, cx);
        });

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            assert_eq!(ws.data().main_window.project_widths.get("p1").copied(), Some(0.75));
            assert_eq!(ws.data_version(), 2);
        });
    }

    #[gpui::test]
    fn set_project_width_extra_writes_only_to_targeted_window(cx: &mut gpui::TestAppContext) {
        // Targeting an extra window writes to that extra's WindowState only.
        // Main and the sibling extra are untouched. Defends against a
        // regression that ignores the WindowId, scatters the write across
        // every window, or always writes to main.
        let mut data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        let extra_a = WindowState::default();
        let extra_a_id = extra_a.id;
        let extra_b = WindowState::default();
        let extra_b_id = extra_b.id;
        data.extra_windows.push(extra_a);
        data.extra_windows.push(extra_b);
        let workspace = cx.new(|_cx| Workspace::new(data));

        workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.set_project_width(WindowId::Extra(extra_a_id), "p1", 0.42, cx);
        });

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            let a = ws.data().window(WindowId::Extra(extra_a_id)).unwrap();
            let b = ws.data().window(WindowId::Extra(extra_b_id)).unwrap();
            assert_eq!(a.project_widths.get("p1").copied(), Some(0.42));
            assert!(b.project_widths.is_empty());
            assert!(ws.data().main_window.project_widths.is_empty());
        });
    }

    #[gpui::test]
    fn set_project_width_unknown_extra_is_silent_noop(cx: &mut gpui::TestAppContext) {
        // The "targeted window was just closed" race: the entity setter
        // delegates to data.set_project_width, which silently no-ops on a
        // missing extra id. Pin the contract so a future refactor that swaps
        // the data layer to a panicking variant fails here loudly.
        let mut data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        let extra = WindowState::default();
        let extra_id = extra.id;
        data.extra_windows.push(extra);
        let workspace = cx.new(|_cx| Workspace::new(data));

        let unknown = uuid::Uuid::new_v4();
        workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.set_project_width(WindowId::Extra(unknown), "p1", 0.42, cx);
        });

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            assert!(ws.data().main_window.project_widths.is_empty());
            let kept = ws.data().window(WindowId::Extra(extra_id)).unwrap();
            assert!(kept.project_widths.is_empty());
        });
    }

    #[gpui::test]
    fn set_folder_collapsed_main_inserts_when_true(cx: &mut gpui::TestAppContext) {
        // Window-scoped entity setter: WindowId::Main + collapsed=true inserts
        // (folder_id, true) into data.main_window.folder_collapsed (the
        // persisted source of truth). data_version bumps because
        // folder-collapsed state is persisted -- the auto-save observer must
        // trigger.
        let data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        let workspace = cx.new(|_cx| Workspace::new(data));

        workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.set_folder_collapsed(WindowId::Main, "f1", true, cx);
        });

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            assert_eq!(ws.data().main_window.folder_collapsed.get("f1"), Some(&true));
            assert_eq!(ws.data_version(), 1);
        });
    }

    #[gpui::test]
    fn set_folder_collapsed_main_removes_when_false(cx: &mut gpui::TestAppContext) {
        // The "absence == expanded" runtime convention: collapsed=false on a
        // previously-collapsed folder removes the entry, NOT inserts
        // Some(false). Defends against a regression that uses unconditional
        // insert (which would leave Some(false) tombstones bloating the on-
        // disk shape over time).
        let mut data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        data.main_window.folder_collapsed.insert("f1".to_string(), true);
        let workspace = cx.new(|_cx| Workspace::new(data));

        workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.set_folder_collapsed(WindowId::Main, "f1", false, cx);
        });

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            assert!(!ws.data().main_window.folder_collapsed.contains_key("f1"));
            assert_eq!(ws.data_version(), 1);
        });
    }

    #[gpui::test]
    fn set_folder_collapsed_extra_writes_only_to_targeted_window(cx: &mut gpui::TestAppContext) {
        // Targeting an extra window writes to that extra's WindowState only.
        // Main and the sibling extra are untouched. Defends against a
        // regression that ignores the WindowId, scatters the write across
        // every window, or always writes to main.
        let mut data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        let extra_a = WindowState::default();
        let extra_a_id = extra_a.id;
        let extra_b = WindowState::default();
        let extra_b_id = extra_b.id;
        data.extra_windows.push(extra_a);
        data.extra_windows.push(extra_b);
        let workspace = cx.new(|_cx| Workspace::new(data));

        workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.set_folder_collapsed(WindowId::Extra(extra_a_id), "f1", true, cx);
        });

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            let a = ws.data().window(WindowId::Extra(extra_a_id)).unwrap();
            let b = ws.data().window(WindowId::Extra(extra_b_id)).unwrap();
            assert_eq!(a.folder_collapsed.get("f1"), Some(&true));
            assert!(b.folder_collapsed.is_empty());
            assert!(ws.data().main_window.folder_collapsed.is_empty());
        });
    }

    #[gpui::test]
    fn set_folder_collapsed_unknown_extra_is_silent_noop(cx: &mut gpui::TestAppContext) {
        // The "targeted window was just closed" race: the entity setter
        // delegates to data.set_folder_collapsed, which silently no-ops on a
        // missing extra id. Pin the contract so a future refactor that swaps
        // the data layer to a panicking variant fails here loudly.
        let mut data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        let extra = WindowState::default();
        let extra_id = extra.id;
        data.extra_windows.push(extra);
        let workspace = cx.new(|_cx| Workspace::new(data));

        let unknown = uuid::Uuid::new_v4();
        workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.set_folder_collapsed(WindowId::Extra(unknown), "f1", true, cx);
        });

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            assert!(ws.data().main_window.folder_collapsed.is_empty());
            let kept = ws.data().window(WindowId::Extra(extra_id)).unwrap();
            assert!(kept.folder_collapsed.is_empty());
        });
    }

    #[gpui::test]
    fn set_os_bounds_main_writes_to_data(cx: &mut gpui::TestAppContext) {
        // Window-scoped entity setter: WindowId::Main + Some(bounds) writes
        // to data.main_window.os_bounds (the persisted source of truth).
        // data_version bumps because os_bounds is persisted -- the auto-save
        // observer must trigger.
        let data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        let workspace = cx.new(|_cx| Workspace::new(data));

        let bounds = WindowBounds {
            origin_x: 100.0,
            origin_y: 50.0,
            width: 1280.0,
            height: 800.0,
        };
        workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.set_os_bounds(WindowId::Main, Some(bounds), cx);
        });

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            assert_eq!(ws.data().main_window.os_bounds, Some(bounds));
            assert_eq!(ws.data_version(), 1);
        });
    }

    #[gpui::test]
    fn set_os_bounds_main_clears_with_none(cx: &mut gpui::TestAppContext) {
        // Passing None must clear the bounds. Without this leg, callers
        // wanting to forget a window's last position would have no API path
        // through the entity. Pinned at the entity layer because the
        // asymmetric set/clear contract is part of the integration surface
        // runtime code touches; data_version bumps even on the clear.
        let mut data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        data.main_window.os_bounds = Some(WindowBounds {
            origin_x: 0.0,
            origin_y: 0.0,
            width: 800.0,
            height: 600.0,
        });
        let workspace = cx.new(|_cx| Workspace::new(data));

        workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.set_os_bounds(WindowId::Main, None, cx);
        });

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            assert!(ws.data().main_window.os_bounds.is_none());
            assert_eq!(ws.data_version(), 1);
        });
    }

    #[gpui::test]
    fn set_os_bounds_extra_writes_only_to_targeted_window(cx: &mut gpui::TestAppContext) {
        // Targeting an extra window writes to that extra's WindowState only.
        // Main and the sibling extra are untouched. Defends against a
        // regression that ignores the WindowId, scatters the write across
        // every window, or always writes to main.
        let mut data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        let extra_a = WindowState::default();
        let extra_a_id = extra_a.id;
        let extra_b = WindowState::default();
        let extra_b_id = extra_b.id;
        data.extra_windows.push(extra_a);
        data.extra_windows.push(extra_b);
        let workspace = cx.new(|_cx| Workspace::new(data));

        let bounds = WindowBounds {
            origin_x: 200.0,
            origin_y: 150.0,
            width: 1024.0,
            height: 768.0,
        };
        workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.set_os_bounds(WindowId::Extra(extra_a_id), Some(bounds), cx);
        });

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            let a = ws.data().window(WindowId::Extra(extra_a_id)).unwrap();
            let b = ws.data().window(WindowId::Extra(extra_b_id)).unwrap();
            assert_eq!(a.os_bounds, Some(bounds));
            assert!(b.os_bounds.is_none());
            assert!(ws.data().main_window.os_bounds.is_none());
        });
    }

    #[gpui::test]
    fn set_os_bounds_unknown_extra_is_silent_noop(cx: &mut gpui::TestAppContext) {
        // The "targeted window was just closed" race: the entity setter
        // delegates to data.set_os_bounds, which silently no-ops on a
        // missing extra id. Pin the contract so a future refactor that swaps
        // the data layer to a panicking variant fails here loudly.
        let mut data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        let extra = WindowState::default();
        let extra_id = extra.id;
        data.extra_windows.push(extra);
        let workspace = cx.new(|_cx| Workspace::new(data));

        let unknown = uuid::Uuid::new_v4();
        let bounds = WindowBounds {
            origin_x: 1.0,
            origin_y: 2.0,
            width: 3.0,
            height: 4.0,
        };
        workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.set_os_bounds(WindowId::Extra(unknown), Some(bounds), cx);
        });

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            assert!(ws.data().main_window.os_bounds.is_none());
            let kept = ws.data().window(WindowId::Extra(extra_id)).unwrap();
            assert!(kept.os_bounds.is_none());
        });
    }

    #[gpui::test]
    fn spawn_extra_window_pushes_entry_and_bumps_version(cx: &mut gpui::TestAppContext) {
        // Wrapper contract: a single call pushes exactly one entry onto
        // `extra_windows`, returns a `WindowId::Extra(uuid)` whose uuid
        // matches the pushed entry's `state.id`, and bumps `data_version`
        // by one so the auto-save observer triggers. Pinned at the entity
        // layer because both halves -- the data-layer push and the version
        // bump -- are part of the spawn contract the upcoming `NewWindow`
        // action handler relies on.
        let data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        let workspace = cx.new(|_cx| Workspace::new(data));

        let returned = workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.spawn_extra_window(None, cx)
        });

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            assert_eq!(ws.data().extra_windows.len(), 1);
            let pushed = &ws.data().extra_windows[0];
            assert_eq!(returned, WindowId::Extra(pushed.id));
            assert_eq!(ws.data_version(), 1);
        });
    }

    #[gpui::test]
    fn spawn_extra_window_snapshot_hides_every_current_project(cx: &mut gpui::TestAppContext) {
        // Wrapper-boundary regression defense: a future refactor that
        // re-implemented the wrapper inline (instead of delegating to
        // `data.spawn_extra_window`) could drop the snapshot semantic and
        // produce a window whose grid renders every project on first
        // open -- defeating PRD line 26 ("a new window to start empty"). Pin
        // the snapshot contract at the entity layer too so a stale wrapper
        // surfaces here, not just in the data-layer test.
        let data = make_workspace_data(
            vec![make_project("p1"), make_project("p2"), make_project("p3")],
            vec!["p1", "p2", "p3"],
        );
        let workspace = cx.new(|_cx| Workspace::new(data));

        let id = workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.spawn_extra_window(None, cx)
        });

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            let spawned = ws.data().window(id).unwrap();
            assert!(spawned.hidden_project_ids.contains("p1"));
            assert!(spawned.hidden_project_ids.contains("p2"));
            assert!(spawned.hidden_project_ids.contains("p3"));
            assert_eq!(spawned.hidden_project_ids.len(), 3);
        });
    }

    #[gpui::test]
    fn spawn_extra_window_two_calls_produce_distinct_extras_and_two_version_bumps(
        cx: &mut gpui::TestAppContext,
    ) {
        // Per-call distinct ids + per-call data_version bumps. Pins the
        // "Cmd+Shift+N twice opens two windows" contract at the entity
        // layer: defends against (a) a hypothetical wrapper that coalesces
        // duplicate spawns by hidden-set contents (two windows that both
        // start fully hidden are still two distinct windows), and (b) a
        // wrapper that lazily defers the version bump (which would let the
        // auto-save observer miss the second spawn until something else
        // mutated the data).
        let data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        let workspace = cx.new(|_cx| Workspace::new(data));

        let (first, second) = workspace.update(cx, |ws: &mut Workspace, cx| {
            let a = ws.spawn_extra_window(None, cx);
            let b = ws.spawn_extra_window(None, cx);
            (a, b)
        });

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            assert_ne!(first, second);
            assert_eq!(ws.data().extra_windows.len(), 2);
            assert!(ws.data().window(first).is_some());
            assert!(ws.data().window(second).is_some());
            assert_eq!(ws.data_version(), 2);
        });
    }

    #[gpui::test]
    fn spawn_extra_window_threads_spawning_bounds_into_cascade_offset(
        cx: &mut gpui::TestAppContext,
    ) {
        // Wrapper threads `spawning_bounds: Option<WindowBounds>` into the
        // data layer, which seeds os_bounds with the +30,+30 cascade. This
        // test pins the entity-layer threading -- a future refactor that
        // dropped the parameter (e.g. went back to the no-args wrapper)
        // would surface here as a missing os_bounds on the spawned entry,
        // independent of the data-layer's `spawn_extra_window_with_
        // spawning_bounds_cascades_origin_by_30_30_preserves_size` test.
        let data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        let workspace = cx.new(|_cx| Workspace::new(data));
        let spawning = WindowBounds {
            origin_x: 50.0,
            origin_y: 75.0,
            width: 1024.0,
            height: 768.0,
        };

        let id = workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.spawn_extra_window(Some(spawning), cx)
        });

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            let spawned = ws.data().window(id).unwrap();
            let bounds = spawned.os_bounds.expect("cascade-offset os_bounds");
            assert_eq!(bounds.origin_x, 80.0);
            assert_eq!(bounds.origin_y, 105.0);
            assert_eq!(bounds.width, 1024.0);
            assert_eq!(bounds.height, 768.0);
        });
    }

    #[gpui::test]
    fn close_extra_window_drops_targeted_entry_and_bumps_version(cx: &mut gpui::TestAppContext) {
        // Slice 07 cri 3: the entity wrapper for close-extra delegates to
        // `data.close_extra_window` and bumps `data_version` so the auto-
        // save observer captures the shrunk `extra_windows` Vec. Without
        // the version bump, a closed extra would reappear on the next
        // launch (cri 6 would silently regress). Pin both halves: the
        // targeted entry is gone AND the version moved.
        let data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        let workspace = cx.new(|_cx| Workspace::new(data));

        let (id_a, id_b) = workspace.update(cx, |ws: &mut Workspace, cx| {
            let a = ws.spawn_extra_window(None, cx);
            let b = ws.spawn_extra_window(None, cx);
            (a, b)
        });
        let after_spawn_version = workspace.read_with(cx, |ws: &Workspace, _cx| ws.data_version());

        workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.close_extra_window(id_a, cx);
        });

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            assert!(ws.data().window(id_a).is_none(), "closed entry is gone");
            assert!(ws.data().window(id_b).is_some(), "sibling survives");
            assert_eq!(ws.data().extra_windows.len(), 1);
            assert_eq!(
                ws.data_version(),
                after_spawn_version + 1,
                "version bumps so auto-save fires"
            );
        });
    }

    #[gpui::test]
    fn close_extra_window_main_does_not_remove_main_state(cx: &mut gpui::TestAppContext) {
        // PRD line 53: main is the always-present slot; closing main quits
        // the app via `LastWindowClosed`, it does not delete persisted
        // main state. Targeting `WindowId::Main` at the wrapper must
        // leave main_window's per-window state intact even if a future
        // caller routes a close event through here unconditionally.
        let mut data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        data.main_window.hidden_project_ids.insert("p1".to_string());
        let workspace = cx.new(|_cx| Workspace::new(data));

        workspace.update(cx, |ws: &mut Workspace, cx| {
            ws.close_extra_window(WindowId::Main, cx);
        });

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            assert!(ws.data().main_window.hidden_project_ids.contains("p1"));
        });
    }

    #[gpui::test]
    fn active_folder_filter_main_reads_main_windows_folder_filter(cx: &mut gpui::TestAppContext) {
        // Source-of-truth contract: targeting `WindowId::Main` reads from
        // `data.main_window.folder_filter` (the persisted, per-window model).
        // This fixture writes the filter directly to main_window via a
        // WorkspaceData mutation -- never through the entity setter -- and
        // asserts the getter surfaces it. Defends against a regression that
        // re-introduces a transient cache field on the entity.
        let mut data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        data.main_window.folder_filter = Some("f1".to_string());
        let workspace = cx.new(|_cx| Workspace::new(data));

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            assert_eq!(ws.active_folder_filter(WindowId::Main).map(|s| s.as_str()), Some("f1"));
        });
    }

    #[gpui::test]
    fn active_folder_filter_extra_reads_targeted_extras_folder_filter(cx: &mut gpui::TestAppContext) {
        // Per-window viewport model: targeting `WindowId::Extra(uuid)` reads
        // from that extra's `WindowState::folder_filter` (NOT main's). The
        // fixture pre-populates main + a sibling extra with their own
        // distinct filters so a regression that ignores window_id and
        // unconditionally returns main's filter, scatters across extras,
        // or routes through the wrong slot would surface here.
        let mut data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        data.main_window.folder_filter = Some("main_folder".to_string());
        let extra_a = WindowState {
            folder_filter: Some("extra_a_folder".to_string()),
            ..Default::default()
        };
        let extra_a_id = extra_a.id;
        let extra_b = WindowState {
            folder_filter: Some("extra_b_folder".to_string()),
            ..Default::default()
        };
        let extra_b_id = extra_b.id;
        data.extra_windows = vec![extra_a, extra_b];
        let workspace = cx.new(|_cx| Workspace::new(data));

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            assert_eq!(
                ws.active_folder_filter(WindowId::Extra(extra_a_id)).map(|s| s.as_str()),
                Some("extra_a_folder"),
            );
            assert_eq!(
                ws.active_folder_filter(WindowId::Extra(extra_b_id)).map(|s| s.as_str()),
                Some("extra_b_folder"),
            );
            // Main is unchanged by the extras' reads.
            assert_eq!(
                ws.active_folder_filter(WindowId::Main).map(|s| s.as_str()),
                Some("main_folder"),
            );
        });
    }

    #[gpui::test]
    fn active_folder_filter_unknown_extra_returns_none(cx: &mut gpui::TestAppContext) {
        // Close-race contract: a fresh uuid that does not match any extra
        // returns `None` (no panic, no fallback to main's filter). Pre-
        // populate main with a filter to ensure the unknown-extra path does
        // NOT silently surface main's value as a default. Mirrors the
        // silent-no-op shape of the window-scoped setters.
        let mut data = make_workspace_data(vec![make_project("p1")], vec!["p1"]);
        data.main_window.folder_filter = Some("main_folder".to_string());
        let workspace = cx.new(|_cx| Workspace::new(data));
        let unknown = uuid::Uuid::new_v4();

        workspace.read_with(cx, |ws: &Workspace, _cx| {
            assert!(ws.active_folder_filter(WindowId::Extra(unknown)).is_none());
            // Main's filter is still readable via its own id.
            assert_eq!(
                ws.active_folder_filter(WindowId::Main).map(|s| s.as_str()),
                Some("main_folder"),
            );
        });
    }
}
