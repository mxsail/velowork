use velowork_terminal::session_backend::SessionBackend;
use velowork_core::theme::FolderColor;
use crate::state::{LayoutNode, ProjectData, WindowState, WorkspaceData};

use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

/// When true, the workspace was loaded from a fallback default (load failed).
/// Auto-save MUST NOT overwrite the real workspace.json in this state.
static LOADED_FROM_DEFAULT: AtomicBool = AtomicBool::new(false);



// Re-export from settings module for backward compatibility
#[allow(unused_imports)]
pub use super::settings::{
    load_settings, save_settings, get_settings_path,
    AppSettings, CursorShape, SidebarSettings,
    DEFAULT_SIDEBAR_WIDTH, MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH,
    SETTINGS_VERSION,
};

/// Current workspace schema version - increment when making breaking changes
pub const WORKSPACE_VERSION: u32 = 2;

/// Get the config directory for the active profile.
///
/// Falls back to the legacy flat layout path if profiles are not yet initialized
/// (e.g. during early CLI dispatch before `init_profile` is called).
pub fn get_config_dir() -> PathBuf {
    if let Some(p) = velowork_core::profiles::try_current() {
        p.root.clone()
    } else {
        velowork_core::profiles::config_root()
    }
}

/// Alias for `get_config_dir` (used by remote/auth, remote/server, session manager UI)
pub fn config_dir() -> PathBuf {
    get_config_dir()
}

/// Get the workspace file path
pub fn get_workspace_path() -> PathBuf {
    if let Some(p) = velowork_core::profiles::try_current() {
        p.workspace_json()
    } else {
        get_config_dir().join("workspace.json")
    }
}

/// Acquire a lock file to prevent multiple instances from running simultaneously.
/// Returns a held `LockGuard` that releases the lock on drop.
/// If another instance is already running, returns an error with its PID.
pub fn acquire_instance_lock() -> Result<LockGuard> {
    let _slow = velowork_core::timing::SlowGuard::new("acquire_instance_lock");
    let lock_path = velowork_core::profiles::try_current()
        .map(|p| p.lock_path())
        .unwrap_or_else(|| get_config_dir().join("velowork.lock"));

    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Check if a lock file already exists with a live process
    if lock_path.exists()
        && let Ok(content) = std::fs::read_to_string(&lock_path)
            && let Ok(pid) = content.trim().parse::<u32>() {
                if is_process_alive(pid) {
                    anyhow::bail!(
                        "Another Velowork instance is already running (PID {pid}). \
                         If this is incorrect, delete {lock_path:?} and try again."
                    );
                }
                // Stale lock file from a crashed process — safe to take over
                log::info!("[workspace:persistence] Removing stale lock file | pid={pid}");
            }

    let my_pid = std::process::id();
    std::fs::write(&lock_path, my_pid.to_string())?;

    Ok(LockGuard { path: lock_path })
}

/// Guard that removes the lock file on drop
pub struct LockGuard {
    path: PathBuf,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Check whether a process with the given PID is still alive
fn is_process_alive(pid: u32) -> bool {
    let _slow = velowork_core::timing::SlowGuard::new("is_process_alive");
    #[cfg(unix)]
    {
        // kill(pid, 0) checks existence without sending a signal
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
    #[cfg(windows)]
    {
        // On Windows, try tasklist to check if PID exists
        velowork_core::process::safe_output(
            velowork_core::process::command("tasklist")
                .args(["/FI", &format!("PID eq {pid}"), "/NH"]),
        )
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
            .unwrap_or(false)
    }
}

/// Validate and fix workspace data consistency.
/// Called after deserialization in all load paths.
pub(crate) fn validate_workspace_data(
    data: &mut WorkspaceData,
    clear_terminal_ids: bool,
    #[cfg_attr(not(windows), allow(unused))]
    backend_preference: SessionBackend,
) {
    // Auto-detect WSL default shell for projects with WSL UNC paths that don't have it set.
    // This must run BEFORE clearing terminal IDs so we can check WSL backend availability.
    #[cfg(windows)]
    for project in &mut data.projects {
        if project.default_shell.is_none() {
            if let Some((distro, _)) = velowork_terminal::shell_config::parse_wsl_unc_path(&project.path) {
                project.default_shell = Some(velowork_terminal::shell_config::ShellType::Wsl {
                    distro: Some(distro),
                });
            }
        }
    }

    // Optionally clear terminal IDs (on app restart without session persistence).
    // On Windows, WSL projects may have their own session backend (dtach/tmux/screen)
    // even though the host has none — preserve their terminal IDs for reconnection.
    if clear_terminal_ids {
        for project in &mut data.projects {
            #[cfg(windows)]
            {
                use velowork_terminal::shell_config::ShellType;
                if let Some(ShellType::Wsl { distro }) = &project.default_shell {
                    let wsl_backend = velowork_terminal::session_backend::resolve_for_wsl(
                        distro.as_deref(),
                        backend_preference,
                    );
                    if wsl_backend.supports_persistence() {
                        // WSL project with session backend — keep terminal IDs for reconnection
                        continue;
                    }
                }
            }
            if let Some(ref mut layout) = project.layout {
                layout.clear_terminal_ids_except(&std::collections::HashSet::new());
            }
            project.service_terminals.clear();
        }
    }

    // Normalize layout trees (flatten redundant nesting, unwrap single-child containers)
    for project in &mut data.projects {
        if let Some(ref mut layout) = project.layout {
            layout.normalize();
        }
    }

    // Clean up orphaned terminal metadata (terminal_names/hidden_terminals entries
    // for terminals no longer in the layout tree)
    for project in &mut data.projects {
        let layout_ids: std::collections::HashSet<String> = project.layout.as_ref()
            .map(|l| l.collect_terminal_ids().into_iter().collect())
            .unwrap_or_default();
        project.terminal_names.retain(|id, _| layout_ids.contains(id));
        project.hidden_terminals.retain(|id, _| layout_ids.contains(id));
    }

    // Ensure project_order contains all project IDs (that aren't in a folder)
    let folder_project_ids: std::collections::HashSet<String> = data.folders.iter()
        .flat_map(|f| f.project_ids.iter().cloned())
        .collect();
    for project in &data.projects {
        if !data.project_order.contains(&project.id)
            && !folder_project_ids.contains(&project.id)
        {
            data.project_order.push(project.id.clone());
        }
    }

    // Folder consistency checks
    {
        let valid_project_ids: std::collections::HashSet<&str> = data.projects.iter().map(|p| p.id.as_str()).collect();

        // Remove stale project refs from folders
        for folder in &mut data.folders {
            folder.project_ids.retain(|pid| valid_project_ids.contains(pid.as_str()));
        }

        // Ensure folder IDs in project_order match actual folders
        let valid_folder_ids: std::collections::HashSet<&str> = data.folders.iter().map(|f| f.id.as_str()).collect();
        data.project_order.retain(|id| {
            valid_project_ids.contains(id.as_str()) || valid_folder_ids.contains(id.as_str())
        });
    }

    // Drop per-window references (hidden set, widths, folder-collapse, filter)
    // to projects/folders that no longer exist. In-app deletes scrub eagerly;
    // this is the load-time safety net for state that bypassed that path.
    data.scrub_orphan_window_state();
}

pub fn load_workspace(backend: SessionBackend) -> Result<WorkspaceData> {
    if let Some(db) = velowork_core::storage::database() {
        let repo = crate::repositories::workspace::WorkspaceRepository::new(db.clone());
        if let Ok(Some(mut data)) = repo.load_workspace(crate::repositories::workspace::WorkspaceRepository::DEFAULT_WORKSPACE_ID) {
            data = migrate_workspace(data);
            let session_backend = backend.resolve();
            let clear_ids = !session_backend.supports_persistence();
            validate_workspace_data(&mut data, clear_ids, backend);
            return Ok(data);
        }
    }

    Ok(default_workspace())
}

/// Save workspace to database via `WorkspaceRepository`.
/// Remote projects are excluded.
pub fn save_workspace(data: &WorkspaceData) -> Result<()> {
    let _slow = velowork_core::timing::SlowGuard::new("save_workspace");
    if LOADED_FROM_DEFAULT.load(Ordering::Relaxed) {
        log::warn!("[workspace] Skipping save: workspace was loaded from fallback default");
        return Ok(());
    }
    let local_data = data.without_remote_projects();

    if let Some(db) = velowork_core::storage::database() {
        let repo = crate::repositories::workspace::WorkspaceRepository::new(db);
        repo.save_workspace(
            crate::repositories::workspace::WorkspaceRepository::DEFAULT_WORKSPACE_ID,
            Some("Default Workspace"),
            &local_data,
        )?;
    } else {
        log::warn!("[workspace] Database not available; workspace state not persisted");
    }

    Ok(())
}

/// Ensure workspace version is current.
pub(crate) fn migrate_workspace(mut data: WorkspaceData) -> WorkspaceData {
    data.version = WORKSPACE_VERSION;
    data
}

/// Create a default workspace with one project
pub fn default_workspace() -> WorkspaceData {
    let project_id = uuid::Uuid::new_v4().to_string();
    let home_dir = dirs::home_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "/".to_string());

    WorkspaceData {
        version: WORKSPACE_VERSION,
        projects: vec![ProjectData {
            id: project_id.clone(),
            name: "Default".to_string(),
            path: home_dir,
            layout: Some(LayoutNode::new_terminal()),
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
        }],
        project_order: vec![project_id],
        service_panel_heights: HashMap::new(),
        folders: Vec::new(),
        main_window: WindowState::default(),
        extra_windows: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{FolderData, SplitDirection};

    fn make_project(id: &str) -> ProjectData {
        ProjectData {
            id: id.to_string(),
            name: format!("Project {}", id),
            path: "/tmp/test".to_string(),
            layout: Some(LayoutNode::new_terminal()),
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

    fn make_workspace(projects: Vec<ProjectData>, order: Vec<&str>, folders: Vec<FolderData>) -> WorkspaceData {
        WorkspaceData {
            version: WORKSPACE_VERSION,
            projects,
            project_order: order.into_iter().map(String::from).collect(),
            service_panel_heights: HashMap::new(),
            folders,
            main_window: WindowState::default(),
            extra_windows: Vec::new(),
        }
    }

    // === validate_workspace_data ===

    #[test]
    fn validate_orphaned_project_added_to_order() {
        let mut data = make_workspace(
            vec![make_project("p1"), make_project("p2")],
            vec!["p1"], // p2 is orphaned
            vec![],
        );
        validate_workspace_data(&mut data, false, SessionBackend::None);
        assert!(data.project_order.contains(&"p2".to_string()));
    }

    #[test]
    fn validate_stale_folder_refs_removed() {
        let mut data = make_workspace(
            vec![make_project("p1")],
            vec!["f1", "p1"],
            vec![FolderData {
                id: "f1".to_string(),
                name: "Folder".to_string(),
                project_ids: vec!["p1".to_string(), "deleted_project".to_string()],
                folder_color: FolderColor::default(),
            }],
        );
        validate_workspace_data(&mut data, false, SessionBackend::None);
        assert_eq!(data.folders[0].project_ids, vec!["p1".to_string()]);
    }

    #[test]
    fn validate_invalid_folder_id_removed_from_order() {
        let mut data = make_workspace(
            vec![make_project("p1")],
            vec!["nonexistent_folder", "p1"],
            vec![],
        );
        validate_workspace_data(&mut data, false, SessionBackend::None);
        assert!(!data.project_order.contains(&"nonexistent_folder".to_string()));
        assert!(data.project_order.contains(&"p1".to_string()));
    }

    #[test]
    fn validate_clear_terminal_ids() {
        let mut project = make_project("p1");
        project.layout = Some(LayoutNode::Terminal {
            terminal_id: Some("tid1".to_string()),
            minimized: true,
            detached: true,
            shell_type: velowork_terminal::shell_config::ShellType::Default,
            zoom_level: 1.0,
        });
        project.service_terminals.insert("web".to_string(), "svc-term-1".to_string());
        let mut data = make_workspace(vec![project], vec!["p1"], vec![]);
        validate_workspace_data(&mut data, true, SessionBackend::None);

        let layout = data.projects[0].layout.as_ref().unwrap();
        match layout {
            LayoutNode::Terminal { terminal_id, minimized, detached, .. } => {
                assert!(terminal_id.is_none());
                assert!(!minimized);
                assert!(!detached);
            }
            _ => panic!("Expected terminal"),
        }
        assert!(data.projects[0].service_terminals.is_empty());
    }

    #[test]
    fn validate_layout_normalization() {
        let mut project = make_project("p1");
        // Single-child split should normalize to just the child
        project.layout = Some(LayoutNode::Split {
            direction: SplitDirection::Horizontal,
            sizes: vec![100.0],
            children: vec![LayoutNode::new_terminal()],
        });
        let mut data = make_workspace(vec![project], vec!["p1"], vec![]);
        validate_workspace_data(&mut data, false, SessionBackend::None);

        assert!(matches!(data.projects[0].layout, Some(LayoutNode::Terminal { .. })));
    }

    #[test]
    fn validate_combined_issues() {
        let mut data = make_workspace(
            vec![make_project("p1"), make_project("p2"), make_project("p3")],
            vec!["bad_folder", "p1"], // p2, p3 orphaned; bad_folder invalid
            vec![FolderData {
                id: "f1".to_string(),
                name: "Folder".to_string(),
                project_ids: vec!["p3".to_string(), "deleted".to_string()],
                folder_color: FolderColor::default(),
            }],
        );
        // Note: f1 is in folders but not in project_order
        data.project_order.push("f1".to_string());

        validate_workspace_data(&mut data, false, SessionBackend::None);

        // bad_folder should be removed (not a valid project or folder)
        assert!(!data.project_order.contains(&"bad_folder".to_string()));
        // p2 should be added (orphaned, not in any folder)
        assert!(data.project_order.contains(&"p2".to_string()));
        // f1 should remain (valid folder)
        assert!(data.project_order.contains(&"f1".to_string()));
        // Stale ref 'deleted' removed from folder
        assert_eq!(data.folders[0].project_ids, vec!["p3".to_string()]);
    }

    // === migrate_workspace ===

    #[test]
    fn migrate_v0_bumps_to_current_version() {
        let data = WorkspaceData {
            version: 0,
            projects: vec![],
            project_order: vec![],
            service_panel_heights: HashMap::new(),
            folders: vec![],
            main_window: WindowState::default(),
            extra_windows: Vec::new(),
        };
        let migrated = migrate_workspace(data);
        assert_eq!(migrated.version, WORKSPACE_VERSION);
    }

    // === Workspace version tests ===

    #[test]
    fn migrate_current_version_noop() {
        let data = WorkspaceData {
            version: WORKSPACE_VERSION,
            projects: vec![],
            project_order: vec![],
            service_panel_heights: HashMap::new(),
            folders: vec![],
            main_window: WindowState::default(),
            extra_windows: Vec::new(),
        };
        let migrated = migrate_workspace(data);
        assert_eq!(migrated.version, WORKSPACE_VERSION);
    }

    // === Serialization ===

    #[test]
    fn default_workspace_round_trips() {
        let data = default_workspace();
        let json = serde_json::to_string(&data).unwrap();
        let deserialized: WorkspaceData = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.projects.len(), 1);
        assert_eq!(deserialized.project_order.len(), 1);
        assert_eq!(deserialized.version, WORKSPACE_VERSION);
    }

    #[test]
    fn workspace_with_folders_round_trips() {
        // Legacy `FolderData.collapsed` and top-level `project_widths` are
        // tombstoned on save (skip_serializing); per-window state lives on
        // `main_window.folder_collapsed` and `main_window.project_widths`.
        let mut data = make_workspace(
            vec![make_project("p1"), make_project("p2")],
            vec!["f1", "p1"],
            vec![FolderData {
                id: "f1".to_string(),
                name: "My Folder".to_string(),
                project_ids: vec!["p2".to_string()],
                folder_color: FolderColor::default(),
            }],
        );
        data.main_window.folder_collapsed.insert("f1".to_string(), true);
        data.main_window.project_widths.insert("p1".to_string(), 60.0);

        let json = serde_json::to_string(&data).unwrap();
        let deserialized: WorkspaceData = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.folders.len(), 1);
        assert_eq!(deserialized.folders[0].name, "My Folder");
        assert_eq!(deserialized.main_window.folder_collapsed.get("f1"), Some(&true));
        assert_eq!(deserialized.main_window.project_widths.get("p1"), Some(&60.0));
    }

    #[test]
    fn validate_cleans_orphaned_terminal_metadata() {
        let mut project = make_project("p1");
        project.layout = Some(LayoutNode::Terminal {
            terminal_id: Some("t1".to_string()),
            minimized: false,
            detached: false,
            shell_type: velowork_terminal::shell_config::ShellType::Default,
            zoom_level: 1.0,
        });
        // t1 is in layout, t2 and t3 are orphaned
        project.terminal_names.insert("t1".to_string(), "Term 1".to_string());
        project.terminal_names.insert("t2".to_string(), "Term 2".to_string());
        project.terminal_names.insert("t3".to_string(), "Term 3".to_string());
        project.hidden_terminals.insert("t2".to_string(), true);

        let mut data = make_workspace(vec![project], vec!["p1"], vec![]);
        validate_workspace_data(&mut data, false, SessionBackend::None);

        assert!(data.projects[0].terminal_names.contains_key("t1"));
        assert!(!data.projects[0].terminal_names.contains_key("t2"));
        assert!(!data.projects[0].terminal_names.contains_key("t3"));
        assert!(!data.projects[0].hidden_terminals.contains_key("t2"));
    }

    #[test]
    fn validate_cleans_all_metadata_when_no_layout() {
        let mut project = make_project("p1");
        project.layout = None;
        project.terminal_names.insert("t1".to_string(), "Term 1".to_string());
        project.terminal_names.insert("t2".to_string(), "Term 2".to_string());

        let mut data = make_workspace(vec![project], vec!["p1"], vec![]);
        validate_workspace_data(&mut data, false, SessionBackend::None);

        assert!(data.projects[0].terminal_names.is_empty());
    }

    #[test]
    fn without_remote_projects_filters_correctly() {
        // Create mixed local + remote workspace data
        let local = make_project("local1");
        let mut remote1 = make_project("remote:conn1:p1");
        remote1.is_remote = true;
        remote1.connection_id = Some("conn1".to_string());
        let mut remote2 = make_project("remote:conn1:p2");
        remote2.is_remote = true;
        remote2.connection_id = Some("conn1".to_string());

        let data = make_workspace(
            vec![local, remote1, remote2],
            vec!["local1", "remote:conn1:folder1"],
            vec![FolderData {
                id: "remote:conn1:folder1".to_string(),
                name: "Server 1".to_string(),
                project_ids: vec!["remote:conn1:p1".to_string(), "remote:conn1:p2".to_string()],
                folder_color: FolderColor::default(),
            }],
        );
        let filtered = data.without_remote_projects();

        // Remote projects should be filtered out
        assert_eq!(filtered.projects.len(), 1);
        assert_eq!(filtered.projects[0].id, "local1");

        // Remote folder should be filtered out
        assert!(filtered.folders.is_empty());

        // Remote folder should be removed from project_order
        assert_eq!(filtered.project_order, vec!["local1".to_string()]);
    }
}
