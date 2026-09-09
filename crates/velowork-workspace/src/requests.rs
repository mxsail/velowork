//! UI request types for transient view-to-view communication.
//!
//! These types describe UI interactions (context menus, overlays, rename dialogs)
//! and are never persisted. They flow through `Workspace`'s request queues.

/// Project-scoped overlay request. Carries a `project_id` once;
/// the specific overlay is in `kind`.
#[derive(Clone, Debug)]
pub struct ProjectOverlay {
    pub project_id: String,
    pub kind: ProjectOverlayKind,
}

/// The specific overlay to show for a project.
#[derive(Clone, Debug)]
pub enum ProjectOverlayKind {
    ContextMenu { position: gpui::Point<gpui::Pixels> },
    ShellSelector { terminal_id: String, current_shell: velowork_terminal::shell_config::ShellType },
    TerminalContextMenu {
        terminal_id: String,
        layout_path: Vec<usize>,
        position: gpui::Point<gpui::Pixels>,
        has_selection: bool,
        link_url: Option<String>,
    },
    TerminalLogStop {
        terminal_id: String,
    },
    TabContextMenu {
        tab_index: usize,
        num_tabs: usize,
        layout_path: Vec<usize>,
        position: gpui::Point<gpui::Pixels>,
    },
    ToggleSftpPanel,
}

/// Folder-scoped overlay request. Carries a `folder_id` once;
/// the specific overlay is in `kind`.
#[derive(Clone, Debug)]
pub struct FolderOverlay {
    pub folder_id: String,
    pub kind: FolderOverlayKind,
}

/// The specific overlay to show for a folder.
#[derive(Clone, Debug)]
pub enum FolderOverlayKind {
    ContextMenu { folder_name: String, position: gpui::Point<gpui::Pixels> },
}

/// Requests consumed by WindowView::process_pending_requests().
///
/// Project-scoped and folder-scoped variants are grouped into
/// `ProjectOverlay` and `FolderOverlay` to avoid duplicating
/// `project_id` / `folder_id` across every variant. Global and
/// remote variants remain flat.
#[derive(Clone, Debug)]
pub enum OverlayRequest {
    Project(ProjectOverlay),
    Folder(FolderOverlay),
    AddProjectDialog,
    ManageProjectsDialog,
    ShowCommandPalette,
    ImportSessionsDialog,
}

/// Requests consumed by Sidebar::render()
#[derive(Clone, Debug)]
pub enum SidebarRequest {
    RenameProject { project_id: String, project_name: String },
    RenameFolder { folder_id: String, folder_name: String },
}
