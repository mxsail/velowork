//! Terminal-specific workspace actions
//!
//! Actions for managing individual terminals within projects.

use velowork_terminal::shell_config::ShellType;
use crate::state::{LayoutNode, Workspace};
use gpui::*;

impl Workspace {
    /// Set terminal ID at a layout path
    pub fn set_terminal_id(
        &mut self,
        project_id: &str,
        path: &[usize],
        terminal_id: String,
        cx: &mut Context<Self>,
    ) {
        self.with_project(project_id, cx, |project| {
            if let Some(ref mut layout) = project.layout
                && let Some(node) = layout.get_at_path_mut(path)
                    && let LayoutNode::Terminal { terminal_id: id, .. } = node {
                        *id = Some(terminal_id);
                        return true;
                    }
            false
        });
    }

    /// Set shell type for a terminal at a layout path
    pub fn set_terminal_shell(
        &mut self,
        project_id: &str,
        path: &[usize],
        shell_type: ShellType,
        cx: &mut Context<Self>,
    ) {
        self.with_layout_node(project_id, path, cx, |node| {
            if let LayoutNode::Terminal { shell_type: st, .. } = node {
                *st = shell_type;
                return true;
            }
            false
        });
    }

    /// Replace shell type for a terminal at a layout path, resetting its terminal_id to None
    /// so a new terminal process can be spawned in its place without creating additional tabs.
    pub fn replace_terminal_shell(
        &mut self,
        project_id: &str,
        path: &[usize],
        shell_type: ShellType,
        cx: &mut Context<Self>,
    ) {
        self.with_layout_node(project_id, path, cx, |node| {
            if let LayoutNode::Terminal {
                shell_type: st,
                terminal_id: tid,
                ..
            } = node
            {
                *st = shell_type;
                *tid = None;
                return true;
            }
            false
        });
    }

    /// Get shell type for a terminal at a layout path
    pub fn get_terminal_shell(&self, project_id: &str, path: &[usize]) -> Option<ShellType> {
        let project = self.project(project_id)?;
        if let Some(LayoutNode::Terminal { shell_type, .. }) = project.layout.as_ref().and_then(|l| l.get_at_path(path)) {
            Some(shell_type.clone())
        } else {
            None
        }
    }

    /// Rename a terminal
    pub fn rename_terminal(
        &mut self,
        project_id: &str,
        terminal_id: &str,
        new_name: String,
        cx: &mut Context<Self>,
    ) {
        let terminal_id = terminal_id.to_string();
        self.with_project(project_id, cx, |project| {
            project.terminal_names.insert(terminal_id, new_name);
            true
        });
    }

    /// Set terminal hidden state
    #[allow(dead_code)] // API for future terminal visibility control
    pub fn set_terminal_hidden(
        &mut self,
        project_id: &str,
        terminal_id: &str,
        hidden: bool,
        cx: &mut Context<Self>,
    ) {
        let terminal_id = terminal_id.to_string();
        self.with_project(project_id, cx, |project| {
            project.hidden_terminals.insert(terminal_id, hidden);
            true
        });
    }

    /// Restore (un-minimize) a terminal at a path and focus it
    pub fn restore_terminal(
        &mut self,
        focus_manager: &mut crate::focus::FocusManager,
        project_id: &str,
        path: &[usize],
        cx: &mut Context<Self>,
    ) {
        let restored = self.with_layout_node(project_id, path, cx, |node| {
            if let LayoutNode::Terminal { minimized, .. } = node {
                *minimized = false;
                true
            } else {
                false
            }
        });

        if restored {
            if let Some(project) = self.project_mut(project_id)
                && let Some(ref mut layout) = project.layout {
                    layout.activate_tabs_along_path(path);
                }
            self.set_focused_terminal(focus_manager, project_id.to_string(), path.to_vec(), cx);
        }
    }

    /// Restore (un-minimize) a terminal by ID (finds path automatically)
    pub fn restore_terminal_by_id(
        &mut self,
        focus_manager: &mut crate::focus::FocusManager,
        project_id: &str,
        terminal_id: &str,
        cx: &mut Context<Self>,
    ) {
        let mut target_path = None;

        if let Some(project) = self.project_mut(project_id)
            && let Some(ref mut layout) = project.layout
                && let Some(path) = layout.find_terminal_path(terminal_id)
                    && let Some(node) = layout.get_at_path_mut(&path)
                        && let LayoutNode::Terminal { minimized, .. } = node {
                            if *minimized {
                                *minimized = false;
                                target_path = Some(path);
                            }
                        }

        let Some(path) = target_path else { return; };
        if let Some(project) = self.project_mut(project_id)
            && let Some(ref mut layout) = project.layout {
                layout.activate_tabs_along_path(&path);
            }
        self.notify_data(cx);
        self.set_focused_terminal(focus_manager, project_id.to_string(), path, cx);
    }

    /// Toggle terminal minimized state by terminal ID (finds path automatically)
    /// and manages focus transition:
    /// - When minimizing: shifts focus to the nearest visible terminal (sibling/project/workspace) or clears focus.
    /// - When restoring: unminimizes and sets focus to the restored terminal.
    pub fn toggle_terminal_minimized_by_id(
        &mut self,
        focus_manager: &mut crate::focus::FocusManager,
        project_id: &str,
        terminal_id: &str,
        cx: &mut Context<Self>,
    ) {
        let mut target_path = None;
        let mut now_minimized = false;

        if let Some(project) = self.project_mut(project_id)
            && let Some(ref mut layout) = project.layout
                && let Some(path) = layout.find_terminal_path(terminal_id)
                    && let Some(node) = layout.get_at_path_mut(&path)
                        && let LayoutNode::Terminal { minimized, .. } = node {
                            *minimized = !*minimized;
                            now_minimized = *minimized;
                            target_path = Some(path);
                            self.notify_data(cx);
                        }

        let Some(path) = target_path else { return; };

        if now_minimized {
            // The terminal was just minimized.
            // If this terminal was the currently focused terminal, find the next visible one.
            let was_focused = focus_manager.focused_terminal_state().is_some_and(|f| {
                f.project_id == project_id && f.layout_path == path
            });

            if was_focused {
                // 1. Try to find visible sibling / visible terminal in the current project
                let next_path_in_project = self
                    .project(project_id)
                    .and_then(|p| p.layout.as_ref())
                    .and_then(|layout| layout.find_sibling_or_visible_terminal_path(&path));

                if let Some(next_path) = next_path_in_project {
                    // Activate any tabs along the path
                    if let Some(project_mut) = self.project_mut(project_id)
                        && let Some(ref mut layout) = project_mut.layout {
                            layout.activate_tabs_along_path(&next_path);
                        }
                    self.set_focused_terminal(focus_manager, project_id.to_string(), next_path, cx);
                } else {
                    // 2. Current project has no other visible terminals. Try other projects.
                    let mut other_project_target = None;
                    for p in self.projects() {
                        if p.id != project_id
                            && let Some(ref layout) = p.layout
                            && let Some(vpath) = layout.find_first_visible_terminal_path() {
                                other_project_target = Some((p.id.clone(), vpath));
                                break;
                            }
                    }

                    if let Some((other_pid, other_path)) = other_project_target {
                        if let Some(project_mut) = self.project_mut(&other_pid)
                            && let Some(ref mut layout) = project_mut.layout {
                                layout.activate_tabs_along_path(&other_path);
                            }
                        self.set_focused_terminal(focus_manager, other_pid, other_path, cx);
                    } else {
                        // 3. No visible terminals in any project
                        focus_manager.clear_focus();
                        cx.notify();
                    }
                }
            }
        } else {
            // The terminal was restored.
            if let Some(project) = self.project_mut(project_id)
                && let Some(ref mut layout) = project.layout {
                    layout.activate_tabs_along_path(&path);
                }
            self.set_focused_terminal(focus_manager, project_id.to_string(), path, cx);
        }
    }

    /// Check if a terminal is minimized by ID
    pub fn is_terminal_minimized(&self, project_id: &str, terminal_id: &str) -> bool {
        if let Some(project) = self.project(project_id)
            && let Some(ref layout) = project.layout
                && let Some(path) = layout.find_terminal_path(terminal_id)
                    && let Some(LayoutNode::Terminal { minimized, .. }) = layout.get_at_path(&path) {
                        return *minimized;
                    }
        false
    }

    /// Detach a terminal to a separate window.
    /// Sets the detached flag on the layout node (single source of truth).
    /// Returns true if the terminal was successfully detached.
    pub fn detach_terminal(
        &mut self,
        project_id: &str,
        path: &[usize],
        cx: &mut Context<Self>,
    ) -> bool {
        self.with_layout_node(project_id, path, cx, |node| {
            if let LayoutNode::Terminal { terminal_id: Some(_), detached, .. } = node
                && !*detached {
                    *detached = true;
                    return true;
                }
            false
        })
    }

    /// Re-attach a detached terminal back to its original location.
    /// Scans all project layouts to find the terminal and clear the detached flag.
    pub fn attach_terminal(&mut self, terminal_id: &str, cx: &mut Context<Self>) {
        for project in &mut self.data.projects {
            if let Some(ref mut layout) = project.layout
                && let Some(path) = layout.find_terminal_path(terminal_id) {
                    if let Some(node) = layout.get_at_path_mut(&path)
                        && let LayoutNode::Terminal { detached, .. } = node {
                            *detached = false;
                        }
                    self.notify_data(cx);
                    return;
                }
        }
    }

    /// Check if a terminal is detached by scanning layout trees.
    pub fn is_terminal_detached(&self, terminal_id: &str) -> bool {
        for project in &self.data.projects {
            if let Some(ref layout) = project.layout
                && let Some(path) = layout.find_terminal_path(terminal_id)
                    && let Some(LayoutNode::Terminal { detached, .. }) = layout.get_at_path(&path) {
                        return *detached;
                    }
        }
        false
    }

    /// Get the zoom level for a terminal at the given path
    pub fn get_terminal_zoom(&self, project_id: &str, path: &[usize]) -> f32 {
        self.project(project_id)
            .and_then(|p| p.layout.as_ref())
            .and_then(|l| l.get_at_path(path))
            .and_then(|node| {
                if let LayoutNode::Terminal { zoom_level, .. } = node {
                    Some(*zoom_level)
                } else {
                    None
                }
            })
            .unwrap_or(1.0)
    }

    /// Set the zoom level for a terminal at the given path
    pub fn set_terminal_zoom(
        &mut self,
        project_id: &str,
        path: &[usize],
        zoom: f32,
        cx: &mut Context<Self>,
    ) {
        let clamped = zoom.clamp(0.5, 3.0);
        self.with_layout_node(project_id, path, cx, |node| {
            if let LayoutNode::Terminal { zoom_level, .. } = node {
                *zoom_level = clamped;
                true
            } else {
                false
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::focus::FocusManager;
    use crate::state::{ProjectData, Workspace, WorkspaceData};
    use gpui::AppContext as _;
    use std::collections::HashMap;
    use velowork_layout::LayoutNode;

    fn make_test_project(id: &str) -> ProjectData {
        let t1 = LayoutNode::Terminal {
            terminal_id: Some("term-1".to_string()),
            minimized: false,
            detached: false,
            shell_type: velowork_core::shell::ShellType::Default,
            zoom_level: 1.0,
        };
        let t2 = LayoutNode::Terminal {
            terminal_id: Some("term-2".to_string()),
            minimized: false,
            detached: false,
            shell_type: velowork_core::shell::ShellType::Default,
            zoom_level: 1.0,
        };
        ProjectData {
            id: id.to_string(),
            name: format!("Project {}", id),
            path: "/tmp/test".to_string(),
            layout: Some(LayoutNode::Split {
                direction: velowork_layout::SplitDirection::Horizontal,
                sizes: vec![0.5, 0.5],
                children: vec![t1, t2],
            }),
            terminal_names: HashMap::new(),
            hidden_terminals: HashMap::new(),
            folder_color: Default::default(),
            is_remote: false,
            connection_id: None,
            service_terminals: HashMap::new(),
            default_shell: None,
            pinned: false,
            last_activity_at: None,
            ..Default::default()
        }
    }

    fn make_workspace_data() -> WorkspaceData {
        WorkspaceData {
            version: 1,
            projects: vec![],
            project_order: vec![],
            service_panel_heights: HashMap::new(),
            folders: vec![],
            main_window: crate::state::WindowState::default(),
            extra_windows: Vec::new(),
        }
    }

    #[gpui::test]
    fn test_toggle_minimized_transfers_focus_to_sibling(cx: &mut gpui::TestAppContext) {
        let mut data = make_workspace_data();
        data.projects = vec![make_test_project("p1")];
        data.project_order = vec!["p1".to_string()];
        let ws = cx.new(|_cx| Workspace::new(data));

        let mut fm = FocusManager::new();
        // Initial focus on term-1 at path [0]
        fm.focus_terminal("p1".to_string(), vec![0]);
        assert_eq!(
            fm.focused_terminal_state().map(|f| f.layout_path.clone()),
            Some(vec![0])
        );

        // Minimize term-1
        ws.update(cx, |ws: &mut Workspace, cx| {
            ws.toggle_terminal_minimized_by_id(&mut fm, "p1", "term-1", cx);
        });

        // Focus should have automatically shifted to term-2 at path [1]
        assert_eq!(
            fm.focused_terminal_state().map(|f| (f.project_id.clone(), f.layout_path.clone())),
            Some(("p1".to_string(), vec![1]))
        );

        // Restore term-1 using restore_terminal
        ws.update(cx, |ws: &mut Workspace, cx| {
            ws.restore_terminal(&mut fm, "p1", &[0], cx);
        });

        // Focus should have returned to term-1 at path [0]
        assert_eq!(
            fm.focused_terminal_state().map(|f| (f.project_id.clone(), f.layout_path.clone())),
            Some(("p1".to_string(), vec![0]))
        );

        // Minimize term-2
        ws.update(cx, |ws: &mut Workspace, cx| {
            ws.toggle_terminal_minimized_by_id(&mut fm, "p1", "term-2", cx);
        });
        assert!(cx.read(|cx| ws.read(cx).is_terminal_minimized("p1", "term-2")));

        // Restore term-2 using restore_terminal_by_id
        ws.update(cx, |ws: &mut Workspace, cx| {
            ws.restore_terminal_by_id(&mut fm, "p1", "term-2", cx);
        });
        assert!(!cx.read(|cx| ws.read(cx).is_terminal_minimized("p1", "term-2")));
        assert_eq!(
            fm.focused_terminal_state().map(|f| (f.project_id.clone(), f.layout_path.clone())),
            Some(("p1".to_string(), vec![1]))
        );
    }

    #[gpui::test]
    fn test_tabs_terminal_minimize_and_restore_by_id(cx: &mut gpui::TestAppContext) {
        let mut data = make_workspace_data();
        let mut proj = make_test_project("p1");
        let t1 = LayoutNode::Terminal {
            terminal_id: Some("term-tab-1".to_string()),
            minimized: false,
            detached: false,
            shell_type: velowork_core::shell::ShellType::Default,
            zoom_level: 1.0,
        };
        let t2 = LayoutNode::Terminal {
            terminal_id: Some("term-tab-2".to_string()),
            minimized: false,
            detached: false,
            shell_type: velowork_core::shell::ShellType::Default,
            zoom_level: 1.0,
        };
        proj.layout = Some(LayoutNode::Tabs {
            children: vec![t1, t2],
            active_tab: 0,
        });
        data.projects = vec![proj];
        data.project_order = vec!["p1".to_string()];
        let ws = cx.new(|_cx| Workspace::new(data));

        let mut fm = FocusManager::new();
        fm.focus_terminal("p1".to_string(), vec![0]);

        // Minimize term-tab-1
        ws.update(cx, |ws: &mut Workspace, cx| {
            ws.toggle_terminal_minimized_by_id(&mut fm, "p1", "term-tab-1", cx);
        });
        assert!(cx.read(|cx| ws.read(cx).is_terminal_minimized("p1", "term-tab-1")));
        // Active tab switched to 1
        assert_eq!(
            cx.read(|cx| match ws.read(cx).project("p1").unwrap().layout.as_ref().unwrap() {
                LayoutNode::Tabs { active_tab, .. } => *active_tab,
                _ => 999,
            }),
            1
        );
        assert_eq!(
            fm.focused_terminal_state().map(|f| (f.project_id.clone(), f.layout_path.clone())),
            Some(("p1".to_string(), vec![1]))
        );

        // Restore term-tab-1 using restore_terminal_by_id
        ws.update(cx, |ws: &mut Workspace, cx| {
            ws.restore_terminal_by_id(&mut fm, "p1", "term-tab-1", cx);
        });
        assert!(!cx.read(|cx| ws.read(cx).is_terminal_minimized("p1", "term-tab-1")));
        // Active tab switched back to 0
        assert_eq!(
            cx.read(|cx| match ws.read(cx).project("p1").unwrap().layout.as_ref().unwrap() {
                LayoutNode::Tabs { active_tab, .. } => *active_tab,
                _ => 999,
            }),
            0
        );
        assert_eq!(
            fm.focused_terminal_state().map(|f| (f.project_id.clone(), f.layout_path.clone())),
            Some(("p1".to_string(), vec![0]))
        );
    }

    #[gpui::test]
    fn test_single_terminal_minimize_and_restore_by_id(cx: &mut gpui::TestAppContext) {
        let mut data = make_workspace_data();
        let mut proj = make_test_project("p1");
        proj.layout = Some(LayoutNode::Terminal {
            terminal_id: Some("term-single".to_string()),
            minimized: false,
            detached: false,
            zoom_level: 1.0,
            shell_type: velowork_core::shell::ShellType::Default,
        });
        data.projects = vec![proj];
        data.project_order = vec!["p1".to_string()];
        let ws = cx.new(|_cx| Workspace::new(data));

        let mut fm = FocusManager::new();
        fm.focus_terminal("p1".to_string(), vec![]);

        // Minimize single terminal
        ws.update(cx, |ws: &mut Workspace, cx| {
            ws.toggle_terminal_minimized_by_id(&mut fm, "p1", "term-single", cx);
        });
        assert!(cx.read(|cx| ws.read(cx).is_terminal_minimized("p1", "term-single")));

        // Restore single terminal by id
        ws.update(cx, |ws: &mut Workspace, cx| {
            ws.restore_terminal_by_id(&mut fm, "p1", "term-single", cx);
        });
        assert!(!cx.read(|cx| ws.read(cx).is_terminal_minimized("p1", "term-single")));
        assert_eq!(
            fm.focused_terminal_state().map(|f| (f.project_id.clone(), f.layout_path.clone())),
            Some(("p1".to_string(), vec![]))
        );
    }
}
