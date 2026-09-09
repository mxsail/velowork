use gpui::Context;
use velowork_state::{SessionTreeNode, SshSession};

use super::{ImportResult, ImportedSession};
use crate::stores::SessionStore;

/// Strategy to handle session name collisions during import.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DuplicateStrategy {
    /// Overwrite existing session with matching name under the same folder.
    #[default]
    Overwrite,
    /// Keep existing session and append `(1)`, `(2)`, ... suffix to imported session.
    Rename,
    /// Skip importing the session if a session with the same name already exists.
    Skip,
}

impl DuplicateStrategy {
    pub fn all() -> &'static [DuplicateStrategy] {
        &[
            DuplicateStrategy::Overwrite,
            DuplicateStrategy::Rename,
            DuplicateStrategy::Skip,
        ]
    }

    pub fn id(&self) -> &'static str {
        match self {
            Self::Overwrite => "overwrite",
            Self::Rename => "rename",
            Self::Skip => "skip",
        }
    }
}

/// Import parsed sessions into the `SessionStore` for a specific project.
pub fn import_sessions_into_store(
    store: &mut SessionStore,
    project_id: Option<&str>,
    imported: Vec<ImportedSession>,
    duplicate_strategy: DuplicateStrategy,
    cx: &mut Context<SessionStore>,
) -> ImportResult {
    let mut result = ImportResult {
        total_found: imported.len(),
        imported_sessions: 0,
        renamed_sessions: 0,
        overwritten_sessions: 0,
        skipped_sessions: 0,
        created_folders: 0,
        warnings: Vec::new(),
    };

    if imported.is_empty() {
        return result;
    }

    for sess in imported {
        if sess.host.trim().is_empty() {
            result.warnings.push(format!("Skipped session '{}' with empty host", sess.name));
            continue;
        }

        // 1. Resolve or create folder hierarchy
        let mut current_parent_id: Option<String> = None;
        if let Some(segments) = sess.group_path {
            for segment in segments {
                let segment_trimmed = segment.trim().to_string();
                if segment_trimmed.is_empty() {
                    continue;
                }

                // Check if folder already exists in current parent
                let existing_folder_id = {
                    let tree = store.tree_for_project(project_id);
                    find_child_folder_id_by_name(tree, current_parent_id.as_deref(), &segment_trimmed)
                };

                if let Some(fid) = existing_folder_id {
                    current_parent_id = Some(fid);
                } else {
                    // Create new folder
                    if let Some(new_fid) = store.add_folder_for_project(
                        project_id,
                        current_parent_id.as_deref(),
                        segment_trimmed,
                        cx,
                    ) {
                        result.created_folders += 1;
                        current_parent_id = Some(new_fid);
                    }
                }
            }
        }

        let raw_name = if sess.name.trim().is_empty() {
            sess.host.clone()
        } else {
            sess.name.trim().to_string()
        };

        // 2. Check for duplicate session name in current parent
        let existing_session_id = {
            let tree = store.tree_for_project(project_id);
            find_child_session_id_by_name(tree, current_parent_id.as_deref(), &raw_name)
        };

        match (existing_session_id, duplicate_strategy) {
            (Some(existing_id), DuplicateStrategy::Overwrite) => {
                let ssh_session = SshSession {
                    id: existing_id,
                    protocol: sess.protocol,
                    name: raw_name,
                    host: sess.host,
                    port: sess.port,
                    username: sess.username,
                    auth_type: sess.auth_type,
                    parent_folder_id: current_parent_id.clone(),
                    notes: sess.description,
                    ..Default::default()
                };
                store.update_session_for_project(project_id, ssh_session, cx);
                result.overwritten_sessions += 1;
                result.imported_sessions += 1;
            }
            (Some(_), DuplicateStrategy::Skip) => {
                result.skipped_sessions += 1;
            }
            (Some(_), DuplicateStrategy::Rename) => {
                let tree = store.tree_for_project(project_id);
                let unique_name = generate_unique_session_name(tree, current_parent_id.as_deref(), &raw_name);
                let session_id = uuid::Uuid::new_v4().to_string();
                let ssh_session = SshSession {
                    id: session_id,
                    protocol: sess.protocol,
                    name: unique_name,
                    host: sess.host,
                    port: sess.port,
                    username: sess.username,
                    auth_type: sess.auth_type,
                    parent_folder_id: current_parent_id.clone(),
                    notes: sess.description,
                    ..Default::default()
                };
                store.add_session_for_project(
                    project_id,
                    current_parent_id.as_deref(),
                    ssh_session,
                    cx,
                );
                result.renamed_sessions += 1;
                result.imported_sessions += 1;
            }
            (None, _) => {
                // Sibling folder name check (fallback to unique rename if a folder has the exact same name)
                let final_name = {
                    let tree = store.tree_for_project(project_id);
                    if SessionStore::node_name_exists_in_parent(
                        tree,
                        current_parent_id.as_deref(),
                        &raw_name,
                        None,
                    ) {
                        result.renamed_sessions += 1;
                        generate_unique_session_name(tree, current_parent_id.as_deref(), &raw_name)
                    } else {
                        raw_name
                    }
                };

                let session_id = uuid::Uuid::new_v4().to_string();
                let ssh_session = SshSession {
                    id: session_id,
                    protocol: sess.protocol,
                    name: final_name,
                    host: sess.host,
                    port: sess.port,
                    username: sess.username,
                    auth_type: sess.auth_type,
                    parent_folder_id: current_parent_id.clone(),
                    notes: sess.description,
                    ..Default::default()
                };

                store.add_session_for_project(
                    project_id,
                    current_parent_id.as_deref(),
                    ssh_session,
                    cx,
                );
                result.imported_sessions += 1;
            }
        }
    }

    result
}

/// Find a direct child folder with `name` under `parent_id`.
fn find_child_folder_id_by_name(
    nodes: &[SessionTreeNode],
    parent_id: Option<&str>,
    name: &str,
) -> Option<String> {
    let target_name = name.trim();
    let siblings: &[SessionTreeNode] = match parent_id {
        Some(pid) => match crate::ssh_sessions::find_node_in_tree(nodes, pid) {
            Some(SessionTreeNode::Folder { children, .. }) => children.as_slice(),
            _ => return None,
        },
        None => nodes,
    };

    for node in siblings {
        if let SessionTreeNode::Folder { id, name, .. } = node {
            if name.trim() == target_name {
                return Some(id.clone());
            }
        }
    }
    None
}

/// Find a direct child session with `name` under `parent_id`.
fn find_child_session_id_by_name(
    nodes: &[SessionTreeNode],
    parent_id: Option<&str>,
    name: &str,
) -> Option<String> {
    let target_name = name.trim();
    let siblings: &[SessionTreeNode] = match parent_id {
        Some(pid) => match crate::ssh_sessions::find_node_in_tree(nodes, pid) {
            Some(SessionTreeNode::Folder { children, .. }) => children.as_slice(),
            _ => return None,
        },
        None => nodes,
    };

    for node in siblings {
        if let SessionTreeNode::Session { session } = node {
            if session.name.trim() == target_name {
                return Some(session.id.clone());
            }
        }
    }
    None
}

/// Generate a unique sibling session name with `(1)`, `(2)`, ... suffix.
fn generate_unique_session_name(
    nodes: &[SessionTreeNode],
    parent_id: Option<&str>,
    base_name: &str,
) -> String {
    for n in 1u32.. {
        let candidate = format!("{base_name} ({n})");
        if !SessionStore::node_name_exists_in_parent(nodes, parent_id, &candidate, None) {
            return candidate;
        }
    }
    format!("{base_name} (imported)")
}

/// Pure helper to merge imported sessions into a raw `SessionTreeNode` vector (useful for testing).
pub fn merge_imported_sessions_into_tree(
    nodes: &mut Vec<SessionTreeNode>,
    imported: Vec<ImportedSession>,
    duplicate_strategy: DuplicateStrategy,
) -> ImportResult {
    let mut result = ImportResult {
        total_found: imported.len(),
        imported_sessions: 0,
        renamed_sessions: 0,
        overwritten_sessions: 0,
        skipped_sessions: 0,
        created_folders: 0,
        warnings: Vec::new(),
    };

    for sess in imported {
        if sess.host.trim().is_empty() {
            result.warnings.push(format!("Skipped session '{}' with empty host", sess.name));
            continue;
        }

        let mut current_parent_id: Option<String> = None;
        if let Some(segments) = sess.group_path {
            for segment in segments {
                let segment_trimmed = segment.trim().to_string();
                if segment_trimmed.is_empty() {
                    continue;
                }

                let existing = find_child_folder_id_by_name(nodes, current_parent_id.as_deref(), &segment_trimmed);
                if let Some(fid) = existing {
                    current_parent_id = Some(fid);
                } else {
                    let new_id = uuid::Uuid::new_v4().to_string();
                    let new_folder = SessionTreeNode::Folder {
                        id: new_id.clone(),
                        name: segment_trimmed,
                        children: Vec::new(),
                        is_collapsed: false,
                    };
                    crate::ssh_sessions::add_node_to_tree(nodes, current_parent_id.as_deref(), new_folder);
                    result.created_folders += 1;
                    current_parent_id = Some(new_id);
                }
            }
        }

        let raw_name = if sess.name.trim().is_empty() {
            sess.host.clone()
        } else {
            sess.name.trim().to_string()
        };

        let existing_session_id = find_child_session_id_by_name(nodes, current_parent_id.as_deref(), &raw_name);

        match (existing_session_id, duplicate_strategy) {
            (Some(existing_id), DuplicateStrategy::Overwrite) => {
                let ssh_session = SshSession {
                    id: existing_id,
                    protocol: sess.protocol,
                    name: raw_name,
                    host: sess.host,
                    port: sess.port,
                    username: sess.username,
                    auth_type: sess.auth_type,
                    parent_folder_id: current_parent_id.clone(),
                    notes: sess.description,
                    ..Default::default()
                };
                crate::ssh_sessions::update_session_in_tree(nodes, ssh_session);
                result.overwritten_sessions += 1;
                result.imported_sessions += 1;
            }
            (Some(_), DuplicateStrategy::Skip) => {
                result.skipped_sessions += 1;
            }
            (Some(_), DuplicateStrategy::Rename) => {
                let unique_name = generate_unique_session_name(nodes, current_parent_id.as_deref(), &raw_name);
                let session_id = uuid::Uuid::new_v4().to_string();
                let ssh_session = SshSession {
                    id: session_id,
                    protocol: sess.protocol,
                    name: unique_name,
                    host: sess.host,
                    port: sess.port,
                    username: sess.username,
                    auth_type: sess.auth_type,
                    parent_folder_id: current_parent_id.clone(),
                    notes: sess.description,
                    ..Default::default()
                };
                let node = SessionTreeNode::Session { session: ssh_session };
                crate::ssh_sessions::add_node_to_tree(nodes, current_parent_id.as_deref(), node);
                result.renamed_sessions += 1;
                result.imported_sessions += 1;
            }
            (None, _) => {
                let final_name = if SessionStore::node_name_exists_in_parent(
                    nodes,
                    current_parent_id.as_deref(),
                    &raw_name,
                    None,
                ) {
                    result.renamed_sessions += 1;
                    generate_unique_session_name(nodes, current_parent_id.as_deref(), &raw_name)
                } else {
                    raw_name
                };

                let session_id = uuid::Uuid::new_v4().to_string();
                let ssh_session = SshSession {
                    id: session_id,
                    protocol: sess.protocol,
                    name: final_name,
                    host: sess.host,
                    port: sess.port,
                    username: sess.username,
                    auth_type: sess.auth_type,
                    parent_folder_id: current_parent_id.clone(),
                    notes: sess.description,
                    ..Default::default()
                };

                let node = SessionTreeNode::Session { session: ssh_session };
                crate::ssh_sessions::add_node_to_tree(nodes, current_parent_id.as_deref(), node);
                result.imported_sessions += 1;
            }
        }
    }

    result
}
