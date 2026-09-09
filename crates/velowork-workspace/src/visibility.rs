//! Pure logic for computing the visible set of projects in the sidebar.
//!
//! Folder filter and focus override interact non-trivially. Keeping this in
//! its own module lets us unit-test the tricky cases without constructing a
//! GPUI entity.


use crate::state::{ProjectData, WindowState, WorkspaceData};

/// Compute the ordered list of visible projects given current workspace state.
///
/// Rules:
/// - When a project is focused, only that project is shown.
/// - When the window has a folder filter, top-level projects are hidden and
///   only projects inside the filtered folder are shown. Focus override still
///   wins.
pub fn compute_visible_projects<'a>(
    data: &'a WorkspaceData,
    focused: Option<&String>,
    _focus_individual: bool,
    window: &WindowState,
) -> Vec<&'a ProjectData> {
    let folder_filter = window.folder_filter.as_ref();

    let mut result = Vec::new();
    for id in &data.project_order {
        if let Some(folder) = data.folders.iter().find(|f| f.id == *id) {
            // When folder filter is active, skip folders that don't match
            if let Some(filter_id) = folder_filter
                && &folder.id != filter_id {
                    // Still allow the focused project through
                    if focused.is_some() {
                        for pid in &folder.project_ids {
                            if let Some(p) = data.projects.iter().find(|p| &p.id == pid) {
                                push_project(data, p, focused, window, &mut result);
                            }
                        }
                    }
                    continue;
                }
            for pid in &folder.project_ids {
                if let Some(p) = data.projects.iter().find(|p| p.id == *pid) {
                    push_project(data, p, focused, window, &mut result);
                }
            }
        } else if let Some(p) = data.projects.iter().find(|p| p.id == *id) {
            // Top-level project: hide when folder filter is active
            if folder_filter.is_some() {
                // Still allow the focused project through
                if focused.is_some() {
                    push_project(data, p, focused, window, &mut result);
                }
                continue;
            }
            push_project(data, p, focused, window, &mut result);
        }
    }

    result
}

/// Push a project into the result list, respecting focus filtering: when a
/// project is focused, only that project is shown.
fn push_project<'a>(
    data: &'a WorkspaceData,
    p: &'a ProjectData,
    focused: Option<&String>,
    window: &WindowState,
    result: &mut Vec<&'a ProjectData>,
) {
    match focused {
        None => {
            if !window.hidden_project_ids.contains(&p.id) {
                result.push(p);
            }
        }
        Some(fid) => {
            if &p.id == fid {
                result.push(p);
            }
        }
    }
    // `data` is retained for signature symmetry with callers.
    let _ = data;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{FolderData, LayoutNode};
    use velowork_core::theme::FolderColor;
    use velowork_terminal::shell_config::ShellType;
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

    fn make_data(projects: Vec<ProjectData>, order: Vec<&str>, hidden: &[&str]) -> WorkspaceData {
        // Per-window viewport model: hidden state is set explicitly via
        // `main_window.hidden_project_ids`. Tests that don't exercise hidden
        // behavior pass an empty `hidden` slice.
        let main_window = WindowState {
            hidden_project_ids: hidden.iter().map(|s| s.to_string()).collect(),
            ..WindowState::default()
        };
        WorkspaceData {
            version: 1,
            projects,
            project_order: order.into_iter().map(String::from).collect(),
            service_panel_heights: HashMap::new(),
            folders: Vec::new(),
            main_window,
            extra_windows: Vec::new(),
        }
    }

    #[test]
    fn filters_hidden_projects() {
        let data = make_data(
            vec![
                make_project("p1"),
                make_project("p2"),
                make_project("p3"),
            ],
            vec!["p1", "p2", "p3"],
            &["p2"],
        );
        let visible = compute_visible_projects(&data, None, false, &data.main_window);
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].id, "p1");
        assert_eq!(visible[1].id, "p3");
    }

    #[test]
    fn focused_project_shown_even_when_hidden() {
        let data = make_data(
            vec![
                make_project("p1"),
                make_project("p2"),
                make_project("p3"),
            ],
            vec!["p1", "p2", "p3"],
            &["p3"],
        );
        let focused = "p3".to_string();
        let visible =
            compute_visible_projects(&data, Some(&focused), false, &data.main_window);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].id, "p3");
    }

    #[test]
    fn folder_expands_children() {
        let mut data = make_data(
            vec![make_project("p1"), make_project("p2")],
            vec!["f1"],
            &[],
        );
        data.folders.push(FolderData {
            id: "f1".to_string(),
            name: "Folder".to_string(),
            project_ids: vec!["p1".to_string(), "p2".to_string()],
            folder_color: FolderColor::default(),
        });
        let visible = compute_visible_projects(&data, None, false, &WindowState::default());
        assert_eq!(visible.len(), 2);
    }

    #[test]
    fn folder_filter_hides_top_level() {
        let mut data = make_data(
            vec![
                make_project("p1"),
                make_project("p2"),
                make_project("p3"),
            ],
            vec!["f1", "p3"],
            &[],
        );
        data.folders.push(FolderData {
            id: "f1".to_string(),
            name: "Folder".to_string(),
            project_ids: vec!["p1".to_string(), "p2".to_string()],
            folder_color: FolderColor::default(),
        });
        let window = WindowState {
            folder_filter: Some("f1".to_string()),
            ..WindowState::default()
        };
        let visible = compute_visible_projects(&data, None, false, &window);
        assert_eq!(visible.len(), 2);
        assert!(visible.iter().all(|p| p.id != "p3"));
    }

    #[test]
    fn hidden_project_ids_hides_projects() {
        // hidden_project_ids on the window state hides projects -- the per-
        // window hidden set is the sole visibility mechanism after the
        // legacy ProjectData.show_in_overview field was removed.
        let data = make_data(
            vec![
                make_project("p1"),
                make_project("p2"),
                make_project("p3"),
            ],
            vec!["p1", "p2", "p3"],
            &[],
        );
        let mut window = WindowState::default();
        window.hidden_project_ids.insert("p2".to_string());
        let visible = compute_visible_projects(&data, None, false, &window);
        let ids: Vec<&str> = visible.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["p1", "p3"]);
    }

    #[test]
    fn folder_filter_sourced_from_window_state() {
        // Same fixture as folder_filter_hides_top_level but routed via
        // WindowState.folder_filter instead of the prior loose argument —
        // verifies the new signature reads filter from the window.
        let mut data = make_data(
            vec![
                make_project("p1"),
                make_project("p2"),
                make_project("p3"),
            ],
            vec!["f1", "p3"],
            &[],
        );
        data.folders.push(FolderData {
            id: "f1".to_string(),
            name: "Folder".to_string(),
            project_ids: vec!["p1".to_string(), "p2".to_string()],
            folder_color: FolderColor::default(),
        });
        let window = WindowState {
            folder_filter: Some("f1".to_string()),
            ..Default::default()
        };
        let visible = compute_visible_projects(&data, None, false, &window);
        let ids: Vec<&str> = visible.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["p1", "p2"]);
    }

    #[test]
    fn focus_override_pierces_folder_filter_for_top_level_project() {
        // Filter pins f1 (p1, p2). A top-level project p3 outside the folder
        // is focused. Focus override must still surface p3 even though the
        // active filter hides every top-level project, and focus semantics
        // mean ONLY the focused project shows. This filter+focus interaction
        // is called out in the PRD and was not covered by any other test.
        let mut data = make_data(
            vec![make_project("p1"), make_project("p2"), make_project("p3")],
            vec!["f1", "p3"],
            &[],
        );
        data.folders.push(FolderData {
            id: "f1".to_string(),
            name: "Folder".to_string(),
            project_ids: vec!["p1".to_string(), "p2".to_string()],
            folder_color: FolderColor::default(),
        });
        let window = WindowState {
            folder_filter: Some("f1".to_string()),
            ..WindowState::default()
        };
        let focused = "p3".to_string();
        let visible = compute_visible_projects(&data, Some(&focused), false, &window);
        let ids: Vec<&str> = visible.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["p3"]);
    }

    #[test]
    fn focus_override_surfaces_project_from_non_matching_folder_under_filter() {
        // Two folders; filter pins f1, but the focused project lives in f2.
        // The non-matching-folder branch must let the focused project through
        // (the focused.is_some() path inside a skipped folder), and nothing
        // from f1 leaks because focus shows only the focused project.
        let mut data = make_data(
            vec![make_project("a1"), make_project("b1")],
            vec!["f1", "f2"],
            &[],
        );
        data.folders.push(FolderData {
            id: "f1".to_string(),
            name: "F1".to_string(),
            project_ids: vec!["a1".to_string()],
            folder_color: FolderColor::default(),
        });
        data.folders.push(FolderData {
            id: "f2".to_string(),
            name: "F2".to_string(),
            project_ids: vec!["b1".to_string()],
            folder_color: FolderColor::default(),
        });
        let window = WindowState {
            folder_filter: Some("f1".to_string()),
            ..WindowState::default()
        };
        let focused = "b1".to_string();
        let visible = compute_visible_projects(&data, Some(&focused), false, &window);
        let ids: Vec<&str> = visible.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["b1"]);
    }
}
