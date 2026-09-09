//! Layout Tree Engine.
//!
//! Provides an abstract, generic layout node tree for IDE workbench layouts.
//! Unlike `velowork-layout::LayoutNode` (which is terminal-specific with
//! shell type, zoom level, etc.), this module defines **view-agnostic** layout
//! primitives suitable for any IDE pane — sidebar, editor, terminal, SFTP
//! panel, or future panel types.
//!
//! This is the building block for the Phase 1 infrastructure: supporting
//! arbitrary split arrangements, tab groups, and workspace serialization.

use serde::{Deserialize, Serialize};

/// Opaque view identifier. Any panel, editor, or terminal can be addressed
/// by a string like `"sidebar.file_tree"` or `"terminal.1"`.
pub type ViewId = String;

/// Direction of a split.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitAxis {
    /// Left–right (horizontal divider).
    Horizontal,
    /// Top–bottom (vertical divider).
    Vertical,
}

/// A single node in the layout tree.
///
/// The tree is recursive: `Split` nodes contain two children, `Tabs` nodes
/// contain multiple tab IDs, and `Leaf` nodes are the terminal views.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkbenchNode {
    /// Leaf: a concrete view panel.
    Leaf {
        id: ViewId,
    },
    /// Tab group: multiple views sharing one pane area.
    Tabs {
        active_index: usize,
        tabs: Vec<ViewId>,
    },
    /// Binary split.
    Split {
        axis: SplitAxis,
        /// Position of the divider, 0.0 ..= 1.0 (fraction of available space
        /// allocated to the first child).
        ratio: f32,
        first: Box<WorkbenchNode>,
        second: Box<WorkbenchNode>,
    },
}

impl WorkbenchNode {
    /// Collect all `ViewId`s contained in this subtree.
    pub fn view_ids(&self) -> Vec<&ViewId> {
        match self {
            WorkbenchNode::Leaf { id } => vec![id],
            WorkbenchNode::Tabs { tabs, .. } => tabs.iter().collect(),
            WorkbenchNode::Split { first, second, .. } => {
                let mut ids = first.view_ids();
                ids.extend(second.view_ids());
                ids
            }
        }
    }

    /// Returns `true` if any node in the subtree carries the given view id.
    pub fn contains_view(&self, view_id: &str) -> bool {
        match self {
            WorkbenchNode::Leaf { id } => id == view_id,
            WorkbenchNode::Tabs { tabs, .. } => tabs.iter().any(|t| t == view_id),
            WorkbenchNode::Split { first, second, .. } => {
                first.contains_view(view_id) || second.contains_view(view_id)
            }
        }
    }
}

/// High-level workbench layout engine.
///
/// Wraps a root `WorkbenchNode` and provides factory methods for common
/// IDE arrangements.
pub struct WorkbenchLayout {
    pub root: WorkbenchNode,
}

impl WorkbenchLayout {
    /// Construct the default Velowork SSH Connection Workbench layout:
    /// ```text
    /// ┌─────────────────┬──────────────────────────────┬─────────────────┐
    /// │ Session Sidebar │ Project Grid (SSH Terminals) │ Right Dock      │
    /// │ (20%)           │ (65% × 75%)                  │ (15%)           │
    /// │                 ├──────────────────────────────┤                 │
    /// │                 │ SFTP / Commands Bottom Dock  │                 │
    /// │                 │ (65% × 25%)                  │                 │
    /// └─────────────────┴──────────────────────────────┴─────────────────┘
    /// ```
    pub fn default_ssh_workspace() -> Self {
        Self {
            root: WorkbenchNode::Split {
                axis: SplitAxis::Horizontal,
                ratio: 0.20,
                first: Box::new(WorkbenchNode::Leaf {
                    id: "sidebar.session_panel".into(),
                }),
                second: Box::new(WorkbenchNode::Split {
                    axis: SplitAxis::Horizontal,
                    ratio: 0.80,
                    first: Box::new(WorkbenchNode::Split {
                        axis: SplitAxis::Vertical,
                        ratio: 0.75,
                        first: Box::new(WorkbenchNode::Tabs {
                            active_index: 0,
                            tabs: vec!["terminal.ssh.prod".into(), "terminal.ssh.staging".into()],
                        }),
                        second: Box::new(WorkbenchNode::Tabs {
                            active_index: 0,
                            tabs: vec!["bottom_dock.sftp".into(), "bottom_dock.commands".into()],
                        }),
                    }),
                    second: Box::new(WorkbenchNode::Leaf {
                        id: "right_sidebar.dock".into(),
                    }),
                }),
            },
        }
    }

    /// Legacy alias for default_ssh_workspace.
    pub fn default_ide() -> Self {
        Self::default_ssh_workspace()
    }

    /// Construct a minimal single-view layout (useful for tests or simple mode).
    pub fn single(view_id: impl Into<String>) -> Self {
        Self {
            root: WorkbenchNode::Leaf {
                id: view_id.into(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_ide_contains_expected_views() {
        let layout = WorkbenchLayout::default_ssh_workspace();
        assert!(layout.root.contains_view("sidebar.session_panel"));
        assert!(layout.root.contains_view("terminal.ssh.prod"));
        assert!(layout.root.contains_view("terminal.ssh.staging"));
        assert!(layout.root.contains_view("bottom_dock.sftp"));
        assert!(layout.root.contains_view("bottom_dock.commands"));
        assert!(layout.root.contains_view("right_sidebar.dock"));
        assert!(!layout.root.contains_view("nonexistent"));
    }

    #[test]
    fn test_view_ids_count() {
        let layout = WorkbenchLayout::default_ssh_workspace();
        let ids = layout.root.view_ids();
        assert_eq!(ids.len(), 6);
    }

    #[test]
    fn test_single_layout() {
        let layout = WorkbenchLayout::single("my_view");
        assert!(layout.root.contains_view("my_view"));
        assert_eq!(layout.root.view_ids().len(), 1);
    }

    #[test]
    fn test_serialization_roundtrip() {
        let layout = WorkbenchLayout::default_ide();
        let json = serde_json::to_string_pretty(&layout.root)
            .expect("serialize");
        let restored: WorkbenchNode = serde_json::from_str(&json)
            .expect("deserialize");
        assert_eq!(layout.root, restored);
    }
}
