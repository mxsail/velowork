//! Tab group operations: add, set-active, reorder.

use crate::focus::FocusManager;
use crate::state::{LayoutNode, Workspace};
use gpui::*;

impl Workspace {
    /// Add a new tab - either to existing tab group (if parent is Tabs) or create new tab group
    pub fn add_tab(
        &mut self,
        focus_manager: &mut FocusManager,
        project_id: &str,
        path: &[usize],
        cx: &mut Context<Self>,
    ) {
        self.add_tab_with_shell(focus_manager, project_id, path, velowork_core::shell::ShellType::Default, cx);
    }

    /// Add a new tab with a specific ShellType atomically
    pub fn add_tab_with_shell(
        &mut self,
        focus_manager: &mut FocusManager,
        project_id: &str,
        path: &[usize],
        shell_type: velowork_core::shell::ShellType,
        cx: &mut Context<Self>,
    ) {
        log::debug!("[workspace:layout] add_tab_with_shell | project_id={} path={:?}", project_id, path);

        // 1. Check if path itself is already a Tabs container
        if let Some(project) = self.project(project_id)
            && let Some(ref layout) = project.layout
        {
            if let Some(LayoutNode::Tabs { .. }) = layout.get_at_path(path) {
                self.add_tab_to_group_with_shell(focus_manager, project_id, path, shell_type, cx);
                return;
            }

            // 2. Check if any ancestor of path is a Tabs container
            for len in (0..path.len()).rev() {
                let ancestor_path = &path[..len];
                if let Some(LayoutNode::Tabs { .. }) = layout.get_at_path(ancestor_path) {
                    self.add_tab_to_group_with_shell(focus_manager, project_id, ancestor_path, shell_type, cx);
                    return;
                }
            }
        }

        // 3. Neither path nor ancestor is Tabs - wrap current node in a new tab group
        let new_node = LayoutNode::new_terminal_with_shell(shell_type);
        let mut created = false;
        self.with_layout_node(project_id, path, cx, |node| {
            let old_node = node.clone();
            *node = LayoutNode::Tabs {
                children: vec![old_node, new_node.clone()],
                active_tab: 1,
            };
            created = true;
            log::debug!("[workspace:layout] Created new tab group");
            true
        });

        if created {
            // Focus the new tab
            let mut new_path = path.to_vec();
            new_path.push(1);
            self.set_focused_terminal(focus_manager, project_id.to_string(), new_path, cx);
        } else {
            // If project had no layout node at all, initialize layout with Tabs
            if let Some(project) = self.project_mut(project_id) {
                if project.layout.is_none() {
                    project.layout = Some(LayoutNode::Tabs {
                        children: vec![new_node],
                        active_tab: 0,
                    });
                    self.set_focused_terminal(focus_manager, project_id.to_string(), vec![0], cx);
                    cx.notify();
                }
            }
        }
    }

    /// Add a new tab to an existing Tabs container
    pub fn add_tab_to_group(
        &mut self,
        focus_manager: &mut FocusManager,
        project_id: &str,
        tabs_path: &[usize],
        cx: &mut Context<Self>,
    ) {
        self.add_tab_to_group_with_shell(focus_manager, project_id, tabs_path, velowork_core::shell::ShellType::Default, cx);
    }

    /// Add a new tab to an existing Tabs container with a specific ShellType
    pub fn add_tab_to_group_with_shell(
        &mut self,
        focus_manager: &mut FocusManager,
        project_id: &str,
        tabs_path: &[usize],
        shell_type: velowork_core::shell::ShellType,
        cx: &mut Context<Self>,
    ) {
        // Resolve actual Tabs path (either tabs_path or its ancestor)
        let resolved_tabs_path = {
            let mut resolved = None;
            if let Some(project) = self.project(project_id)
                && let Some(ref layout) = project.layout
            {
                if let Some(LayoutNode::Tabs { .. }) = layout.get_at_path(tabs_path) {
                    resolved = Some(tabs_path.to_vec());
                } else {
                    for len in (0..tabs_path.len()).rev() {
                        let ancestor_path = &tabs_path[..len];
                        if let Some(LayoutNode::Tabs { .. }) = layout.get_at_path(ancestor_path) {
                            resolved = Some(ancestor_path.to_vec());
                            break;
                        }
                    }
                }
            }
            resolved.unwrap_or_else(|| tabs_path.to_vec())
        };

        let mut new_tab_index = 0;
        let new_node = LayoutNode::new_terminal_with_shell(shell_type);
        self.with_layout_node(project_id, &resolved_tabs_path, cx, |node| {
            if let LayoutNode::Tabs { children, active_tab } = node {
                children.push(new_node);
                *active_tab = children.len() - 1;
                new_tab_index = *active_tab;
                log::debug!("[workspace:layout] Added new tab to existing group, now {} tabs", children.len());
                true
            } else {
                false
            }
        });

        // Focus the new tab
        let mut new_path = resolved_tabs_path.clone();
        new_path.push(new_tab_index);
        self.set_focused_terminal(focus_manager, project_id.to_string(), new_path, cx);
    }

    /// Set active tab in a tabs container
    pub fn set_active_tab(
        &mut self,
        project_id: &str,
        path: &[usize],
        tab_index: usize,
        cx: &mut Context<Self>,
    ) {
        self.with_layout_node(project_id, path, cx, |node| {
            if let LayoutNode::Tabs { active_tab, .. } = node {
                *active_tab = tab_index;
                true
            } else {
                false
            }
        });
    }

    /// Move a tab from one position to another within a tabs container
    pub fn move_tab(
        &mut self,
        project_id: &str,
        path: &[usize],
        from_index: usize,
        to_index: usize,
        cx: &mut Context<Self>,
    ) {
        self.with_layout_node(project_id, path, cx, |node| {
            if let LayoutNode::Tabs { children, active_tab } = node {
                if from_index >= children.len() || to_index >= children.len() {
                    return false;
                }
                if from_index == to_index {
                    return false;
                }

                // Remove the tab from its current position
                let tab = children.remove(from_index);

                // Clamp target index to valid range after removal
                let target = to_index.min(children.len());

                // Insert at new position
                children.insert(target, tab);

                // Update active_tab index to follow the moved tab if it was active
                if *active_tab == from_index {
                    *active_tab = target;
                } else if from_index < *active_tab && target >= *active_tab {
                    // Active tab shifted left
                    *active_tab = active_tab.saturating_sub(1);
                } else if from_index > *active_tab && target <= *active_tab {
                    // Active tab shifted right
                    *active_tab = (*active_tab + 1).min(children.len().saturating_sub(1));
                }

                true
            } else {
                false
            }
        });
    }
}
