use velowork_state::{SessionTreeNode, SshSession};

pub fn add_node_to_tree(
    nodes: &mut Vec<SessionTreeNode>,
    parent_id: Option<&str>,
    new_node: SessionTreeNode,
) -> bool {
    if let Some(pid) = parent_id {
        for node in nodes.iter_mut() {
            if let SessionTreeNode::Folder { id, children, .. } = node {
                if id == pid {
                    children.push(new_node);
                    sort_siblings(children);
                    return true;
                }
                if add_node_to_tree(children, Some(pid), new_node.clone()) {
                    return true;
                }
            }
        }
        false
    } else {
        nodes.push(new_node);
        sort_siblings(nodes);
        true
    }
}

/// Recursively sort each level of the tree so that session nodes always appear
/// before folder (directory) nodes. Order within the same kind is preserved
/// (stable sort). This canonical sibling ordering is applied whenever the tree
/// is rebuilt from storage and after any in-memory mutation that changes a
/// sibling list, so the rendered tree always honors it without affecting
/// expand/collapse or drag/drop interactions.
pub fn sort_siblings(nodes: &mut [SessionTreeNode]) {
    nodes.sort_by(|a, b| {
        let a_is_folder = matches!(a, SessionTreeNode::Folder { .. });
        let b_is_folder = matches!(b, SessionTreeNode::Folder { .. });
        // Sessions (false) sort before folders (true).
        a_is_folder.cmp(&b_is_folder)
    });
    for node in nodes.iter_mut() {
        if let SessionTreeNode::Folder { children, .. } = node {
            sort_siblings(children);
        }
    }
}

pub fn remove_node_from_tree(nodes: &mut Vec<SessionTreeNode>, target_id: &str) -> bool {
    let mut remove_index = None;
    for (i, node) in nodes.iter_mut().enumerate() {
        if node.id() == target_id {
            remove_index = Some(i);
            break;
        }
        if let SessionTreeNode::Folder { children, .. } = node {
            if remove_index.is_none() && remove_node_from_tree(children, target_id) {
                return true;
            }
        }
    }
    if let Some(idx) = remove_index {
        nodes.remove(idx);
        true
    } else {
        false
    }
}

pub fn update_session_in_tree(nodes: &mut Vec<SessionTreeNode>, session: SshSession) -> bool {
    for node in nodes.iter_mut() {
        match node {
            SessionTreeNode::Session { session: s } => {
                if s.id == session.id {
                    *s = session;
                    return true;
                }
            }
            SessionTreeNode::Folder { children, .. } => {
                if update_session_in_tree(children, session.clone()) {
                    return true;
                }
            }
        }
    }
    false
}

pub fn toggle_folder_collapsed_in_tree(nodes: &mut Vec<SessionTreeNode>, folder_id: &str) -> bool {
    for node in nodes.iter_mut() {
        if let SessionTreeNode::Folder { id, is_collapsed, children, .. } = node {
            if id == folder_id {
                *is_collapsed = !*is_collapsed;
                return true;
            }
            if toggle_folder_collapsed_in_tree(children, folder_id) {
                return true;
            }
        }
    }
    false
}

pub fn set_folder_collapsed_in_tree(nodes: &mut Vec<SessionTreeNode>, folder_id: &str, collapsed: bool) {
    for node in nodes.iter_mut() {
        if let SessionTreeNode::Folder { id, is_collapsed, children, .. } = node {
            if id == folder_id {
                *is_collapsed = collapsed;
                return;
            }
            set_folder_collapsed_in_tree(children, folder_id, collapsed);
        }
    }
}

pub fn set_all_folders_collapsed_in_tree(nodes: &mut Vec<SessionTreeNode>, collapsed: bool) {
    for node in nodes.iter_mut() {
        if let SessionTreeNode::Folder { is_collapsed, children, .. } = node {
            *is_collapsed = collapsed;
            set_all_folders_collapsed_in_tree(children, collapsed);
        }
    }
}

pub fn find_parent_folder_id_in_tree(nodes: &[SessionTreeNode], session_id: &str) -> Option<String> {
    for node in nodes {
        if let SessionTreeNode::Folder { id, children, .. } = node {
            for child in children {
                if child.id() == session_id {
                    return Some(id.clone());
                }
            }
            if let Some(parent_id) = find_parent_folder_id_in_tree(children, session_id) {
                return Some(parent_id);
            }
        }
    }
    None
}

/// Find the index of a node within a specific parent folder's children.
pub fn find_node_index_in_parent(
    nodes: &[SessionTreeNode],
    target_id: &str,
    parent_id: &str,
) -> Option<usize> {
    for node in nodes {
        if let SessionTreeNode::Folder { id, children, .. } = node {
            if id == parent_id {
                return children.iter().position(|c| c.id() == target_id);
            }
            if let Some(idx) = find_node_index_in_parent(children, target_id, parent_id) {
                return Some(idx);
            }
        }
    }
    None
}

/// Check if `descendant_id` is a descendant of `ancestor_id` in the tree.
/// Used to prevent moving a folder into its own subfolder (cycle prevention).
pub fn is_descendant(nodes: &[SessionTreeNode], ancestor_id: &str, descendant_id: &str) -> bool {
    for node in nodes {
        if let SessionTreeNode::Folder { id, children, .. } = node {
            if id == ancestor_id {
                return find_node_in_subtree(children, descendant_id);
            }
            if is_descendant(children, ancestor_id, descendant_id) {
                return true;
            }
        }
    }
    false
}

/// Find a node by id in the tree (returns a shared reference).
pub fn find_node_in_tree<'a>(
    nodes: &'a [SessionTreeNode],
    target_id: &str,
) -> Option<&'a SessionTreeNode> {
    for node in nodes {
        if node.id() == target_id {
            return Some(node);
        }
        if let SessionTreeNode::Folder { children, .. } = node {
            if let Some(found) = find_node_in_tree(children, target_id) {
                return Some(found);
            }
        }
    }
    None
}

/// Collect the names of all session (leaf) nodes that are direct children of
/// `parent_id`. When `parent_id` is `None`, returns the names of all root-level
/// sessions. Used to pick a non-colliding name when duplicating a session.
pub fn sibling_session_names(nodes: &[SessionTreeNode], parent_id: Option<&str>) -> Vec<String> {
    let siblings: &[SessionTreeNode] = match parent_id {
        Some(pid) => match find_node_in_tree(nodes, pid) {
            Some(SessionTreeNode::Folder { children, .. }) => children.as_slice(),
            _ => return Vec::new(),
        },
        None => nodes,
    };
    siblings
        .iter()
        .filter_map(|n| match n {
            SessionTreeNode::Session { session } => Some(session.name.clone()),
            _ => None,
        })
        .collect()
}

fn find_node_in_subtree(nodes: &[SessionTreeNode], target_id: &str) -> bool {
    for node in nodes {
        if node.id() == target_id {
            return true;
        }
        if let SessionTreeNode::Folder { children, .. } = node {
            if find_node_in_subtree(children, target_id) {
                return true;
            }
        }
    }
    false
}

/// Remove a node from the tree and return it (if found).
pub fn take_node_from_tree(nodes: &mut Vec<SessionTreeNode>, target_id: &str) -> Option<SessionTreeNode> {
    for (i, node) in nodes.iter().enumerate() {
        if node.id() == target_id {
            return Some(nodes.remove(i));
        }
    }
    for node in nodes.iter_mut() {
        if let SessionTreeNode::Folder { children, .. } = node {
            if let Some(taken) = take_node_from_tree(children, target_id) {
                return Some(taken);
            }
        }
    }
    None
}

/// Insert a node into a parent folder at a specific index.
/// If `parent_id` is None, inserts at root level.
/// If `index` >= children.len(), appends to the end.
pub fn insert_node_at_index(
    nodes: &mut Vec<SessionTreeNode>,
    parent_id: Option<&str>,
    index: usize,
    new_node: SessionTreeNode,
) -> bool {
    if let Some(pid) = parent_id {
        for node in nodes.iter_mut() {
            if let SessionTreeNode::Folder { id, children, .. } = node {
                if id == pid {
                    let insert_at = index.min(children.len());
                    children.insert(insert_at, new_node);
                    sort_siblings(children);
                    return true;
                }
                if insert_node_at_index(children, Some(pid), index, new_node.clone()) {
                    return true;
                }
            }
        }
        false
    } else {
        let insert_at = index.min(nodes.len());
        nodes.insert(insert_at, new_node);
        sort_siblings(nodes);
        true
    }
}

/// Move a node from one position to another in the tree.
/// Returns false if the source doesn't exist, or if the move would create a cycle.
pub fn move_node_in_tree(
    nodes: &mut Vec<SessionTreeNode>,
    source_id: &str,
    target_parent_id: Option<&str>,
    target_index: usize,
) -> bool {
    // Prevent moving a folder into itself or its descendants
    if let Some(tpid) = target_parent_id {
        if tpid == source_id {
            return false;
        }
        if is_descendant(nodes, source_id, tpid) {
            return false;
        }
    }

    // Remove the node from its current position
    let node = match take_node_from_tree(nodes, source_id) {
        Some(n) => n,
        None => return false,
    };

    // Insert at the target position
    insert_node_at_index(nodes, target_parent_id, target_index, node)
}

